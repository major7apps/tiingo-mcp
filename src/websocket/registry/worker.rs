use std::{collections::HashSet, future::Future, sync::Arc};

use chrono::{DateTime, Utc};
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio::{
    sync::{mpsc, oneshot, watch},
    time::{Instant, sleep_until, timeout},
};
use tokio_tungstenite::tungstenite::{Error as WebSocketError, Message};

use crate::{
    config::{
        MAX_WEBSOCKET_POLL_BYTES, MAX_WEBSOCKET_QUEUE_BYTES, MAX_WEBSOCKET_QUEUE_EVENTS,
        MAX_WEBSOCKET_SYMBOLS, WEBSOCKET_ABSOLUTE_LIFETIME, WEBSOCKET_ACK_TIMEOUT,
        WEBSOCKET_IDLE_LIFETIME, WEBSOCKET_LIVENESS_TIMEOUT,
    },
    error::TiingoError,
    websocket::protocol::{
        Authorization, MarketData, ProtocolCodec, RawMessageType, ServerMessage, Service,
        SubscriptionAction, SubscriptionId, json_number_matches_u64, redact_bounded_secret,
    },
};

use super::{
    CAPABILITY, ClientSocket, MarketDataEvent, PollResult, ReceiveClock, ReconnectClock, Session,
    SubscriptionStatus, TerminalErrorKind, TiingoConnector, WorkerCommand, set_status,
    set_terminal_failure,
};

