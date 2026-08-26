use std::{
    collections::{HashMap, HashSet, VecDeque},
    fmt,
    future::Future,
    pin::Pin,
    sync::{
        Arc, Weak,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
};

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use tokio::{
    net::TcpStream,
    sync::{Mutex, Notify, mpsc, oneshot, watch},
    task::JoinHandle,
    time::{Instant, timeout, timeout_at},
};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async_with_config,
    tungstenite::{Error as WebSocketError, protocol::WebSocketConfig},
};

use crate::{
    config::{
        MAX_WEBSOCKET_MESSAGE_BYTES, MAX_WEBSOCKET_POLL_BYTES, MAX_WEBSOCKET_POLL_EVENTS,
        MAX_WEBSOCKET_SESSIONS, MAX_WEBSOCKET_SYMBOLS, WEBSOCKET_ACK_TIMEOUT,
        WEBSOCKET_POLL_TIMEOUT,
    },
    error::TiingoError,
    websocket::protocol::{Authorization, ProtocolCodec, Service},
};

const CAPABILITY: &str = "WebSocket market data";
const MAX_RETAINED_TERMINAL_SESSIONS: usize = MAX_WEBSOCKET_SESSIONS;

#[cfg(test)]
static POLL_RESULT_SERIALIZATIONS: AtomicUsize = AtomicUsize::new(0);

mod worker;

type ClientSocket = WebSocketStream<MaybeTlsStream<TcpStream>>;

#[derive(Clone)]
pub struct TiingoConnector {
    iex_endpoint: String,
    consolidated_endpoint: String,
}

impl fmt::Debug for TiingoConnector {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TiingoConnector")
            .field("iex_endpoint", &"[CONFIGURED]")
            .field("consolidated_endpoint", &"[CONFIGURED]")
            .finish()
    }
}

impl TiingoConnector {
    pub fn production() -> Self {
        Self {
            iex_endpoint: Service::Iex.endpoint().to_owned(),
            consolidated_endpoint: Service::Consolidated.endpoint().to_owned(),
        }
    }

    pub fn with_endpoints(
        iex_endpoint: impl Into<String>,
        consolidated_endpoint: impl Into<String>,
    ) -> Result<Self, TiingoError> {
        let connector = Self {
            iex_endpoint: iex_endpoint.into(),
            consolidated_endpoint: consolidated_endpoint.into(),
        };
        for endpoint in [&connector.iex_endpoint, &connector.consolidated_endpoint] {
            let url = url::Url::parse(endpoint)
                .map_err(|_| TiingoError::Validation("WebSocket endpoint is invalid".into()))?;
            if !matches!(url.scheme(), "ws" | "wss") {
                return Err(TiingoError::Validation(
                    "WebSocket endpoint must use ws or wss".into(),
                ));
            }
        }
        Ok(connector)
    }

    fn endpoint(&self, service: Service) -> &str {
        match service {
            Service::Iex => &self.iex_endpoint,
            Service::Consolidated => &self.consolidated_endpoint,
        }
    }

    async fn connect(&self, service: Service) -> Result<ClientSocket, TiingoError> {
        let config = WebSocketConfig::default()
            .max_message_size(Some(MAX_WEBSOCKET_MESSAGE_BYTES))
            .max_frame_size(Some(MAX_WEBSOCKET_MESSAGE_BYTES));
        connect_async_with_config(self.endpoint(service), Some(config), false)
            .await
            .map(|(socket, _)| socket)
            .map_err(map_connect_error)
    }
}

#[derive(Clone)]
pub struct MarketDataRegistry {
    inner: Arc<RegistryInner>,
}

impl fmt::Debug for MarketDataRegistry {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MarketDataRegistry")
            .field(
                "api_key",
                &self.inner.api_key.as_ref().map(|_| "[REDACTED]"),
            )
            .field("connector", &self.inner.connector)
            .finish_non_exhaustive()
    }
}