#[allow(clippy::too_many_arguments)]
pub(super) async fn run_worker(
    session: Arc<Session>,
    connector: TiingoConnector,
    clock: Arc<dyn ReceiveClock>,
    reconnect_clock: Arc<dyn ReconnectClock>,
    codec: ProtocolCodec,
    service: Service,
    authorization: Authorization,
    mut symbols: Vec<String>,
    mut cancel: watch::Receiver<bool>,
    mut commands: mpsc::Receiver<WorkerCommand>,
    initial: oneshot::Sender<Result<(), TiingoError>>,
) {
    let established = tokio::select! {
        _ = cancel.changed() => Err(TiingoError::Transport { capability: CAPABILITY }),
        result = timeout(
            WEBSOCKET_ACK_TIMEOUT,
            establish_connection(&connector, &codec, service, authorization.clone(), symbols.clone()),
        ) => match result {
            Ok(result) => result,
            Err(_) => Err(TiingoError::Timeout { capability: CAPABILITY }),
        },
    };
    let (mut socket, mut subscription_id) = match established {
        Ok(established) => established,
        Err(error) => {
            set_status(&session, SubscriptionStatus::Failed).await;
            let _ = initial.send(Err(error));
            return;
        }
    };
    let mut acknowledged_subscription_ids = vec![subscription_id.clone()];
    set_status(&session, SubscriptionStatus::Active).await;
    if initial.send(Ok(())).is_err() {
        close_socket(&mut socket, &codec, authorization, subscription_id, symbols).await;
        set_status(&session, SubscriptionStatus::Stopped).await;
        return;
    }
    let mut last_liveness = Instant::now();

    'worker: loop {
        let expiry_deadline = expiry_deadline(&session).await;
        let liveness_deadline = last_liveness + WEBSOCKET_LIVENESS_TIMEOUT;
        let reconnect = tokio::select! {
            _ = sleep_until(liveness_deadline) => {
                bounded_close_socket(&mut socket).await;
                true
            }
            _ = sleep_until(expiry_deadline) => {
                if is_expired(&session).await {
                    close_socket(
                        &mut socket,
                        &codec,
                        authorization,
                        subscription_id,
                        symbols,
                    ).await;
                    set_status(&session, SubscriptionStatus::Expired).await;
                    return;
                }
                false
            }
            _ = cancel.changed() => {
                close_socket(
                    &mut socket,
                    &codec,
                    authorization,
                    subscription_id,
                    symbols,
                ).await;
                set_status(&session, SubscriptionStatus::Stopped).await;
                return;
            }
            command = commands.recv() => {
                match command {
                    Some(WorkerCommand::Update { add_symbols, remove_symbols, response }) => {
                        let update = run_with_session_control(
                            &session,
                            &mut cancel,
                            apply_update(
                                &mut socket,
                                &codec,
                                &session,
                                clock.as_ref(),
                                &mut last_liveness,
                                authorization.clone(),
                                &mut subscription_id,
                                &mut acknowledged_subscription_ids,
                                &mut symbols,
                                add_symbols,
                                remove_symbols,
                            ),
                        )
                        .await;
                        match update {
                            Controlled::Cancelled => {
                                let _ = response.send(Err(TiingoError::Transport {
                                    capability: CAPABILITY,
                                }));
                                close_socket(
                                    &mut socket,
                                    &codec,
                                    authorization,
                                    subscription_id,
                                    symbols,
                                )
                                .await;
                                set_status(&session, SubscriptionStatus::Stopped).await;
                                return;
                            }
                            Controlled::Expired => {
                                let _ = response.send(Err(TiingoError::Timeout {
                                    capability: CAPABILITY,
                                }));
                                close_socket(
                                    &mut socket,
                                    &codec,
                                    authorization,
                                    subscription_id,
                                    symbols,
                                )
                                .await;
                                set_status(&session, SubscriptionStatus::Expired).await;
                                return;
                            }
                            Controlled::Completed(Ok(())) => {
                                session.data.lock().await.symbols = symbols.clone();
                                let _ = response.send(Ok(symbols.clone()));
                                false
                            }
                            Controlled::Completed(Err(error @ TiingoError::Validation(_))) => {
                                let _ = response.send(Err(error));
                                false
                            }
                            Controlled::Completed(Err(error @ TiingoError::Transport { .. })) => {
                                let _ = response.send(Err(error));
                                bounded_close_socket(&mut socket).await;
                                if symbols.is_empty() {
                                    set_terminal_failure(
                                        &session,
                                        TerminalErrorKind::Transport,
                                    )
                                    .await;
                                    return;
                                }
                                true
                            }
                            Controlled::Completed(Err(error)) => {
                                if session.data.lock().await.status != SubscriptionStatus::DataGap {
                                    set_terminal_failure(
                                        &session,
                                        terminal_error_kind(&error),
                                    )
                                    .await;
                                }
                                let _ = response.send(Err(error));
                                bounded_close_socket(&mut socket).await;
                                return;
                            }
                        }
                    }
                    None => {
                        bounded_close_socket(&mut socket).await;
                        set_status(&session, SubscriptionStatus::Stopped).await;
                        return;
                    }
                }
            }
            incoming = socket.next() => {
                match incoming {
                    Some(Ok(Message::Text(payload))) => {
                        match queue_text_message(
                            &session,
                            &codec,
                            &authorization,
                            &acknowledged_subscription_ids,
                            clock.now(),
                            payload.as_bytes(),
                        )
                        .await
                        {
                            Ok(QueueOutcome::Continue { refresh_liveness }) => {
                                if refresh_liveness {
                                    last_liveness = Instant::now();
                                }
                                false
                            }
                            Ok(QueueOutcome::DataGap) => {
                                bounded_close_socket(&mut socket).await;
                                return;
                            }
                            Err(error) => {
                                set_terminal_failure(&session, terminal_error_kind(&error)).await;
                                bounded_close_socket(&mut socket).await;
                                return;
                            }
                        }
                    }
                    Some(Ok(Message::Ping(payload))) => {
                        match run_with_session_control(
                            &session,
                            &mut cancel,
                            timeout(WEBSOCKET_ACK_TIMEOUT, socket.send(Message::Pong(payload))),
                        )
                        .await
                        {
                            Controlled::Completed(Ok(Ok(()))) => false,
                            Controlled::Completed(Ok(Err(_)))
                            | Controlled::Completed(Err(_)) => {
                                bounded_close_socket(&mut socket).await;
                                true
                            }
                            Controlled::Cancelled => {
                                close_socket(
                                    &mut socket,
                                    &codec,
                                    authorization,
                                    subscription_id,
                                    symbols,
                                )
                                .await;
                                set_status(&session, SubscriptionStatus::Stopped).await;
                                return;
                            }
                            Controlled::Expired => {
                                close_socket(
                                    &mut socket,
                                    &codec,
                                    authorization,
                                    subscription_id,
                                    symbols,
                                )
                                .await;
                                set_status(&session, SubscriptionStatus::Expired).await;
                                return;
                            }
                        }
                    }
                    Some(Ok(Message::Binary(_))) => {
                        set_terminal_failure(&session, TerminalErrorKind::Protocol).await;
                        bounded_close_socket(&mut socket).await;
                        return;
                    }
                    Some(Err(WebSocketError::Capacity(_))) => {
                        set_terminal_failure(&session, TerminalErrorKind::Protocol).await;
                        bounded_close_socket(&mut socket).await;
                        return;
                    }
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => true,
                    _ => false,
                }
            }
        };
        if !reconnect {
            continue 'worker;
        }

        match reconnect_connection(
            &session,
            &connector,
            &codec,
            service,
            authorization.clone(),
            symbols.clone(),
            &mut cancel,
            reconnect_clock.as_ref(),
        )
        .await
        {
            ReconnectOutcome::Connected(new_socket, new_subscription_id) => {
                socket = *new_socket;
                remember_subscription_id(&mut acknowledged_subscription_ids, &new_subscription_id);
                subscription_id = new_subscription_id;
                last_liveness = Instant::now();
                set_status(&session, SubscriptionStatus::Active).await;
            }
            ReconnectOutcome::Stopped => {
                set_status(&session, SubscriptionStatus::Stopped).await;
                return;
            }
            ReconnectOutcome::Expired => {
                set_status(&session, SubscriptionStatus::Expired).await;
                return;
            }
            ReconnectOutcome::Failed(terminal_error) => {
                set_terminal_failure(&session, terminal_error).await;
                return;
            }
        }
    }
}