struct RegistryInner {
    api_key: Option<Authorization>,
    connector: TiingoConnector,
    clock: Arc<dyn ReceiveClock>,
    reconnect_clock: Arc<dyn ReconnectClock>,
    sessions: Mutex<HashMap<String, Arc<Session>>>,
    shutdown_lock: Mutex<()>,
    active_sessions: Arc<AtomicUsize>,
    terminal_counter: Arc<AtomicU64>,
    shutting_down: AtomicBool,
    shutdown_complete: AtomicBool,
}

struct Session {
    cancel: watch::Sender<bool>,
    commands: mpsc::Sender<WorkerCommand>,
    mutation: Mutex<()>,
    join: Mutex<Option<JoinHandle<()>>>,
    data: Mutex<SessionData>,
    notify: Notify,
    active_sessions: Arc<AtomicUsize>,
    active_slot_held: AtomicBool,
    terminal_counter: Arc<AtomicU64>,
    terminal_order: AtomicU64,
}

struct SessionData {
    status: SubscriptionStatus,
    terminal_error: Option<TerminalErrorKind>,
    events: VecDeque<MarketDataEvent>,
    queue_bytes: usize,
    next_sequence: u64,
    seen_observations: HashSet<String>,
    latest_timestamp_by_symbol: HashMap<String, DateTime<Utc>>,
    symbols: Vec<String>,
    threshold_level: u8,
    started_at: Instant,
    last_access: Instant,
}

pub trait ReceiveClock: fmt::Debug + Send + Sync + 'static {
    fn now(&self) -> DateTime<Utc>;
}

#[derive(Debug)]
struct SystemClock;

impl ReceiveClock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

pub trait ReconnectClock: fmt::Debug + Send + Sync + 'static {
    fn sleep(&self, delay: std::time::Duration) -> Pin<Box<dyn Future<Output = ()> + Send + '_>>;
}

#[derive(Debug)]
struct TokioReconnectClock;