async fn establish_connection(
    connector: &TiingoConnector,
    codec: &ProtocolCodec,
    service: Service,
    authorization: Authorization,
    symbols: Vec<String>,
) -> Result<(ClientSocket, SubscriptionId), TiingoError> {
    let mut socket = connector.connect(service).await?;
    let command = codec.initial_subscribe(authorization, symbols);
    let payload = serde_json::to_string(&command).map_err(|_| TiingoError::WebSocketProtocol {
        reason: "subscribe command could not be encoded",
    })?;
    socket
        .send(Message::text(payload))
        .await
        .map_err(|_| TiingoError::Transport {
            capability: CAPABILITY,
        })?;
    let subscription_id = wait_for_ack(&mut socket, codec).await?;
    Ok((socket, subscription_id))
}

enum ReconnectOutcome {
    Connected(Box<ClientSocket>, SubscriptionId),
    Stopped,
    Expired,
    Failed(TerminalErrorKind),
}

enum Controlled<T> {
    Completed(T),
    Cancelled,
    Expired,
}

async fn run_with_session_control<F>(
    session: &Session,
    cancel: &mut watch::Receiver<bool>,
    operation: F,
) -> Controlled<F::Output>
where
    F: Future,
{
    tokio::pin!(operation);
    loop {
        let expiry_at = expiry_deadline(session).await;
        tokio::select! {
            _ = cancel.changed() => return Controlled::Cancelled,
            _ = sleep_until(expiry_at) => {
                if is_expired(session).await {
                    return Controlled::Expired;
                }
            }
            result = &mut operation => return Controlled::Completed(result),
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn reconnect_connection(
    session: &Session,
    connector: &TiingoConnector,
    codec: &ProtocolCodec,
    service: Service,
    authorization: Authorization,
    symbols: Vec<String>,
    cancel: &mut watch::Receiver<bool>,
    reconnect_clock: &dyn ReconnectClock,
) -> ReconnectOutcome {
    set_status(session, SubscriptionStatus::Reconnecting).await;
    for delay in crate::config::WEBSOCKET_RECONNECT_DELAYS {
        let delay = reconnect_clock.sleep(delay);
        tokio::pin!(delay);
        loop {
            let expiry_at = expiry_deadline(session).await;
            tokio::select! {
                _ = cancel.changed() => return ReconnectOutcome::Stopped,
                _ = sleep_until(expiry_at) => {
                    if is_expired(session).await {
                        return ReconnectOutcome::Expired;
                    }
                }
                _ = &mut delay => break,
            }
        }

        let result = match run_with_session_control(
            session,
            cancel,
            timeout(
                WEBSOCKET_ACK_TIMEOUT,
                establish_connection(
                    connector,
                    codec,
                    service,
                    authorization.clone(),
                    symbols.clone(),
                ),
            ),
        )
        .await
        {
            Controlled::Completed(Ok(result)) => result,
            Controlled::Completed(Err(_)) => Err(TiingoError::Timeout {
                capability: CAPABILITY,
            }),
            Controlled::Cancelled => return ReconnectOutcome::Stopped,
            Controlled::Expired => return ReconnectOutcome::Expired,
        };
        match result {
            Ok((socket, subscription_id)) => {
                return ReconnectOutcome::Connected(Box::new(socket), subscription_id);
            }
            Err(error @ TiingoError::Authentication { .. })
            | Err(error @ TiingoError::Entitlement { .. })
            | Err(error @ TiingoError::WebSocketProtocol { .. }) => {
                return ReconnectOutcome::Failed(terminal_error_kind(&error));
            }
            Err(_) => {}
        }
    }
    ReconnectOutcome::Failed(TerminalErrorKind::Transport)
}

async fn expiry_deadline(session: &Session) -> Instant {
    let data = session.data.lock().await;
    (data.started_at + WEBSOCKET_ABSOLUTE_LIFETIME).min(data.last_access + WEBSOCKET_IDLE_LIFETIME)
}

async fn is_expired(session: &Session) -> bool {
    let data = session.data.lock().await;
    Instant::now() >= data.started_at + WEBSOCKET_ABSOLUTE_LIFETIME
        || Instant::now() >= data.last_access + WEBSOCKET_IDLE_LIFETIME
}

#[allow(clippy::too_many_arguments)]
async fn apply_update(
    socket: &mut ClientSocket,
    codec: &ProtocolCodec,
    session: &Session,
    clock: &dyn ReceiveClock,
    last_liveness: &mut Instant,
    authorization: Authorization,
    subscription_id: &mut SubscriptionId,
    acknowledged_subscription_ids: &mut Vec<SubscriptionId>,
    symbols: &mut Vec<String>,
    add_symbols: Vec<String>,
    remove_symbols: Vec<String>,
) -> Result<(), TiingoError> {
    validate_resulting_symbols(symbols, &add_symbols, &remove_symbols)?;

    if !remove_symbols.is_empty() {
        let mut buffered_messages = BufferedTextMessages::default();
        let acknowledgement = send_update_and_ack(
            socket,
            codec,
            session,
            clock,
            last_liveness,
            &mut buffered_messages,
            SubscriptionAction::Unsubscribe,
            authorization.clone(),
            subscription_id.clone(),
            remove_symbols.clone(),
        )
        .await;
        let acknowledged_subscription_id = match acknowledgement {
            Ok(subscription_id) => subscription_id,
            Err(error) => {
                if !buffered_messages.overflowed {
                    publish_buffered_messages(
                        session,
                        codec,
                        &authorization,
                        acknowledged_subscription_ids,
                        buffered_messages,
                    )
                    .await?;
                }
                return Err(error);
            }
        };
        remember_subscription_id(acknowledged_subscription_ids, &acknowledged_subscription_id);
        *subscription_id = acknowledged_subscription_id;
        symbols.retain(|symbol| !remove_symbols.contains(symbol));
        session.data.lock().await.symbols = symbols.clone();
        publish_buffered_messages(
            session,
            codec,
            &authorization,
            acknowledged_subscription_ids,
            buffered_messages,
        )
        .await?;
    }
    if !add_symbols.is_empty() {
        let mut buffered_messages = BufferedTextMessages::default();
        let acknowledgement = send_update_and_ack(
            socket,
            codec,
            session,
            clock,
            last_liveness,
            &mut buffered_messages,
            SubscriptionAction::Subscribe,
            authorization.clone(),
            subscription_id.clone(),
            add_symbols.clone(),
        )
        .await;
        let acknowledged_subscription_id = match acknowledgement {
            Ok(subscription_id) => subscription_id,
            Err(error) => {
                if !buffered_messages.overflowed {
                    publish_buffered_messages(
                        session,
                        codec,
                        &authorization,
                        acknowledged_subscription_ids,
                        buffered_messages,
                    )
                    .await?;
                }
                return Err(error);
            }
        };
        remember_subscription_id(acknowledged_subscription_ids, &acknowledged_subscription_id);
        *subscription_id = acknowledged_subscription_id;
        symbols.extend(add_symbols);
        session.data.lock().await.symbols = symbols.clone();
        publish_buffered_messages(
            session,
            codec,
            &authorization,
            acknowledged_subscription_ids,
            buffered_messages,
        )
        .await?;
    }
    Ok(())
}

pub(super) fn validate_resulting_symbols(
    symbols: &[String],
    add_symbols: &[String],
    remove_symbols: &[String],
) -> Result<(), TiingoError> {
    let mut resulting = symbols.iter().cloned().collect::<HashSet<_>>();
    for symbol in remove_symbols {
        if !resulting.remove(symbol) {
            return Err(TiingoError::Validation(format!(
                "cannot remove {symbol} because it is not subscribed"
            )));
        }
    }
    for symbol in add_symbols {
        if !resulting.insert(symbol.clone()) {
            return Err(TiingoError::Validation(format!(
                "cannot add {symbol} because it is already subscribed"
            )));
        }
    }
    if !(1..=MAX_WEBSOCKET_SYMBOLS).contains(&resulting.len()) {
        return Err(TiingoError::Validation(
            "an updated WebSocket subscription must contain between 1 and 100 equity symbols"
                .into(),
        ));
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn send_update_and_ack(
    socket: &mut ClientSocket,
    codec: &ProtocolCodec,
    session: &Session,
    clock: &dyn ReceiveClock,
    last_liveness: &mut Instant,
    buffered_messages: &mut BufferedTextMessages,
    action: SubscriptionAction,
    authorization: Authorization,
    subscription_id: SubscriptionId,
    symbols: Vec<String>,
) -> Result<SubscriptionId, TiingoError> {
    let command =
        codec.subscription_update(action, authorization.clone(), subscription_id, symbols);
    let payload = serde_json::to_string(&command).map_err(|_| TiingoError::WebSocketProtocol {
        reason: "subscription update could not be encoded",
    })?;
    timeout(WEBSOCKET_ACK_TIMEOUT, async {
        socket
            .send(Message::text(payload))
            .await
            .map_err(|_| TiingoError::Transport {
                capability: CAPABILITY,
            })?;
        wait_for_update_ack(
            socket,
            codec,
            session,
            buffered_messages,
            clock,
            last_liveness,
        )
        .await
    })
    .await
    .map_err(|_| TiingoError::Timeout {
        capability: CAPABILITY,
    })?
}

enum QueueOutcome {
    Continue { refresh_liveness: bool },
    DataGap,
}

struct BufferedTextMessage {
    received_at: DateTime<Utc>,
    payload: Vec<u8>,
}

#[derive(Default)]
struct BufferedTextMessages {
    messages: Vec<BufferedTextMessage>,
    bytes: usize,
    overflowed: bool,
}

async fn buffer_text_message(
    session: &Session,
    buffered_messages: &mut BufferedTextMessages,
    received_at: DateTime<Utc>,
    payload: &[u8],
) -> Result<(), TiingoError> {
    let message_bytes = payload
        .len()
        .saturating_add(std::mem::size_of::<BufferedTextMessage>());
    let exceeds_bounds = {
        let data = session.data.lock().await;
        data.events
            .len()
            .saturating_add(buffered_messages.messages.len())
            >= MAX_WEBSOCKET_QUEUE_EVENTS
            || data
                .queue_bytes
                .saturating_add(buffered_messages.bytes)
                .saturating_add(message_bytes)
                > MAX_WEBSOCKET_QUEUE_BYTES
    };
    if exceeds_bounds {
        buffered_messages.overflowed = true;
        set_status(session, SubscriptionStatus::DataGap).await;
        return Err(TiingoError::WebSocketProtocol {
            reason: "market-data queue overflowed while awaiting an update acknowledgement",
        });
    }
    buffered_messages.bytes = buffered_messages.bytes.saturating_add(message_bytes);
    buffered_messages.messages.push(BufferedTextMessage {
        received_at,
        payload: payload.to_vec(),
    });
    Ok(())
}

async fn publish_buffered_messages(
    session: &Session,
    codec: &ProtocolCodec,
    authorization: &Authorization,
    acknowledged_subscription_ids: &[SubscriptionId],
    buffered_messages: BufferedTextMessages,
) -> Result<(), TiingoError> {
    for message in buffered_messages.messages {
        if matches!(
            queue_text_message(
                session,
                codec,
                authorization,
                acknowledged_subscription_ids,
                message.received_at,
                &message.payload,
            )
            .await?,
            QueueOutcome::DataGap
        ) {
            return Err(TiingoError::WebSocketProtocol {
                reason: "market-data queue overflowed while publishing an acknowledged update",
            });
        }
    }
    Ok(())
}

async fn queue_text_message(
    session: &Session,
    codec: &ProtocolCodec,
    authorization: &Authorization,
    acknowledged_subscription_ids: &[SubscriptionId],
    received_at: DateTime<Utc>,
    payload: &[u8],
) -> Result<QueueOutcome, TiingoError> {
    let received = codec.decode(payload, received_at)?;
    match &received.message {
        ServerMessage::Information(information) if information.response.code == 401 => {
            return Err(TiingoError::Authentication {
                capability: CAPABILITY,
            });
        }
        ServerMessage::Information(information) if information.response.code == 403 => {
            return Err(TiingoError::Entitlement {
                capability: CAPABILITY,
            });
        }
        ServerMessage::Information(information)
            if !(200..=299).contains(&information.response.code) =>
        {
            return Err(TiingoError::WebSocketProtocol {
                reason: "information message was rejected",
            });
        }
        ServerMessage::Information(_) => {
            return Ok(QueueOutcome::Continue {
                refresh_liveness: false,
            });
        }
        ServerMessage::Heartbeat(_) => {
            return Ok(QueueOutcome::Continue {
                refresh_liveness: true,
            });
        }
        ServerMessage::Raw(raw) if raw.message_type == RawMessageType::Error => {
            return Err(classify_websocket_rejection(&raw.payload).unwrap_or(
                TiingoError::WebSocketProtocol {
                    reason: "server error message was not recognized",
                },
            ));
        }
        _ => {}
    }
    let refresh_liveness = match &received.message {
        ServerMessage::Market(_) => true,
        ServerMessage::Raw(raw) => {
            matches!(
                raw.message_type,
                RawMessageType::Update | RawMessageType::Delete
            )
        }
        ServerMessage::Information(_) | ServerMessage::Heartbeat(_) => false,
    };
    let mut value: Value =
        serde_json::from_slice(payload).map_err(|_| TiingoError::WebSocketProtocol {
            reason: "message is not one complete JSON object",
        })?;
    let duplicate_payload_fingerprint =
        serde_json::to_string(&value).map_err(|_| TiingoError::WebSocketProtocol {
            reason: "market event could not be encoded",
        })?;
    redact_sensitive_value(
        &mut value,
        authorization,
        acknowledged_subscription_ids,
        matches!(&received.message, ServerMessage::Market(_)),
    );
    let (vendor_timestamp, symbol) = message_identity(&received.message);

    let mut data = session.data.lock().await;
    let duplicate = vendor_timestamp.as_ref().is_some_and(|timestamp| {
        data.seen_observations
            .observe(format!("{timestamp}\0{duplicate_payload_fingerprint}"))
    });
    let out_of_order = match (&symbol, &vendor_timestamp) {
        (Some(symbol), Some(timestamp)) => DateTime::parse_from_rfc3339(timestamp)
            .ok()
            .map(|timestamp| timestamp.with_timezone(&Utc))
            .is_some_and(|timestamp| {
                let out_of_order = data
                    .latest_timestamp_by_symbol
                    .get(symbol)
                    .is_some_and(|latest| timestamp < *latest);
                if !out_of_order {
                    data.latest_timestamp_by_symbol
                        .entry(symbol.clone())
                        .and_modify(|latest| *latest = (*latest).max(timestamp))
                        .or_insert(timestamp);
                }
                out_of_order
            }),
        _ => false,
    };
    let event = MarketDataEvent {
        sequence: data.next_sequence,
        received_at,
        vendor_timestamp: vendor_timestamp.map(|timestamp| {
            redact_sensitive_text(&timestamp, authorization, acknowledged_subscription_ids)
        }),
        symbol: symbol.map(|symbol| {
            redact_sensitive_text(&symbol, authorization, acknowledged_subscription_ids)
        }),
        duplicate,
        out_of_order,
        payload: value,
    };
    let event_bytes = serde_json::to_vec(&event)
        .map_err(|_| TiingoError::WebSocketProtocol {
            reason: "market event could not be encoded",
        })?
        .len();
    let single_event_poll_bytes = serde_json::to_vec(&PollResult {
        id: "0".repeat(32),
        state: SubscriptionStatus::Failed,
        terminal_error: Some(TerminalErrorKind::Authentication),
        events: vec![event.clone()],
    })
    .map_err(|_| TiingoError::WebSocketProtocol {
        reason: "poll response could not be encoded",
    })?
    .len();
    if single_event_poll_bytes > MAX_WEBSOCKET_POLL_BYTES
        || data.events.len() >= MAX_WEBSOCKET_QUEUE_EVENTS
        || data.queue_bytes.saturating_add(event_bytes) > MAX_WEBSOCKET_QUEUE_BYTES
    {
        drop(data);
        set_status(session, SubscriptionStatus::DataGap).await;
        return Ok(QueueOutcome::DataGap);
    }
    data.next_sequence += 1;
    data.queue_bytes += event_bytes;
    data.events.push_back(event);
    drop(data);
    session.notify.notify_waiters();
    Ok(QueueOutcome::Continue { refresh_liveness })
}

fn redact_sensitive_value(
    value: &mut Value,
    authorization: &Authorization,
    acknowledged_subscription_ids: &[SubscriptionId],
    preserve_root_market_data_numbers: bool,
) {
    match value {
        Value::String(text) => {
            *text = redact_sensitive_text(text, authorization, acknowledged_subscription_ids);
        }
        Value::Array(values) => {
            for value in values {
                redact_sensitive_value(value, authorization, acknowledged_subscription_ids, false);
            }
        }
        Value::Object(values) => {
            let original = std::mem::take(values);
            for (key, mut value) in original {
                if key.eq_ignore_ascii_case("subscriptionId") {
                    value = Value::String("[REDACTED]".into());
                } else if preserve_root_market_data_numbers && key == "data" {
                    redact_market_data_value(
                        &mut value,
                        authorization,
                        acknowledged_subscription_ids,
                    );
                } else {
                    redact_sensitive_value(
                        &mut value,
                        authorization,
                        acknowledged_subscription_ids,
                        false,
                    );
                }
                values.insert(
                    redact_sensitive_text(&key, authorization, acknowledged_subscription_ids),
                    value,
                );
            }
        }
        Value::Number(number) => {
            if authorization.matches_number(number)
                || acknowledged_subscription_ids.iter().any(|subscription_id| {
                    matches!(
                        subscription_id,
                        SubscriptionId::Number(value) if json_number_matches_u64(number, *value)
                    )
                })
            {
                *value = Value::String("[REDACTED]".into());
            }
        }
        Value::Null | Value::Bool(_) => {}
    }
}

fn redact_market_data_value(
    value: &mut Value,
    authorization: &Authorization,
    acknowledged_subscription_ids: &[SubscriptionId],
) {
    match value {
        Value::String(text) => {
            *text = redact_sensitive_text(text, authorization, acknowledged_subscription_ids);
        }
        Value::Array(values) => {
            for value in values {
                redact_market_data_value(value, authorization, acknowledged_subscription_ids);
            }
        }
        Value::Object(_) => {
            redact_sensitive_value(value, authorization, acknowledged_subscription_ids, false)
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

fn redact_sensitive_text(
    value: &str,
    authorization: &Authorization,
    acknowledged_subscription_ids: &[SubscriptionId],
) -> String {
    let mut redacted = authorization.redact(value);
    for subscription_id in acknowledged_subscription_ids {
        let secret = match subscription_id {
            SubscriptionId::Number(value) => value.to_string(),
            SubscriptionId::String(value) => value.clone(),
        };
        redacted = redact_bounded_secret(&redacted, &secret);
    }
    redacted
}

fn terminal_error_kind(error: &TiingoError) -> TerminalErrorKind {
    match error {
        TiingoError::Authentication { .. } => TerminalErrorKind::Authentication,
        TiingoError::Entitlement { .. } => TerminalErrorKind::Entitlement,
        TiingoError::Transport { .. } | TiingoError::Timeout { .. } => TerminalErrorKind::Transport,
        _ => TerminalErrorKind::Protocol,
    }
}

fn map_socket_receive_error(error: WebSocketError) -> TiingoError {
    match error {
        WebSocketError::Capacity(_) => TiingoError::WebSocketProtocol {
            reason: "WebSocket message exceeded the configured size limit",
        },
        _ => TiingoError::Transport {
            capability: CAPABILITY,
        },
    }
}

fn remember_subscription_id(
    acknowledged_subscription_ids: &mut Vec<SubscriptionId>,
    subscription_id: &SubscriptionId,
) {
    if !acknowledged_subscription_ids.contains(subscription_id) {
        acknowledged_subscription_ids.push(subscription_id.clone());
    }
}

fn message_identity(message: &ServerMessage) -> (Option<String>, Option<String>) {
    match message {
        ServerMessage::Market(MarketData::IexReference(update))
        | ServerMessage::Market(MarketData::ConsolidatedReference(update)) => (
            Some(update.vendor_timestamp.clone()),
            Some(update.ticker.clone()),
        ),
        ServerMessage::Market(MarketData::IexTops(update)) => (
            Some(update.vendor_timestamp.clone()),
            Some(update.ticker.clone()),
        ),
        ServerMessage::Market(MarketData::ConsolidatedLiquidity(update)) => (
            Some(update.vendor_timestamp.clone()),
            Some(update.ticker.clone()),
        ),
        _ => (None, None),
    }
}

async fn wait_for_update_ack(
    socket: &mut ClientSocket,
    codec: &ProtocolCodec,
    session: &Session,
    buffered_messages: &mut BufferedTextMessages,
    clock: &dyn ReceiveClock,
    last_liveness: &mut Instant,
) -> Result<SubscriptionId, TiingoError> {
    loop {
        let message = socket
            .next()
            .await
            .ok_or(TiingoError::Transport {
                capability: CAPABILITY,
            })?
            .map_err(map_socket_receive_error)?;
        match message {
            Message::Text(payload) => {
                let received_at = clock.now();
                let received = codec.decode(payload.as_bytes(), received_at)?;
                let refresh_liveness = match &received.message {
                    ServerMessage::Market(_) => true,
                    ServerMessage::Raw(raw) => matches!(
                        raw.message_type,
                        RawMessageType::Update | RawMessageType::Delete
                    ),
                    ServerMessage::Information(_) | ServerMessage::Heartbeat(_) => false,
                };
                match received.message {
                    ServerMessage::Information(information) => {
                        return acknowledgement_result(information);
                    }
                    ServerMessage::Heartbeat(_) => {
                        *last_liveness = Instant::now();
                        continue;
                    }
                    ServerMessage::Raw(raw) if raw.message_type == RawMessageType::Error => {
                        if let Some(error) = classify_websocket_rejection(&raw.payload) {
                            return Err(error);
                        }
                        return Err(TiingoError::WebSocketProtocol {
                            reason: "server error message was not recognized",
                        });
                    }
                    _ => {}
                }
                if refresh_liveness {
                    *last_liveness = Instant::now();
                }
                buffer_text_message(session, buffered_messages, received_at, payload.as_bytes())
                    .await?;
            }
            Message::Ping(payload) => {
                socket
                    .send(Message::Pong(payload))
                    .await
                    .map_err(|_| TiingoError::Transport {
                        capability: CAPABILITY,
                    })?;
            }
            Message::Close(_) => {
                return Err(TiingoError::Transport {
                    capability: CAPABILITY,
                });
            }
            _ => {
                return Err(TiingoError::WebSocketProtocol {
                    reason: "subscription acknowledgement was not a text message",
                });
            }
        }
    }
}

fn acknowledgement_result(
    information: crate::websocket::protocol::Information,
) -> Result<SubscriptionId, TiingoError> {
    match information.response.code {
        200..=299 => Ok(information.subscription_id),
        401 => Err(TiingoError::Authentication {
            capability: CAPABILITY,
        }),
        403 => Err(TiingoError::Entitlement {
            capability: CAPABILITY,
        }),
        _ => Err(TiingoError::WebSocketProtocol {
            reason: "subscription acknowledgement was rejected",
        }),
    }
}

async fn wait_for_ack(
    socket: &mut ClientSocket,
    codec: &ProtocolCodec,
) -> Result<SubscriptionId, TiingoError> {
    loop {
        let message = socket
            .next()
            .await
            .ok_or(TiingoError::Transport {
                capability: CAPABILITY,
            })?
            .map_err(map_socket_receive_error)?;
        match message {
            Message::Text(payload) => {
                let received = codec.decode(payload.as_bytes(), chrono::Utc::now())?;
                match received.message {
                    ServerMessage::Information(information) => {
                        return acknowledgement_result(information);
                    }
                    ServerMessage::Heartbeat(_) => continue,
                    ServerMessage::Raw(raw) if raw.message_type == RawMessageType::Error => {
                        if let Some(error) = classify_websocket_rejection(&raw.payload) {
                            return Err(error);
                        }
                    }
                    _ => {}
                }
                return Err(TiingoError::WebSocketProtocol {
                    reason: "subscription acknowledgement was not an information message",
                });
            }
            Message::Ping(payload) => {
                socket
                    .send(Message::Pong(payload))
                    .await
                    .map_err(|_| TiingoError::Transport {
                        capability: CAPABILITY,
                    })?;
            }
            Message::Close(_) => {
                return Err(TiingoError::Transport {
                    capability: CAPABILITY,
                });
            }
            _ => {
                return Err(TiingoError::WebSocketProtocol {
                    reason: "subscription acknowledgement was not a text message",
                });
            }
        }
    }
}

fn classify_websocket_rejection(payload: &Value) -> Option<TiingoError> {
    let code = find_response_code(payload);
    if code == Some(401) {
        return Some(TiingoError::Authentication {
            capability: CAPABILITY,
        });
    }
    if code == Some(403) {
        return Some(TiingoError::Entitlement {
            capability: CAPABILITY,
        });
    }
    let mut text = String::new();
    collect_response_text(payload, &mut text);
    let text = text.to_ascii_lowercase();
    if text.contains("entitlement")
        || text.contains("not entitled")
        || text.contains("permission denied")
    {
        return Some(TiingoError::Entitlement {
            capability: CAPABILITY,
        });
    }
    if text.contains("authentication")
        || text.contains("authorization")
        || text.contains("credential")
        || text.contains("api key")
        || text.contains("invalid token")
    {
        return Some(TiingoError::Authentication {
            capability: CAPABILITY,
        });
    }
    None
}

fn find_response_code(value: &Value) -> Option<u16> {
    match value {
        Value::Object(object) => {
            for key in ["code", "status", "statusCode"] {
                if let Some(code) = object
                    .get(key)
                    .and_then(Value::as_u64)
                    .and_then(|code| u16::try_from(code).ok())
                {
                    return Some(code);
                }
            }
            object.values().find_map(find_response_code)
        }
        Value::Array(values) => values.iter().find_map(find_response_code),
        _ => None,
    }
}

fn collect_response_text(value: &Value, output: &mut String) {
    match value {
        Value::String(value) => {
            output.push(' ');
            output.push_str(value);
        }
        Value::Array(values) => {
            for value in values {
                collect_response_text(value, output);
            }
        }
        Value::Object(object) => {
            for value in object.values() {
                collect_response_text(value, output);
            }
        }
        _ => {}
    }
}

async fn bounded_close_socket(socket: &mut ClientSocket) {
    let _ = timeout(WEBSOCKET_ACK_TIMEOUT, socket.close(None)).await;
}

async fn close_socket(
    socket: &mut ClientSocket,
    codec: &ProtocolCodec,
    authorization: Authorization,
    subscription_id: SubscriptionId,
    symbols: Vec<String>,
) {
    let cleanup = async {
        let command = codec.subscription_update(
            SubscriptionAction::Unsubscribe,
            authorization,
            subscription_id,
            symbols,
        );
        if let Ok(payload) = serde_json::to_string(&command) {
            let _ = socket.send(Message::text(payload)).await;
        }
        let _ = socket.close(None).await;
    };
    let _ = timeout(WEBSOCKET_ACK_TIMEOUT, cleanup).await;
}