impl ReconnectClock for TokioReconnectClock {
    fn sleep(&self, delay: std::time::Duration) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        Box::pin(tokio::time::sleep(delay))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StartRequest {
    pub service: Service,
    pub symbols: Vec<String>,
    pub threshold_level: Option<u8>,
    pub confirm_iex_market_data_agreement: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SubscriptionStatus {
    Starting,
    Active,
    Reconnecting,
    Stopped,
    Expired,
    Failed,
    DataGap,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalErrorKind {
    Authentication,
    Entitlement,
    Transport,
    Protocol,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartResult {
    pub id: String,
    pub state: SubscriptionStatus,
}

#[derive(Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketDataEvent {
    pub sequence: u64,
    pub received_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vendor_timestamp: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    pub duplicate: bool,
    pub out_of_order: bool,
    pub payload: Value,
}

impl fmt::Debug for MarketDataEvent {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MarketDataEvent")
            .field("sequence", &self.sequence)
            .field("received_at", &self.received_at)
            .field("vendor_timestamp", &self.vendor_timestamp)
            .field("symbol", &self.symbol)
            .field("duplicate", &self.duplicate)
            .field("out_of_order", &self.out_of_order)
            .field("payload", &"[REDACTED]")
            .finish()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PollResult {
    pub id: String,
    pub state: SubscriptionStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal_error: Option<TerminalErrorKind>,
    pub events: Vec<MarketDataEvent>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpdateRequest {
    pub add_symbols: Vec<String>,
    pub remove_symbols: Vec<String>,
    pub threshold_level: Option<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateResult {
    pub id: String,
    pub state: SubscriptionStatus,
    pub symbols: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StopResult {
    pub id: String,
    pub state: SubscriptionStatus,
}

enum WorkerCommand {
    Update {
        add_symbols: Vec<String>,
        remove_symbols: Vec<String>,
        response: oneshot::Sender<Result<Vec<String>, TiingoError>>,
    },
}

impl MarketDataRegistry {
    pub fn new(api_key: Option<String>) -> Self {
        Self::with_connector(api_key, TiingoConnector::production())
    }

    pub fn with_connector(api_key: Option<String>, connector: TiingoConnector) -> Self {
        Self::with_connector_and_clock(api_key, connector, Arc::new(SystemClock))
    }

    pub fn with_connector_and_clock(
        api_key: Option<String>,
        connector: TiingoConnector,
        clock: Arc<dyn ReceiveClock>,
    ) -> Self {
        Self::with_connector_and_clocks(api_key, connector, clock, Arc::new(TokioReconnectClock))
    }

    pub fn with_connector_and_clocks(
        api_key: Option<String>,
        connector: TiingoConnector,
        clock: Arc<dyn ReceiveClock>,
        reconnect_clock: Arc<dyn ReconnectClock>,
    ) -> Self {
        Self {
            inner: Arc::new(RegistryInner {
                api_key: api_key
                    .filter(|key| !key.trim().is_empty())
                    .map(Authorization::new),
                connector,
                clock,
                reconnect_clock,
                sessions: Mutex::new(HashMap::new()),
                shutdown_lock: Mutex::new(()),
                active_sessions: Arc::new(AtomicUsize::new(0)),
                terminal_counter: Arc::new(AtomicU64::new(0)),
                shutting_down: AtomicBool::new(false),
                shutdown_complete: AtomicBool::new(false),
            }),
        }
    }

    pub async fn start(&self, request: StartRequest) -> Result<StartResult, TiingoError> {
        let symbols = normalize_symbols(request.symbols)?;
        let threshold_level = request.threshold_level.unwrap_or(6);
        let codec = ProtocolCodec::new(request.service, threshold_level)?;
        if request.service == Service::Iex
            && matches!(threshold_level, 0 | 5)
            && !request.confirm_iex_market_data_agreement
        {
            return Err(TiingoError::Validation(
                "IEX threshold levels 0 and 5 require confirmation of a direct market-data agreement"
                    .into(),
            ));
        }
        let authorization = self
            .inner
            .api_key
            .clone()
            .ok_or_else(|| TiingoError::Configuration("TIINGO_API_KEY is missing".into()))?;
        if self.inner.shutting_down.load(Ordering::Acquire) {
            return Err(TiingoError::Validation(
                "the WebSocket registry is shutting down".into(),
            ));
        }

        let (initial_tx, initial_rx) = oneshot::channel();
        let connector = self.inner.connector.clone();
        let clock = Arc::clone(&self.inner.clock);
        let reconnect_clock = Arc::clone(&self.inner.reconnect_clock);
        let service = request.service;
        let session_id = {
            let mut sessions = self.inner.sessions.lock().await;
            if self.inner.shutting_down.load(Ordering::Acquire) {
                return Err(TiingoError::Validation(
                    "the WebSocket registry is shutting down".into(),
                ));
            }
            if self.inner.active_sessions.load(Ordering::Acquire) >= MAX_WEBSOCKET_SESSIONS {
                return Err(TiingoError::Validation(
                    "at most eight WebSocket subscriptions may be active in this process".into(),
                ));
            }
            prune_terminal_sessions(&mut sessions).await;
            let session_id = loop {
                let candidate = format!("{:032x}", rand::random::<u128>());
                if !sessions.contains_key(&candidate) {
                    break candidate;
                }
            };
            let (cancel, cancel_rx) = watch::channel(false);
            let (commands, command_rx) = mpsc::channel(8);
            let now = Instant::now();
            self.inner.active_sessions.fetch_add(1, Ordering::AcqRel);
            let session = Arc::new(Session {
                cancel,
                commands,
                mutation: Mutex::new(()),
                join: Mutex::new(None),
                data: Mutex::new(SessionData {
                    status: SubscriptionStatus::Starting,
                    terminal_error: None,
                    events: VecDeque::new(),
                    queue_bytes: 0,
                    next_sequence: 1,
                    seen_observations: HashSet::new(),
                    latest_timestamp_by_symbol: HashMap::new(),
                    symbols: symbols.clone(),
                    threshold_level,
                    started_at: now,
                    last_access: now,
                }),
                notify: Notify::new(),
                active_sessions: Arc::clone(&self.inner.active_sessions),
                active_slot_held: AtomicBool::new(true),
                terminal_counter: Arc::clone(&self.inner.terminal_counter),
                terminal_order: AtomicU64::new(0),
            });
            let worker_session = Arc::clone(&session);
            let handle = tokio::spawn(async move {
                worker::run_worker(
                    worker_session,
                    connector,
                    clock,
                    reconnect_clock,
                    codec,
                    service,
                    authorization,
                    symbols,
                    cancel_rx,
                    command_rx,
                    initial_tx,
                )
                .await;
            });
            *session
                .join
                .try_lock()
                .expect("a new WebSocket session has no join-lock contention") = Some(handle);
            sessions.insert(session_id.clone(), Arc::clone(&session));
            session_id
        };

        let mut guard = StartGuard {
            registry: Arc::downgrade(&self.inner),
            session_id: session_id.clone(),
            armed: true,
        };
        let result = match timeout(WEBSOCKET_ACK_TIMEOUT, initial_rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(TiingoError::Transport {
                capability: CAPABILITY,
            }),
            Err(_) => Err(TiingoError::Timeout {
                capability: CAPABILITY,
            }),
        };
        match result {
            Ok(()) => {
                let publication = self.inner.shutdown_lock.lock().await;
                if self.inner.shutting_down.load(Ordering::Acquire) {
                    drop(publication);
                    cleanup_session(&self.inner, &session_id, true).await;
                    guard.armed = false;
                    return Err(TiingoError::Validation(
                        "the WebSocket registry is shutting down".into(),
                    ));
                }
                guard.armed = false;
                Ok(StartResult {
                    id: session_id,
                    state: SubscriptionStatus::Active,
                })
            }
            Err(error) => {
                guard.armed = false;
                cleanup_session(&self.inner, &session_id, true).await;
                Err(error)
            }
        }
    }

    pub async fn poll(
        &self,
        session_id: &str,
        after_sequence: u64,
    ) -> Result<PollResult, TiingoError> {
        self.poll_with_bounds(
            session_id,
            after_sequence,
            MAX_WEBSOCKET_POLL_EVENTS,
            WEBSOCKET_POLL_TIMEOUT,
        )
        .await
    }

    pub async fn poll_with_bounds(
        &self,
        session_id: &str,
        after_sequence: u64,
        limit: usize,
        max_wait: std::time::Duration,
    ) -> Result<PollResult, TiingoError> {
        if limit > MAX_WEBSOCKET_POLL_EVENTS {
            return Err(TiingoError::Validation(
                "a WebSocket poll may return at most 256 events".into(),
            ));
        }
        if max_wait > WEBSOCKET_POLL_TIMEOUT {
            return Err(TiingoError::Validation(
                "a WebSocket poll may wait at most 5000 milliseconds".into(),
            ));
        }
        let session = self.session(session_id).await?;
        touch_session(&session).await;
        let deadline = Instant::now() + max_wait;
        loop {
            let notified = session.notify.notified();
            let result = poll_snapshot(session_id, &session, after_sequence, limit).await?;
            if !result.events.is_empty() || is_terminal(result.state) {
                return Ok(result);
            }
            if timeout_at(deadline, notified).await.is_err() {
                return poll_snapshot(session_id, &session, after_sequence, limit).await;
            }
        }
    }

    pub async fn update(
        &self,
        session_id: &str,
        request: UpdateRequest,
    ) -> Result<UpdateResult, TiingoError> {
        let session = self.session(session_id).await?;
        let _mutation = session.mutation.lock().await;
        touch_session(&session).await;
        let (status, current_threshold, current_symbols) = {
            let data = session.data.lock().await;
            (data.status, data.threshold_level, data.symbols.clone())
        };
        if status != SubscriptionStatus::Active {
            return Err(TiingoError::Validation(
                "only an active WebSocket subscription can be updated".into(),
            ));
        }
        if request
            .threshold_level
            .is_some_and(|threshold| threshold != current_threshold)
        {
            return Err(TiingoError::Validation(
                "threshold changes require stopping this subscription and starting a new one"
                    .into(),
            ));
        }
        let add_symbols = normalize_mutation_symbols(request.add_symbols)?;
        let remove_symbols = normalize_mutation_symbols(request.remove_symbols)?;
        if add_symbols
            .iter()
            .any(|symbol| remove_symbols.contains(symbol))
        {
            return Err(TiingoError::Validation(
                "the same symbol cannot be added and removed in one update".into(),
            ));
        }
        if add_symbols.is_empty() && remove_symbols.is_empty() {
            return Err(TiingoError::Validation(
                "an update must add or remove at least one symbol".into(),
            ));
        }
        worker::validate_resulting_symbols(&current_symbols, &add_symbols, &remove_symbols)?;

        let (response, result) = oneshot::channel();
        session
            .commands
            .send(WorkerCommand::Update {
                add_symbols,
                remove_symbols,
                response,
            })
            .await
            .map_err(|_| TiingoError::Validation("the WebSocket subscription is closed".into()))?;
        let symbols = result.await.map_err(|_| TiingoError::Transport {
            capability: CAPABILITY,
        })??;
        Ok(UpdateResult {
            id: session_id.to_owned(),
            state: SubscriptionStatus::Active,
            symbols,
        })
    }

    pub async fn stop(&self, session_id: &str) -> Result<StopResult, TiingoError> {
        let session = self.session(session_id).await?;
        let _ = session.cancel.send(true);
        let _mutation = session.mutation.lock().await;
        join_session(&session).await;
        set_status(&session, SubscriptionStatus::Stopped).await;
        Ok(StopResult {
            id: session_id.to_owned(),
            state: SubscriptionStatus::Stopped,
        })
    }

    async fn session(&self, session_id: &str) -> Result<Arc<Session>, TiingoError> {
        self.inner
            .sessions
            .lock()
            .await
            .get(session_id)
            .cloned()
            .ok_or_else(|| TiingoError::Validation("unknown WebSocket subscription ID".into()))
    }

    pub async fn shutdown(&self) {
        let _shutdown = self.inner.shutdown_lock.lock().await;
        if self.inner.shutdown_complete.load(Ordering::Acquire) {
            return;
        }
        self.inner.shutting_down.store(true, Ordering::Release);
        let sessions = {
            let sessions = self.inner.sessions.lock().await;
            sessions.values().cloned().collect::<Vec<_>>()
        };
        for session in &sessions {
            let _ = session.cancel.send(true);
        }
        for session in sessions {
            join_session(&session).await;
        }
        self.inner.sessions.lock().await.clear();
        self.inner.shutdown_complete.store(true, Ordering::Release);
    }
}

struct StartGuard {
    registry: Weak<RegistryInner>,
    session_id: String,
    armed: bool,
}

impl Drop for StartGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let Some(registry) = self.registry.upgrade() else {
            return;
        };
        let session_id = self.session_id.clone();
        tokio::spawn(async move {
            cleanup_session(&registry, &session_id, true).await;
        });
    }
}

async fn cleanup_session(registry: &Arc<RegistryInner>, session_id: &str, remove: bool) {
    let session = {
        let sessions = registry.sessions.lock().await;
        sessions.get(session_id).cloned()
    };
    if let Some(session) = session {
        let _ = session.cancel.send(true);
        join_session(&session).await;
    }
    if remove {
        registry.sessions.lock().await.remove(session_id);
    }
}

async fn join_session(session: &Session) {
    let mut join = session.join.lock().await;
    if let Some(handle) = join.as_mut() {
        let _ = handle.await;
    }
    *join = None;
}

async fn set_status(session: &Session, status: SubscriptionStatus) {
    set_status_and_terminal_error(session, status, None).await;
}

async fn set_terminal_failure(session: &Session, terminal_error: TerminalErrorKind) {
    set_status_and_terminal_error(session, SubscriptionStatus::Failed, Some(terminal_error)).await;
}

async fn set_status_and_terminal_error(
    session: &Session,
    status: SubscriptionStatus,
    terminal_error: Option<TerminalErrorKind>,
) {
    {
        let mut data = session.data.lock().await;
        if is_terminal(status) && !is_terminal(data.status) {
            let terminal_order = session.terminal_counter.fetch_add(1, Ordering::AcqRel) + 1;
            session
                .terminal_order
                .store(terminal_order, Ordering::Release);
        }
        data.status = status;
        data.terminal_error = terminal_error;
    }
    if is_terminal(status) && session.active_slot_held.swap(false, Ordering::AcqRel) {
        session.active_sessions.fetch_sub(1, Ordering::AcqRel);
    }
    session.notify.notify_waiters();
}

async fn prune_terminal_sessions(sessions: &mut HashMap<String, Arc<Session>>) {
    loop {
        let oldest = sessions
            .iter()
            .filter_map(|(session_id, session)| {
                let order = session.terminal_order.load(Ordering::Acquire);
                (order != 0).then(|| (order, session_id.clone(), Arc::clone(session)))
            })
            .min_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
        let terminal_count = sessions
            .values()
            .filter(|session| session.terminal_order.load(Ordering::Acquire) != 0)
            .count();
        if terminal_count < MAX_RETAINED_TERMINAL_SESSIONS {
            return;
        }
        let Some((_, session_id, session)) = oldest else {
            return;
        };
        join_session(&session).await;
        sessions.remove(&session_id);
    }
}

async fn poll_snapshot(
    session_id: &str,
    session: &Session,
    after_sequence: u64,
    limit: usize,
) -> Result<PollResult, TiingoError> {
    let data = session.data.lock().await;
    let mut result = PollResult {
        id: session_id.to_owned(),
        state: data.status,
        terminal_error: data.terminal_error,
        events: Vec::new(),
    };
    let mut serialized_len = serialize_poll_result(&result)?.len();
    for event in data
        .events
        .iter()
        .filter(|event| event.sequence > after_sequence)
        .take(limit)
    {
        let event_len = serde_json::to_vec(event)
            .map_err(|_| TiingoError::WebSocketProtocol {
                reason: "poll event could not be encoded",
            })?
            .len();
        let separator_len = usize::from(!result.events.is_empty());
        let candidate_len = serialized_len
            .saturating_add(separator_len)
            .saturating_add(event_len);
        if candidate_len > MAX_WEBSOCKET_POLL_BYTES {
            break;
        }
        serialized_len = candidate_len;
        result.events.push(event.clone());
    }
    Ok(result)
}

fn serialize_poll_result(result: &PollResult) -> Result<Vec<u8>, TiingoError> {
    #[cfg(test)]
    POLL_RESULT_SERIALIZATIONS.fetch_add(1, Ordering::Relaxed);
    serde_json::to_vec(result).map_err(|_| TiingoError::WebSocketProtocol {
        reason: "poll response could not be encoded",
    })
}

async fn touch_session(session: &Session) {
    let mut data = session.data.lock().await;
    if !is_terminal(data.status) {
        data.last_access = Instant::now();
    }
}

fn is_terminal(status: SubscriptionStatus) -> bool {
    matches!(
        status,
        SubscriptionStatus::Stopped
            | SubscriptionStatus::Expired
            | SubscriptionStatus::Failed
            | SubscriptionStatus::DataGap
    )
}

fn normalize_symbols(symbols: Vec<String>) -> Result<Vec<String>, TiingoError> {
    if !(1..=MAX_WEBSOCKET_SYMBOLS).contains(&symbols.len()) {
        return Err(TiingoError::Validation(
            "WebSocket subscriptions require between 1 and 100 equity symbols".into(),
        ));
    }

    let mut seen = HashSet::with_capacity(symbols.len());
    let mut normalized = Vec::with_capacity(symbols.len());
    for symbol in symbols {
        if symbol.is_empty()
            || symbol.len() > 32
            || !symbol
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            || symbol.starts_with('-')
            || symbol.ends_with('-')
        {
            return Err(TiingoError::Validation(
                "equity symbols must use only ASCII letters, digits, or interior hyphens; wildcards and dots are not allowed"
                    .into(),
            ));
        }
        let symbol = symbol.to_ascii_uppercase();
        if !seen.insert(symbol.clone()) {
            return Err(TiingoError::Validation(
                "equity symbols must be unique after case normalization".into(),
            ));
        }
        normalized.push(symbol);
    }
    Ok(normalized)
}

fn normalize_mutation_symbols(symbols: Vec<String>) -> Result<Vec<String>, TiingoError> {
    if symbols.len() > MAX_WEBSOCKET_SYMBOLS {
        return Err(TiingoError::Validation(
            "a WebSocket update may contain at most 100 equity symbols".into(),
        ));
    }
    if symbols.is_empty() {
        return Ok(symbols);
    }
    normalize_symbols(symbols)
}

fn map_connect_error(error: WebSocketError) -> TiingoError {
    match error {
        WebSocketError::Http(response) if response.status().as_u16() == 401 => {
            TiingoError::Authentication {
                capability: CAPABILITY,
            }
        }
        WebSocketError::Http(response) if response.status().as_u16() == 403 => {
            TiingoError::Entitlement {
                capability: CAPABILITY,
            }
        }
        _ => TiingoError::Transport {
            capability: CAPABILITY,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn poll_snapshot_serializes_whole_wrapper_once_for_maximum_page() {
        let events = (1..=MAX_WEBSOCKET_POLL_EVENTS)
            .map(|sequence| MarketDataEvent {
                sequence: sequence as u64,
                received_at: Utc::now(),
                vendor_timestamp: None,
                symbol: None,
                duplicate: false,
                out_of_order: false,
                payload: serde_json::json!({"messageType": "U", "value": sequence}),
            })
            .collect::<VecDeque<_>>();
        let (cancel, _) = watch::channel(false);
        let (commands, _) = mpsc::channel(1);
        let now = Instant::now();
        let session = Session {
            cancel,
            commands,
            mutation: Mutex::new(()),
            join: Mutex::new(None),
            data: Mutex::new(SessionData {
                status: SubscriptionStatus::Active,
                terminal_error: None,
                events,
                queue_bytes: 0,
                next_sequence: MAX_WEBSOCKET_POLL_EVENTS as u64 + 1,
                seen_observations: HashSet::new(),
                latest_timestamp_by_symbol: HashMap::new(),
                symbols: vec!["AAPL".into()],
                threshold_level: 6,
                started_at: now,
                last_access: now,
            }),
            notify: Notify::new(),
            active_sessions: Arc::new(AtomicUsize::new(1)),
            active_slot_held: AtomicBool::new(true),
            terminal_counter: Arc::new(AtomicU64::new(0)),
            terminal_order: AtomicU64::new(0),
        };

        POLL_RESULT_SERIALIZATIONS.store(0, Ordering::Relaxed);
        let page = poll_snapshot(
            "00000000000000000000000000000000",
            &session,
            0,
            MAX_WEBSOCKET_POLL_EVENTS,
        )
        .await
        .expect("maximum poll page is encodable");

        assert_eq!(page.events.len(), MAX_WEBSOCKET_POLL_EVENTS);
        assert_eq!(
            POLL_RESULT_SERIALIZATIONS.load(Ordering::Relaxed),
            1,
            "poll byte admission must not reserialize the growing result"
        );
    }
}
