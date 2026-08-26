use std::{
    future::Future,
    pin::Pin,
    sync::{Arc, Condvar, Mutex as StdMutex},
    task::Poll,
    time::Duration,
};

use chrono::{DateTime, Utc};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tiingo_mcp::config::MAX_WEBSOCKET_MESSAGE_BYTES;
use tiingo_mcp::error::TiingoError;
use tiingo_mcp::websocket::{
    protocol::Service,
    registry::{
        MarketDataEvent, MarketDataRegistry, PollResult, ReceiveClock, ReconnectClock,
        StartRequest, SubscriptionStatus, TerminalErrorKind, TiingoConnector, UpdateRequest,
    },
};
use tokio::net::TcpListener;
use tokio_tungstenite::{
    accept_async,
    tungstenite::{
        Message,
        protocol::frame::{
            Frame,
            coding::{Data, OpCode},
        },
    },
};

#[derive(Debug)]
struct FixedClock(DateTime<Utc>);

impl ReceiveClock for FixedClock {
    fn now(&self) -> DateTime<Utc> {
        self.0
    }
}

#[derive(Debug)]
struct BlockingClock {
    now: DateTime<Utc>,
    entered: StdMutex<Option<tokio::sync::oneshot::Sender<()>>>,
    release: Arc<(StdMutex<bool>, Condvar)>,
}

impl ReceiveClock for BlockingClock {
    fn now(&self) -> DateTime<Utc> {
        if let Some(entered) = self.entered.lock().expect("clock lock is healthy").take() {
            let _ = entered.send(());
        }
        let (released, wake) = &*self.release;
        let mut released = released.lock().expect("release lock is healthy");
        while !*released {
            released = wake.wait(released).expect("release wait is healthy");
        }
        self.now
    }
}

#[derive(Debug)]
struct ManualReconnectClock {
    requests: tokio::sync::mpsc::UnboundedSender<(Duration, tokio::sync::oneshot::Sender<()>)>,
}

impl ReconnectClock for ManualReconnectClock {
    fn sleep(&self, delay: Duration) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        let requests = self.requests.clone();
        Box::pin(async move {
            let (release, released) = tokio::sync::oneshot::channel();
            requests
                .send((delay, release))
                .expect("record reconnect delay");
            let _ = released.await;
        })
    }
}

async fn poll_single_raw_event(subscription_id: Value, event: Value) -> MarketDataEvent {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock WebSocket server");
    let endpoint = format!("ws://{}", listener.local_addr().expect("listener address"));
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept client");
        let mut socket = accept_async(stream).await.expect("WebSocket handshake");
        socket
            .next()
            .await
            .expect("subscribe command")
            .expect("valid subscribe command");
        socket
            .send(Message::text(
                json!({
                    "messageType": "I",
                    "data": {"subscriptionId": subscription_id},
                    "response": {"code": 200, "message": "subscribed"}
                })
                .to_string(),
            ))
            .await
            .expect("send acknowledgement");
        socket
            .send(Message::text(event.to_string()))
            .await
            .expect("send raw market event");
        while let Some(message) = socket.next().await {
            if matches!(message, Ok(Message::Close(_))) {
                break;
            }
        }
    });
    let connector = TiingoConnector::with_endpoints(endpoint.clone(), endpoint)
        .expect("mock endpoint is valid");
    let registry = MarketDataRegistry::with_connector(Some("test-key".into()), connector);
    let started = registry
        .start(StartRequest {
            service: Service::Iex,
            symbols: vec!["AAPL".into()],
            threshold_level: None,
            confirm_iex_market_data_agreement: false,
        })
        .await
        .expect("subscription starts");
    let mut poll = registry
        .poll(&started.id, 0)
        .await
        .expect("raw market event is retained");
    assert_eq!(poll.events.len(), 1);
    let event = poll.events.remove(0);
    registry.shutdown().await;
    server.await.expect("mock server exits cleanly");
    event
}

#[tokio::test]
async fn iex_direct_feed_threshold_requires_market_data_agreement_confirmation() {
    let connector = TiingoConnector::with_endpoints("ws://127.0.0.1:1", "ws://127.0.0.1:1")
        .expect("loopback endpoints are valid");
    let registry = MarketDataRegistry::with_connector(Some("test-key".into()), connector);

    let error = registry
        .start(StartRequest {
            service: Service::Iex,
            symbols: vec!["AAPL".into()],
            threshold_level: Some(0),
            confirm_iex_market_data_agreement: false,
        })
        .await
        .expect_err("direct IEX feed access must require confirmation");

    assert_eq!(error.payload().kind, "validation");
    assert!(error.to_string().contains("market-data agreement"));
}

#[tokio::test]
async fn start_rejects_unbounded_duplicate_and_unsafe_equity_symbols() {
    let connector = TiingoConnector::with_endpoints("ws://127.0.0.1:1", "ws://127.0.0.1:1")
        .expect("loopback endpoints are valid");
    let registry = MarketDataRegistry::with_connector(Some("test-key".into()), connector);
    let invalid_symbol_sets = [
        Vec::new(),
        (0..101).map(|index| format!("S{index}")).collect(),
        vec!["*".into()],
        vec!["BRK.A".into()],
        vec!["../AAPL".into()],
        vec!["aapl".into(), "AAPL".into()],
    ];

    for symbols in invalid_symbol_sets {
        let error = registry
            .start(StartRequest {
                service: Service::Iex,
                symbols,
                threshold_level: None,
                confirm_iex_market_data_agreement: false,
            })
            .await
            .expect_err("invalid symbol sets must fail before socket connection");
        assert_eq!(error.payload().kind, "validation");
    }
}

#[tokio::test]
async fn start_rejects_a_blank_api_key_as_sanitized_configuration() {
    let connector = TiingoConnector::with_endpoints("ws://127.0.0.1:1", "ws://127.0.0.1:1")
        .expect("loopback endpoints are valid");
    let registry = MarketDataRegistry::with_connector(Some("   ".into()), connector);

    let error = registry
        .start(StartRequest {
            service: Service::Iex,
            symbols: vec!["AAPL".into()],
            threshold_level: None,
            confirm_iex_market_data_agreement: false,
        })
        .await
        .expect_err("blank key fails before socket connection");

    assert_eq!(error.payload().kind, "configuration");
}

#[tokio::test]
async fn consolidated_start_rejects_thresholds_other_than_four_or_six() {
    let connector = TiingoConnector::with_endpoints("ws://127.0.0.1:1", "ws://127.0.0.1:1")
        .expect("loopback endpoints are valid");
    let registry = MarketDataRegistry::with_connector(Some("test-key".into()), connector);

    let error = registry
        .start(StartRequest {
            service: Service::Consolidated,
            symbols: vec!["AAPL".into()],
            threshold_level: Some(5),
            confirm_iex_market_data_agreement: false,
        })
        .await
        .expect_err("consolidated level 5 must be rejected");

    assert_eq!(error.payload().kind, "validation");
    assert!(error.to_string().contains("threshold level 5"));
}

#[tokio::test]
async fn confirmed_iex_direct_and_consolidated_thresholds_start_with_exact_frames() {
    for (service, threshold_level, confirm_agreement, expected_threshold) in [
        (Service::Iex, Some(5), true, 5),
        (Service::Consolidated, None, false, 6),
        (Service::Consolidated, Some(4), false, 4),
    ] {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock WebSocket server");
        let endpoint = format!("ws://{}", listener.local_addr().expect("listener address"));
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept client");
            let mut socket = accept_async(stream).await.expect("WebSocket handshake");
            let command = socket
                .next()
                .await
                .expect("subscribe command")
                .expect("valid subscribe command")
                .into_text()
                .expect("text command");
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&command).expect("JSON command"),
                json!({
                    "eventName": "subscribe",
                    "authorization": "test-key",
                    "eventData": {
                        "thresholdLevel": expected_threshold,
                        "tickers": ["AAPL"]
                    }
                })
            );
            socket
                .send(Message::text(
                    json!({
                        "messageType": "I",
                        "data": {"subscriptionId": 60},
                        "response": {"code": 200, "message": "subscribed"}
                    })
                    .to_string(),
                ))
                .await
                .expect("send acknowledgement");
            while let Some(message) = socket.next().await {
                if matches!(message, Ok(Message::Close(_))) {
                    break;
                }
            }
        });
        let connector = TiingoConnector::with_endpoints(endpoint.clone(), endpoint)
            .expect("mock endpoint is valid");
        let registry = MarketDataRegistry::with_connector(Some("test-key".into()), connector);

        let started = registry
            .start(StartRequest {
                service,
                symbols: vec!["AAPL".into()],
                threshold_level,
                confirm_iex_market_data_agreement: confirm_agreement,
            })
            .await
            .expect("approved threshold starts");
        assert_eq!(started.state, SubscriptionStatus::Active);
        registry.shutdown().await;
        server.await.expect("mock server exits cleanly");
    }
}

#[tokio::test]
async fn start_normalizes_symbols_uses_default_threshold_and_waits_for_acknowledgement() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock WebSocket server");
    let endpoint = format!("ws://{}", listener.local_addr().expect("listener address"));
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept client");
        let mut socket = accept_async(stream).await.expect("WebSocket handshake");
        let command = socket
            .next()
            .await
            .expect("subscribe command")
            .expect("valid subscribe command")
            .into_text()
            .expect("text command");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&command).expect("JSON command"),
            json!({
                "eventName": "subscribe",
                "authorization": "test-key",
                "eventData": {"thresholdLevel": 6, "tickers": ["AAPL"]}
            })
        );
        socket
            .send(Message::text(
                json!({
                    "messageType": "H",
                    "response": {"code": 200, "message": "heartbeat"}
                })
                .to_string(),
            ))
            .await
            .expect("send pre-ack heartbeat");
        socket
            .send(Message::text(
                json!({
                    "messageType": "I",
                    "data": {"subscriptionId": "upstream-secret"},
                    "response": {"code": 200, "message": "subscribed"}
                })
                .to_string(),
            ))
            .await
            .expect("send acknowledgement");
        while let Some(message) = socket.next().await {
            if matches!(message, Ok(Message::Close(_))) {
                break;
            }
        }
    });
    let connector = TiingoConnector::with_endpoints(endpoint.clone(), endpoint)
        .expect("mock endpoint is valid");
    let registry = MarketDataRegistry::with_connector(Some("test-key".into()), connector);

    let started = registry
        .start(StartRequest {
            service: Service::Iex,
            symbols: vec!["aapl".into()],
            threshold_level: None,
            confirm_iex_market_data_agreement: false,
        })
        .await
        .expect("acknowledged subscription starts");

    assert_eq!(started.state, SubscriptionStatus::Active);
    assert_eq!(started.id.len(), 32);
    assert!(
        started
            .id
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    );
    let debug = format!("{started:?}");
    assert!(!debug.contains("test-key"));
    assert!(!debug.contains("upstream-secret"));

    registry.shutdown().await;
    server.await.expect("mock server exits cleanly");
}

#[tokio::test]
async fn poll_reassembles_fragmented_text_and_replays_consecutive_messages_in_arrival_order() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock WebSocket server");
    let endpoint = format!("ws://{}", listener.local_addr().expect("listener address"));
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept client");
        let mut socket = accept_async(stream).await.expect("WebSocket handshake");
        socket
            .next()
            .await
            .expect("subscribe command")
            .expect("valid subscribe command");
        socket
            .send(Message::text(
                json!({
                    "messageType": "I",
                    "data": {"subscriptionId": 41},
                    "response": {"code": 200, "message": "subscribed"}
                })
                .to_string(),
            ))
            .await
            .expect("send acknowledgement");

        let fragmented = json!({
            "messageType": "A",
            "service": "iex",
            "data": ["2026-08-25T14:00:00Z", "AAPL", 100.0]
        })
        .to_string();
        let split = fragmented.len() / 2;
        socket
            .send(Message::Frame(Frame::message(
                fragmented.as_bytes()[..split].to_vec(),
                OpCode::Data(Data::Text),
                false,
            )))
            .await
            .expect("send initial text fragment");
        socket
            .send(Message::Frame(Frame::message(
                fragmented.as_bytes()[split..].to_vec(),
                OpCode::Data(Data::Continue),
                true,
            )))
            .await
            .expect("send final text fragment");
        for (timestamp, price) in [
            ("2026-08-25T14:00:01Z", 101.0),
            ("2026-08-25T14:00:02Z", 102.0),
        ] {
            socket
                .send(Message::text(
                    json!({
                        "messageType": "A",
                        "service": "iex",
                        "data": [timestamp, "AAPL", price]
                    })
                    .to_string(),
                ))
                .await
                .expect("send complete market message");
        }
        while let Some(message) = socket.next().await {
            if matches!(message, Ok(Message::Close(_))) {
                break;
            }
        }
    });
    let received_at = DateTime::parse_from_rfc3339("2026-08-25T14:00:03Z")
        .expect("valid fixture time")
        .with_timezone(&Utc);
    let connector = TiingoConnector::with_endpoints(endpoint.clone(), endpoint)
        .expect("mock endpoint is valid");
    let registry = MarketDataRegistry::with_connector_and_clock(
        Some("test-key".into()),
        connector,
        Arc::new(FixedClock(received_at)),
    );
    let started = registry
        .start(StartRequest {
            service: Service::Iex,
            symbols: vec!["AAPL".into()],
            threshold_level: None,
            confirm_iex_market_data_agreement: false,
        })
        .await
        .expect("subscription starts");

    let tail = registry
        .poll(&started.id, 2)
        .await
        .expect("tail poll succeeds");
    assert_eq!(tail.events[0].sequence, 3);
    let first = registry.poll(&started.id, 0).await.expect("poll succeeds");
    assert_eq!(first.state, SubscriptionStatus::Active);
    assert_eq!(first.events.len(), 3);
    assert_eq!(
        first
            .events
            .iter()
            .map(|event| event.sequence)
            .collect::<Vec<_>>(),
        vec![1, 2, 3]
    );
    assert_eq!(
        first
            .events
            .iter()
            .map(|event| event.payload["data"][2].as_f64().expect("price"))
            .collect::<Vec<_>>(),
        vec![100.0, 101.0, 102.0]
    );
    assert!(
        first
            .events
            .iter()
            .all(|event| event.received_at == received_at)
    );

    let replay = registry
        .poll(&started.id, 0)
        .await
        .expect("replay succeeds");
    assert_eq!(replay, first);

    registry.shutdown().await;
    server.await.expect("mock server exits cleanly");
}

async fn assert_oversized_wire_frames_are_terminal(frames: Vec<Frame>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock WebSocket server");
    let endpoint = format!("ws://{}", listener.local_addr().expect("listener address"));
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept client");
        let mut socket = accept_async(stream).await.expect("WebSocket handshake");
        socket
            .next()
            .await
            .expect("subscribe command")
            .expect("valid subscribe command");
        socket
            .send(Message::text(
                json!({
                    "messageType": "I",
                    "data": {"subscriptionId": "bounded-wire-id"},
                    "response": {"code": 200, "message": "subscribed"}
                })
                .to_string(),
            ))
            .await
            .expect("send acknowledgement");
        for frame in frames {
            if socket.send(Message::Frame(frame)).await.is_err() {
                break;
            }
        }
        drop(socket);
        tokio::time::timeout(Duration::from_millis(750), listener.accept())
            .await
            .is_ok()
    });
    let connector = TiingoConnector::with_endpoints(endpoint.clone(), endpoint)
        .expect("mock endpoint is valid");
    let registry = MarketDataRegistry::with_connector(Some("test-key".into()), connector);
    let started = registry
        .start(StartRequest {
            service: Service::Iex,
            symbols: vec!["AAPL".into()],
            threshold_level: None,
            confirm_iex_market_data_agreement: false,
        })
        .await
        .expect("subscription starts");

    let terminal = tokio::time::timeout(Duration::from_secs(1), registry.poll(&started.id, 0))
        .await
        .expect("oversized wire payload becomes terminal promptly")
        .expect("terminal state remains inspectable");
    assert_eq!(terminal.state, SubscriptionStatus::Failed);
    assert_eq!(terminal.terminal_error, Some(TerminalErrorKind::Protocol));
    assert!(terminal.events.is_empty());
    assert!(
        !server.await.expect("mock server joined"),
        "oversized wire payload must not reconnect"
    );
    registry.shutdown().await;
}

#[tokio::test]
async fn oversized_single_wire_frame_is_terminal_before_fragment_completion() {
    assert_oversized_wire_frames_are_terminal(vec![Frame::message(
        vec![b'x'; MAX_WEBSOCKET_MESSAGE_BYTES + 1],
        OpCode::Data(Data::Text),
        false,
    )])
    .await;
}

#[tokio::test]
async fn oversized_fragmented_wire_message_is_terminal_before_final_fragment() {
    let first_fragment_bytes = MAX_WEBSOCKET_MESSAGE_BYTES / 2;
    assert_oversized_wire_frames_are_terminal(vec![
        Frame::message(
            vec![b'x'; first_fragment_bytes],
            OpCode::Data(Data::Text),
            false,
        ),
        Frame::message(
            vec![b'x'; MAX_WEBSOCKET_MESSAGE_BYTES - first_fragment_bytes + 1],
            OpCode::Data(Data::Continue),
            false,
        ),
    ])
    .await;
}

#[tokio::test]
async fn oversized_initial_ack_is_rejected_as_protocol_without_reconnect() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock WebSocket server");
    let endpoint = format!("ws://{}", listener.local_addr().expect("listener address"));
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept client");
        let mut socket = accept_async(stream).await.expect("WebSocket handshake");
        socket
            .next()
            .await
            .expect("subscribe command")
            .expect("valid subscribe command");
        let _ = socket
            .send(Message::Frame(Frame::message(
                vec![b'x'; MAX_WEBSOCKET_MESSAGE_BYTES + 1],
                OpCode::Data(Data::Text),
                false,
            )))
            .await;
        drop(socket);
        tokio::time::timeout(Duration::from_millis(750), listener.accept())
            .await
            .is_ok()
    });
    let connector = TiingoConnector::with_endpoints(endpoint.clone(), endpoint)
        .expect("mock endpoint is valid");
    let registry = MarketDataRegistry::with_connector(Some("test-key".into()), connector);

    let error = registry
        .start(StartRequest {
            service: Service::Iex,
            symbols: vec!["AAPL".into()],
            threshold_level: None,
            confirm_iex_market_data_agreement: false,
        })
        .await
        .expect_err("oversized initial acknowledgement must fail start");
    assert!(matches!(error, TiingoError::WebSocketProtocol { .. }));
    assert!(
        !server.await.expect("mock server joined"),
        "oversized initial acknowledgement must not reconnect"
    );
    registry.shutdown().await;
}

#[tokio::test]
async fn oversized_wire_frame_during_update_ack_is_terminal_protocol_without_reconnect() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock WebSocket server");
    let endpoint = format!("ws://{}", listener.local_addr().expect("listener address"));
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept client");
        let mut socket = accept_async(stream).await.expect("WebSocket handshake");
        socket
            .next()
            .await
            .expect("subscribe command")
            .expect("valid subscribe command");
        socket
            .send(Message::text(
                json!({
                    "messageType": "I",
                    "data": {"subscriptionId": "update-wire-id"},
                    "response": {"code": 200, "message": "subscribed"}
                })
                .to_string(),
            ))
            .await
            .expect("send acknowledgement");
        socket
            .next()
            .await
            .expect("update command")
            .expect("valid update command");
        let _ = socket
            .send(Message::Frame(Frame::message(
                vec![b'x'; MAX_WEBSOCKET_MESSAGE_BYTES + 1],
                OpCode::Data(Data::Text),
                false,
            )))
            .await;
        drop(socket);
        tokio::time::timeout(Duration::from_millis(750), listener.accept())
            .await
            .is_ok()
    });
    let connector = TiingoConnector::with_endpoints(endpoint.clone(), endpoint)
        .expect("mock endpoint is valid");
    let registry = MarketDataRegistry::with_connector(Some("test-key".into()), connector);
    let started = registry
        .start(StartRequest {
            service: Service::Iex,
            symbols: vec!["AAPL".into()],
            threshold_level: None,
            confirm_iex_market_data_agreement: false,
        })
        .await
        .expect("subscription starts");

    let error = registry
        .update(
            &started.id,
            UpdateRequest {
                add_symbols: vec!["MSFT".into()],
                remove_symbols: Vec::new(),
                threshold_level: None,
            },
        )
        .await
        .expect_err("oversized update response fails the mutation");
    assert_eq!(error.payload().kind, "websocket_protocol");
    let terminal = registry
        .poll(&started.id, u64::MAX)
        .await
        .expect("terminal state remains inspectable");
    assert_eq!(terminal.state, SubscriptionStatus::Failed);
    assert_eq!(terminal.terminal_error, Some(TerminalErrorKind::Protocol));
    assert!(
        !server.await.expect("mock server joined"),
        "oversized update response must not reconnect"
    );
    registry.shutdown().await;
}

#[tokio::test]
async fn poll_caps_deterministic_replay_by_event_count_and_serialized_bytes() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock WebSocket server");
    let endpoint = format!("ws://{}", listener.local_addr().expect("listener address"));
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept client");
        let mut socket = accept_async(stream).await.expect("WebSocket handshake");
        socket
            .next()
            .await
            .expect("subscribe command")
            .expect("valid subscribe command");
        socket
            .send(Message::text(
                json!({
                    "messageType": "I",
                    "data": {"subscriptionId": 42},
                    "response": {"code": 200, "message": "subscribed"}
                })
                .to_string(),
            ))
            .await
            .expect("send acknowledgement");
        let padding = "x".repeat(5_000);
        for index in 0..300 {
            socket
                .send(Message::text(
                    json!({
                        "messageType": "A",
                        "service": "iex",
                        "data": ["2026-08-25T14:00:00Z", "AAPL", index as f64],
                        "padding": padding
                    })
                    .to_string(),
                ))
                .await
                .expect("send market message");
        }
        while let Some(message) = socket.next().await {
            if matches!(message, Ok(Message::Close(_))) {
                break;
            }
        }
    });
    let connector = TiingoConnector::with_endpoints(endpoint.clone(), endpoint)
        .expect("mock endpoint is valid");
    let registry = MarketDataRegistry::with_connector(Some("test-key".into()), connector);
    let started = registry
        .start(StartRequest {
            service: Service::Iex,
            symbols: vec!["AAPL".into()],
            threshold_level: None,
            confirm_iex_market_data_agreement: false,
        })
        .await
        .expect("subscription starts");

    let tail = registry
        .poll(&started.id, 299)
        .await
        .expect("tail poll succeeds");
    assert_eq!(tail.events[0].sequence, 300);
    let first = registry
        .poll(&started.id, 0)
        .await
        .expect("bounded poll succeeds");
    assert!(first.events.len() <= 256);
    assert!(serde_json::to_vec(&first).expect("serialize poll").len() <= 1024 * 1024);
    assert_eq!(first.events[0].sequence, 1);
    let replay = registry
        .poll(&started.id, 0)
        .await
        .expect("replay succeeds");
    assert_eq!(replay, first);
    let continuation = registry
        .poll(
            &started.id,
            first.events.last().expect("first page event").sequence,
        )
        .await
        .expect("continuation succeeds");
    assert_eq!(
        continuation.events[0].sequence,
        first.events.last().expect("first page event").sequence + 1
    );

    registry.shutdown().await;
    server.await.expect("mock server exits cleanly");
}

#[tokio::test]
async fn queue_overflow_is_terminal_data_gap_and_never_silently_drops() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock WebSocket server");
    let endpoint = format!("ws://{}", listener.local_addr().expect("listener address"));
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept client");
        let mut socket = accept_async(stream).await.expect("WebSocket handshake");
        socket
            .next()
            .await
            .expect("subscribe command")
            .expect("valid subscribe command");
        socket
            .send(Message::text(
                json!({
                    "messageType": "I",
                    "data": {"subscriptionId": 43},
                    "response": {"code": 200, "message": "subscribed"}
                })
                .to_string(),
            ))
            .await
            .expect("send acknowledgement");
        for index in 0..=2_048 {
            socket
                .send(Message::text(
                    json!({
                        "messageType": "A",
                        "service": "iex",
                        "data": ["2026-08-25T14:00:00Z", "AAPL", index as f64]
                    })
                    .to_string(),
                ))
                .await
                .expect("send market message");
        }
        while let Some(message) = socket.next().await {
            if matches!(message, Ok(Message::Close(_))) {
                return true;
            }
        }
        false
    });
    let connector = TiingoConnector::with_endpoints(endpoint.clone(), endpoint)
        .expect("mock endpoint is valid");
    let registry = MarketDataRegistry::with_connector(Some("test-key".into()), connector);
    let started = registry
        .start(StartRequest {
            service: Service::Iex,
            symbols: vec!["AAPL".into()],
            threshold_level: None,
            confirm_iex_market_data_agreement: false,
        })
        .await
        .expect("subscription starts");

    let terminal = registry
        .poll(&started.id, 2_048)
        .await
        .expect("terminal state remains inspectable");
    assert_eq!(terminal.state, SubscriptionStatus::DataGap);
    assert!(terminal.events.is_empty());
    assert!(server.await.expect("mock server exits"));

    let retained = registry
        .poll(&started.id, 0)
        .await
        .expect("retained poll succeeds");
    assert_eq!(retained.state, SubscriptionStatus::DataGap);
    assert_eq!(retained.events[0].sequence, 1);
    registry.shutdown().await;
}

#[tokio::test]
async fn duplicate_and_older_observations_keep_arrival_order_and_are_flagged() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock WebSocket server");
    let endpoint = format!("ws://{}", listener.local_addr().expect("listener address"));
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept client");
        let mut socket = accept_async(stream).await.expect("WebSocket handshake");
        socket
            .next()
            .await
            .expect("subscribe command")
            .expect("valid subscribe command");
        socket
            .send(Message::text(
                json!({
                    "messageType": "I",
                    "data": {"subscriptionId": 44},
                    "response": {"code": 200, "message": "subscribed"}
                })
                .to_string(),
            ))
            .await
            .expect("send acknowledgement");
        let newest = json!({
            "messageType": "A",
            "service": "iex",
            "data": ["2026-08-25T14:00:02Z", "AAPL", 102.0]
        })
        .to_string();
        socket
            .send(Message::text(newest.clone()))
            .await
            .expect("send newest observation");
        socket
            .send(Message::text(newest))
            .await
            .expect("send exact duplicate");
        socket
            .send(Message::text(
                json!({
                    "messageType": "A",
                    "service": "iex",
                    "data": ["2026-08-25T14:00:01Z", "AAPL", 101.0]
                })
                .to_string(),
            ))
            .await
            .expect("send older observation");
        while let Some(message) = socket.next().await {
            if matches!(message, Ok(Message::Close(_))) {
                break;
            }
        }
    });
    let connector = TiingoConnector::with_endpoints(endpoint.clone(), endpoint)
        .expect("mock endpoint is valid");
    let registry = MarketDataRegistry::with_connector(Some("test-key".into()), connector);
    let started = registry
        .start(StartRequest {
            service: Service::Iex,
            symbols: vec!["AAPL".into()],
            threshold_level: None,
            confirm_iex_market_data_agreement: false,
        })
        .await
        .expect("subscription starts");
    let tail = registry
        .poll(&started.id, 2)
        .await
        .expect("tail poll succeeds");
    assert_eq!(tail.events[0].sequence, 3);
    let events = registry
        .poll(&started.id, 0)
        .await
        .expect("full poll succeeds")
        .events;

    assert_eq!(events.len(), 3);
    assert_eq!(
        (events[0].duplicate, events[0].out_of_order),
        (false, false)
    );
    assert_eq!((events[1].duplicate, events[1].out_of_order), (true, false));
    assert_eq!((events[2].duplicate, events[2].out_of_order), (false, true));
    assert_eq!(
        events
            .iter()
            .map(|event| event.sequence)
            .collect::<Vec<_>>(),
        vec![1, 2, 3]
    );

    registry.shutdown().await;
    server.await.expect("mock server exits cleanly");
}

#[tokio::test]
async fn update_ack_keeps_published_replay_bytes_and_duplicate_classification_stable() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock WebSocket server");
    let endpoint = format!("ws://{}", listener.local_addr().expect("listener address"));
    let (initial_polled, release_update) = tokio::sync::oneshot::channel();
    let observation = json!({
        "messageType": "A",
        "service": "iex",
        "data": ["2026-08-25T14:00:05Z", "AAPL", 5.0]
    })
    .to_string();
    let server_observation = observation.clone();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept client");
        let mut socket = accept_async(stream).await.expect("WebSocket handshake");
        socket
            .next()
            .await
            .expect("subscribe command")
            .expect("valid subscribe command");
        socket
            .send(Message::text(
                json!({
                    "messageType": "I",
                    "data": {"subscriptionId": 4},
                    "response": {"code": 200, "message": "subscribed"}
                })
                .to_string(),
            ))
            .await
            .expect("send initial acknowledgement");
        socket
            .send(Message::text(server_observation.clone()))
            .await
            .expect("send initial observation");
        release_update.await.expect("initial event was polled");
        socket
            .next()
            .await
            .expect("update command")
            .expect("valid update command");
        socket
            .send(Message::text(
                json!({
                    "messageType": "I",
                    "data": {"subscriptionId": 5},
                    "response": {"code": 200, "message": "updated"}
                })
                .to_string(),
            ))
            .await
            .expect("send update acknowledgement");
        socket
            .send(Message::text(server_observation))
            .await
            .expect("send exact duplicate after acknowledgement");
        while let Some(message) = socket.next().await {
            if matches!(message, Ok(Message::Close(_))) {
                break;
            }
        }
    });
    let connector = TiingoConnector::with_endpoints(endpoint.clone(), endpoint)
        .expect("mock endpoint is valid");
    let registry = MarketDataRegistry::with_connector(Some("test-key".into()), connector);
    let started = registry
        .start(StartRequest {
            service: Service::Iex,
            symbols: vec!["AAPL".into()],
            threshold_level: None,
            confirm_iex_market_data_agreement: false,
        })
        .await
        .expect("subscription starts");
    let before = registry
        .poll(&started.id, 0)
        .await
        .expect("initial observation is poll-visible");
    let before_bytes = serde_json::to_vec(&before.events[0]).expect("serialize initial event");
    initial_polled
        .send(())
        .expect("release update after initial poll");
    registry
        .update(
            &started.id,
            UpdateRequest {
                add_symbols: vec!["MSFT".into()],
                remove_symbols: Vec::new(),
                threshold_level: None,
            },
        )
        .await
        .expect("update succeeds");
    let duplicate = registry
        .poll(&started.id, before.events[0].sequence)
        .await
        .expect("duplicate observation is poll-visible");
    let replay = registry
        .poll(&started.id, 0)
        .await
        .expect("original cursor replays");
    let replay_bytes = serde_json::to_vec(&replay.events[0]).expect("serialize replayed event");

    assert_eq!(before.events[0].payload["data"][2], 5.0);
    assert_eq!(duplicate.events[0].payload["data"][2], 5.0);
    assert!(
        replay_bytes == before_bytes && duplicate.events[0].duplicate,
        "acknowledgement must not mutate replay bytes or stale duplicate truth; replay_stable={}, duplicate={}",
        replay_bytes == before_bytes,
        duplicate.events[0].duplicate
    );

    registry.shutdown().await;
    server.await.expect("mock server exits cleanly");
}

#[tokio::test]
async fn update_serializes_remove_then_add_with_each_acknowledged_subscription_id() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock WebSocket server");
    let endpoint = format!("ws://{}", listener.local_addr().expect("listener address"));
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept client");
        let mut socket = accept_async(stream).await.expect("WebSocket handshake");
        socket
            .next()
            .await
            .expect("subscribe command")
            .expect("valid subscribe command");
        socket
            .send(Message::text(
                json!({
                    "messageType": "I",
                    "data": {"subscriptionId": "initial-secret"},
                    "response": {"code": 200, "message": "subscribed"}
                })
                .to_string(),
            ))
            .await
            .expect("send initial acknowledgement");

        let remove = socket
            .next()
            .await
            .expect("remove command")
            .expect("valid remove command")
            .into_text()
            .expect("text remove command");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&remove).expect("remove JSON"),
            json!({
                "eventName": "unsubscribe",
                "authorization": "test-key",
                "eventData": {"subscriptionId": "initial-secret", "tickers": ["MSFT"]}
            })
        );
        socket
            .send(Message::text(
                json!({
                    "messageType": "I",
                    "data": {"subscriptionId": "after-remove-secret"},
                    "response": {"code": 200, "message": "updated"}
                })
                .to_string(),
            ))
            .await
            .expect("send remove acknowledgement");

        let add = socket
            .next()
            .await
            .expect("add command")
            .expect("valid add command")
            .into_text()
            .expect("text add command");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&add).expect("add JSON"),
            json!({
                "eventName": "subscribe",
                "authorization": "test-key",
                "eventData": {"subscriptionId": "after-remove-secret", "tickers": ["NVDA"]}
            })
        );
        socket
            .send(Message::text(
                json!({
                    "messageType": "I",
                    "data": {"subscriptionId": "after-add-secret"},
                    "response": {"code": 200, "message": "updated"}
                })
                .to_string(),
            ))
            .await
            .expect("send add acknowledgement");
        while let Some(message) = socket.next().await {
            if matches!(message, Ok(Message::Close(_))) {
                break;
            }
        }
    });
    let connector = TiingoConnector::with_endpoints(endpoint.clone(), endpoint)
        .expect("mock endpoint is valid");
    let registry = MarketDataRegistry::with_connector(Some("test-key".into()), connector);
    let started = registry
        .start(StartRequest {
            service: Service::Iex,
            symbols: vec!["AAPL".into(), "MSFT".into()],
            threshold_level: None,
            confirm_iex_market_data_agreement: false,
        })
        .await
        .expect("subscription starts");

    let updated = registry
        .update(
            &started.id,
            UpdateRequest {
                add_symbols: vec!["nvda".into()],
                remove_symbols: vec!["msft".into()],
                threshold_level: None,
            },
        )
        .await
        .expect("acknowledged update succeeds");
    assert_eq!(updated.state, SubscriptionStatus::Active);
    assert_eq!(updated.symbols, vec!["AAPL", "NVDA"]);
    let debug = format!("{updated:?}");
    assert!(!debug.contains("initial-secret"));
    assert!(!debug.contains("after-remove-secret"));
    assert!(!debug.contains("after-add-secret"));

    registry.shutdown().await;
    server.await.expect("mock server exits cleanly");
}

#[tokio::test]
async fn update_rejects_an_empty_result_without_mutating_or_stopping_the_subscription() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock WebSocket server");
    let endpoint = format!("ws://{}", listener.local_addr().expect("listener address"));
    let (send_data, receive_data) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept client");
        let mut socket = accept_async(stream).await.expect("WebSocket handshake");
        socket
            .next()
            .await
            .expect("subscribe command")
            .expect("valid subscribe command");
        socket
            .send(Message::text(
                json!({
                    "messageType": "I",
                    "data": {"subscriptionId": 45},
                    "response": {"code": 200, "message": "subscribed"}
                })
                .to_string(),
            ))
            .await
            .expect("send acknowledgement");
        receive_data.await.expect("test requests market event");
        socket
            .send(Message::text(
                json!({
                    "messageType": "A",
                    "service": "iex",
                    "data": ["2026-08-25T14:00:00Z", "AAPL", 100.0]
                })
                .to_string(),
            ))
            .await
            .expect("subscription remains connected");
        while let Some(message) = socket.next().await {
            if matches!(message, Ok(Message::Close(_))) {
                break;
            }
        }
    });
    let connector = TiingoConnector::with_endpoints(endpoint.clone(), endpoint)
        .expect("mock endpoint is valid");
    let registry = MarketDataRegistry::with_connector(Some("test-key".into()), connector);
    let started = registry
        .start(StartRequest {
            service: Service::Iex,
            symbols: vec!["AAPL".into()],
            threshold_level: None,
            confirm_iex_market_data_agreement: false,
        })
        .await
        .expect("subscription starts");

    let error = registry
        .update(
            &started.id,
            UpdateRequest {
                add_symbols: Vec::new(),
                remove_symbols: vec!["AAPL".into()],
                threshold_level: None,
            },
        )
        .await
        .expect_err("an update may not remove the last symbol");
    assert_eq!(error.payload().kind, "validation");
    send_data.send(()).expect("request market event");
    let poll = registry
        .poll(&started.id, 0)
        .await
        .expect("poll remains available");
    assert_eq!(poll.state, SubscriptionStatus::Active);
    assert_eq!(poll.events.len(), 1);

    registry.shutdown().await;
    server.await.expect("mock server exits cleanly");
}

#[tokio::test]
async fn update_rejects_threshold_changes_and_projected_symbol_overflow_before_sending() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock WebSocket server");
    let endpoint = format!("ws://{}", listener.local_addr().expect("listener address"));
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept client");
        let mut socket = accept_async(stream).await.expect("WebSocket handshake");
        socket
            .next()
            .await
            .expect("subscribe command")
            .expect("valid subscribe command");
        socket
            .send(Message::text(
                json!({
                    "messageType": "I",
                    "data": {"subscriptionId": 59},
                    "response": {"code": 200, "message": "subscribed"}
                })
                .to_string(),
            ))
            .await
            .expect("send acknowledgement");
        let first_post_start_frame = socket
            .next()
            .await
            .expect("stop unsubscribe frame")
            .expect("valid stop frame")
            .into_text()
            .expect("text stop frame");
        let first_post_start_frame: serde_json::Value =
            serde_json::from_str(&first_post_start_frame).expect("stop frame JSON");
        assert_eq!(first_post_start_frame["eventName"], "unsubscribe");
        assert_eq!(
            first_post_start_frame["eventData"]["tickers"]
                .as_array()
                .expect("stop tickers")
                .len(),
            100
        );
        let _ = socket.close(None).await;
    });
    let connector = TiingoConnector::with_endpoints(endpoint.clone(), endpoint)
        .expect("mock endpoint is valid");
    let registry = MarketDataRegistry::with_connector(Some("test-key".into()), connector);
    let symbols = (0..100).map(|index| format!("S{index}")).collect();
    let started = registry
        .start(StartRequest {
            service: Service::Iex,
            symbols,
            threshold_level: None,
            confirm_iex_market_data_agreement: false,
        })
        .await
        .expect("subscription starts");

    let threshold_error = registry
        .update(
            &started.id,
            UpdateRequest {
                add_symbols: vec!["MSFT".into()],
                remove_symbols: vec!["S0".into()],
                threshold_level: Some(5),
            },
        )
        .await
        .expect_err("threshold mutation is rejected");
    assert_eq!(threshold_error.payload().kind, "validation");
    assert!(threshold_error.to_string().contains("stopping"));
    assert!(threshold_error.to_string().contains("starting"));

    let overflow_error = registry
        .update(
            &started.id,
            UpdateRequest {
                add_symbols: vec!["OVER".into()],
                remove_symbols: vec![],
                threshold_level: None,
            },
        )
        .await
        .expect_err("projected 101-symbol subscription is rejected");
    assert_eq!(overflow_error.payload().kind, "validation");
    assert!(overflow_error.to_string().contains("between 1 and 100"));

    let stopped = registry
        .stop(&started.id)
        .await
        .expect("session remains stoppable");
    assert_eq!(stopped.state, SubscriptionStatus::Stopped);
    registry.shutdown().await;
    server.await.expect("mock server exits cleanly");
}

#[tokio::test]
async fn stop_unsubscribes_closes_joins_and_is_idempotent() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock WebSocket server");
    let endpoint = format!("ws://{}", listener.local_addr().expect("listener address"));
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept client");
        let mut socket = accept_async(stream).await.expect("WebSocket handshake");
        socket
            .next()
            .await
            .expect("subscribe command")
            .expect("valid subscribe command");
        socket
            .send(Message::text(
                json!({
                    "messageType": "I",
                    "data": {"subscriptionId": "stop-secret"},
                    "response": {"code": 200, "message": "subscribed"}
                })
                .to_string(),
            ))
            .await
            .expect("send acknowledgement");
        let unsubscribe = socket
            .next()
            .await
            .expect("unsubscribe command")
            .expect("valid unsubscribe command")
            .into_text()
            .expect("text unsubscribe command");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&unsubscribe).expect("unsubscribe JSON"),
            json!({
                "eventName": "unsubscribe",
                "authorization": "test-key",
                "eventData": {"subscriptionId": "stop-secret", "tickers": ["AAPL"]}
            })
        );
        matches!(socket.next().await, Some(Ok(Message::Close(_))) | None)
    });
    let connector = TiingoConnector::with_endpoints(endpoint.clone(), endpoint)
        .expect("mock endpoint is valid");
    let registry = MarketDataRegistry::with_connector(Some("test-key".into()), connector);
    let started = registry
        .start(StartRequest {
            service: Service::Iex,
            symbols: vec!["AAPL".into()],
            threshold_level: None,
            confirm_iex_market_data_agreement: false,
        })
        .await
        .expect("subscription starts");

    let stopped = registry.stop(&started.id).await.expect("stop succeeds");
    assert_eq!(stopped.state, SubscriptionStatus::Stopped);
    assert!(server.await.expect("unsubscribe precedes clean close"));
    let stopped_again = registry
        .stop(&started.id)
        .await
        .expect("second stop succeeds");
    assert_eq!(stopped_again, stopped);

    registry.shutdown().await;
}

#[tokio::test]
async fn inactive_subscription_expires_after_five_minutes_despite_heartbeats() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock WebSocket server");
    let endpoint = format!("ws://{}", listener.local_addr().expect("listener address"));
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept client");
        let mut socket = accept_async(stream).await.expect("WebSocket handshake");
        socket
            .next()
            .await
            .expect("subscribe command")
            .expect("valid subscribe command");
        socket
            .send(Message::text(
                json!({
                    "messageType": "I",
                    "data": {"subscriptionId": 46},
                    "response": {"code": 200, "message": "subscribed"}
                })
                .to_string(),
            ))
            .await
            .expect("send acknowledgement");
        let mut heartbeat = tokio::time::interval(Duration::from_secs(30));
        loop {
            tokio::select! {
                _ = heartbeat.tick() => {
                    if socket.send(Message::text(json!({
                        "messageType": "H",
                        "response": {"code": 200, "message": "heartbeat"}
                    }).to_string())).await.is_err() {
                        return;
                    }
                }
                incoming = socket.next() => match incoming {
                    Some(Ok(Message::Close(_))) | None => return,
                    _ => {}
                }
            }
        }
    });
    let connector = TiingoConnector::with_endpoints(endpoint.clone(), endpoint)
        .expect("mock endpoint is valid");
    let registry = MarketDataRegistry::with_connector(Some("test-key".into()), connector);
    let started = registry
        .start(StartRequest {
            service: Service::Iex,
            symbols: vec!["AAPL".into()],
            threshold_level: None,
            confirm_iex_market_data_agreement: false,
        })
        .await
        .expect("subscription starts");

    tokio::time::pause();
    for _ in 0..10 {
        tokio::time::advance(Duration::from_secs(30)).await;
        tokio::task::yield_now().await;
    }
    tokio::time::advance(Duration::from_millis(1)).await;
    for _ in 0..20 {
        tokio::task::yield_now().await;
    }
    assert!(server.is_finished(), "idle expiry must close the socket");
    server.await.expect("mock server exits cleanly");
    let expired = registry
        .poll(&started.id, 0)
        .await
        .expect("expiry remains inspectable");
    assert_eq!(expired.state, SubscriptionStatus::Expired);

    registry.shutdown().await;
}

#[tokio::test]
async fn partial_update_reports_applied_symbols_at_absolute_expiry() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock WebSocket server");
    let endpoint = format!("ws://{}", listener.local_addr().expect("listener address"));
    let (add_seen, add_received) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept client");
        let mut socket = accept_async(stream).await.expect("WebSocket handshake");
        socket
            .next()
            .await
            .expect("subscribe command")
            .expect("valid subscribe command");
        socket
            .send(Message::text(
                json!({
                    "messageType": "I",
                    "data": {"subscriptionId": 47},
                    "response": {"code": 200, "message": "subscribed"}
                })
                .to_string(),
            ))
            .await
            .expect("send acknowledgement");
        let mut heartbeat = tokio::time::interval(Duration::from_secs(30));
        let mut removed = false;
        let mut add_seen = Some(add_seen);
        loop {
            tokio::select! {
                _ = heartbeat.tick() => {
                    if socket.send(Message::text(json!({
                        "messageType": "H",
                        "response": {"code": 200, "message": "heartbeat"}
                    }).to_string())).await.is_err() {
                        return;
                    }
                }
                incoming = socket.next() => match incoming {
                    Some(Ok(Message::Text(payload))) if !removed => {
                        let payload: serde_json::Value =
                            serde_json::from_str(&payload).expect("remove JSON");
                        assert_eq!(payload["eventName"], "unsubscribe");
                        socket.send(Message::text(json!({
                            "messageType": "I",
                            "data": {"subscriptionId": "expiry-remove-id"},
                            "response": {"code": 200, "message": "updated"}
                        }).to_string())).await.expect("acknowledge remove");
                        removed = true;
                    }
                    Some(Ok(Message::Text(payload))) => {
                        let payload: serde_json::Value =
                            serde_json::from_str(&payload).expect("add JSON");
                        if let Some(add_seen) = add_seen.take() {
                            assert_eq!(payload["eventName"], "subscribe");
                            add_seen.send(()).expect("record pending add");
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => return,
                    Some(Ok(_)) => {}
                    Some(Err(_)) if add_seen.is_none() => return,
                    Some(Err(error)) => panic!("receive client message: {error}"),
                }
            }
        }
    });
    let connector = TiingoConnector::with_endpoints(endpoint.clone(), endpoint)
        .expect("mock endpoint is valid");
    let registry = MarketDataRegistry::with_connector(Some("test-key".into()), connector);
    let started = registry
        .start(StartRequest {
            service: Service::Iex,
            symbols: vec!["AAPL".into(), "MSFT".into()],
            threshold_level: None,
            confirm_iex_market_data_agreement: false,
        })
        .await
        .expect("subscription starts");

    tokio::time::pause();
    for _ in 0..29 {
        tokio::time::advance(Duration::from_secs(60)).await;
        for _ in 0..10 {
            tokio::task::yield_now().await;
        }
        registry
            .poll_with_bounds(&started.id, 0, 1, Duration::ZERO)
            .await
            .expect("minute touch succeeds");
    }
    assert!(
        !server.is_finished(),
        "activity keeps the idle deadline refreshed"
    );
    tokio::time::advance(Duration::from_secs(56)).await;
    let update_registry = registry.clone();
    let update_id = started.id.clone();
    let update = tokio::spawn(async move {
        update_registry
            .update(
                &update_id,
                UpdateRequest {
                    add_symbols: vec!["NVDA".into()],
                    remove_symbols: vec!["AAPL".into()],
                    threshold_level: None,
                },
            )
            .await
    });
    tokio::time::resume();
    add_received.await.expect("server sees pending add");
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(4)).await;
    let error = update
        .await
        .expect("update task joins")
        .expect_err("absolute expiry interrupts the pending add");
    let error = serde_json::to_value(error.payload()).expect("serialize expiry update error");
    assert_eq!(error["kind"], "timeout");
    assert_eq!(error["appliedSymbols"], json!(["MSFT"]));
    tokio::time::resume();
    tokio::time::timeout(Duration::from_secs(1), server)
        .await
        .expect("absolute expiry closes the socket promptly")
        .expect("mock server exits cleanly");
    let expired = registry
        .poll_with_bounds(&started.id, 0, 1, Duration::ZERO)
        .await
        .expect("expiry remains inspectable");
    assert_eq!(expired.state, SubscriptionStatus::Expired);

    registry.shutdown().await;
}

#[tokio::test]
async fn liveness_fails_after_seventy_five_seconds_and_heartbeat_and_data_refresh_it() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock WebSocket server");
    let endpoint = format!("ws://{}", listener.local_addr().expect("listener address"));
    let (refresh, mut refreshes) = tokio::sync::mpsc::unbounded_channel::<(
        &'static str,
        u8,
        tokio::sync::oneshot::Sender<()>,
    )>();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept client");
        let mut socket = accept_async(stream).await.expect("WebSocket handshake");
        socket
            .next()
            .await
            .expect("subscribe command")
            .expect("valid subscribe command");
        socket
            .send(Message::text(
                json!({
                    "messageType": "I",
                    "data": {"subscriptionId": 48},
                    "response": {"code": 200, "message": "subscribed"}
                })
                .to_string(),
            ))
            .await
            .expect("send acknowledgement");
        loop {
            tokio::select! {
                refresh = refreshes.recv() => {
                    let Some((kind, cycle, processed)) = refresh else { return };
                    let message = match kind {
                        "heartbeat" => json!({
                            "messageType": "H",
                            "response": {"code": 200, "message": "heartbeat"}
                        }),
                        "data" => json!({
                            "messageType": "A",
                            "service": "iex",
                            "data": ["2026-08-25T14:00:00Z", "AAPL", 100.0]
                        }),
                        "raw" => json!({
                            "messageType": "U",
                            "data": {"ticker": "AAPL", "value": 101.0}
                        }),
                        _ => panic!("unknown liveness refresh kind"),
                    };
                    socket
                        .send(Message::text(message.to_string()))
                        .await
                        .expect("send liveness refresh");
                    let ping_payload = vec![cycle];
                    socket
                        .send(Message::Ping(ping_payload.clone().into()))
                        .await
                        .expect("send liveness processing barrier");
                    loop {
                        match socket.next().await {
                            Some(Ok(Message::Pong(payload))) if payload.as_ref() == ping_payload => {
                                break;
                            }
                            Some(Ok(_)) => {}
                            Some(Err(error)) => panic!("receive liveness barrier: {error}"),
                            None => panic!("liveness barrier socket remains open"),
                        }
                    }
                    processed.send(()).expect("record processed liveness refresh");
                },
                incoming = socket.next() => match incoming {
                    Some(Ok(Message::Close(_))) | None => return,
                    _ => {}
                }
            }
        }
    });
    let connector = TiingoConnector::with_endpoints(endpoint.clone(), endpoint)
        .expect("mock endpoint is valid");
    let registry = MarketDataRegistry::with_connector(Some("test-key".into()), connector);
    let started = registry
        .start(StartRequest {
            service: Service::Iex,
            symbols: vec!["AAPL".into()],
            threshold_level: None,
            confirm_iex_market_data_agreement: false,
        })
        .await
        .expect("subscription starts");

    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(74)).await;
    assert!(!server.is_finished());
    tokio::time::resume();
    let (processed, processing_complete) = tokio::sync::oneshot::channel();
    refresh
        .send(("heartbeat", 1, processed))
        .expect("request heartbeat");
    processing_complete
        .await
        .expect("worker processes heartbeat");
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(74)).await;
    assert!(!server.is_finished());
    tokio::time::resume();
    let (processed, processing_complete) = tokio::sync::oneshot::channel();
    refresh
        .send(("data", 2, processed))
        .expect("request market data");
    processing_complete
        .await
        .expect("worker processes market data");
    let data_poll = registry
        .poll(&started.id, 0)
        .await
        .expect("market data is processed");
    assert_eq!(data_poll.events.len(), 1);
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(74)).await;
    assert!(!server.is_finished());
    tokio::time::resume();
    let (processed, processing_complete) = tokio::sync::oneshot::channel();
    refresh
        .send(("raw", 3, processed))
        .expect("request raw market update");
    processing_complete
        .await
        .expect("worker processes raw market update");
    let raw_poll = registry
        .poll(
            &started.id,
            data_poll.events.last().expect("market event").sequence,
        )
        .await
        .expect("raw update is processed");
    assert_eq!(raw_poll.events.len(), 1);
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(74)).await;
    assert!(!server.is_finished());
    tokio::time::advance(Duration::from_secs(1)).await;
    tokio::time::advance(Duration::from_millis(1)).await;
    tokio::time::resume();
    tokio::time::timeout(Duration::from_secs(1), server)
        .await
        .expect("75 seconds without data or heartbeat closes the socket")
        .expect("mock server exits cleanly");
    let failed = registry
        .poll_with_bounds(&started.id, u64::MAX, 1, Duration::ZERO)
        .await
        .expect("liveness failure remains inspectable");
    assert_eq!(failed.state, SubscriptionStatus::Reconnecting);

    let stopped = registry
        .stop(&started.id)
        .await
        .expect("stop cancels reconnect");
    assert_eq!(stopped.state, SubscriptionStatus::Stopped);
    registry.shutdown().await;
}

#[tokio::test]
async fn disconnect_reconnects_five_times_with_exact_backoff_and_fresh_subscribes() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock WebSocket server");
    let endpoint = format!("ws://{}", listener.local_addr().expect("listener address"));
    let (disconnect, disconnected) = tokio::sync::oneshot::channel();
    let (ready, ready_for_reconnects) = tokio::sync::oneshot::channel();
    let (accepted, mut accepts) = tokio::sync::mpsc::unbounded_channel();
    let (rejected, mut rejections) = tokio::sync::mpsc::unbounded_channel();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept initial client");
        let mut socket = accept_async(stream)
            .await
            .expect("initial WebSocket handshake");
        socket
            .next()
            .await
            .expect("initial subscribe")
            .expect("valid initial subscribe");
        socket
            .send(Message::text(
                json!({
                    "messageType": "I",
                    "data": {"subscriptionId": "first-upstream-id"},
                    "response": {"code": 200, "message": "subscribed"}
                })
                .to_string(),
            ))
            .await
            .expect("send initial acknowledgement");
        disconnected.await.expect("test requests disconnect");
        socket
            .send(Message::Close(None))
            .await
            .expect("send disconnect close");
        drop(socket);
        ready.send(()).expect("record initial disconnect");

        for attempt in 1..=5 {
            let (stream, _) = listener.accept().await.expect("accept reconnect");
            accepted.send(attempt).expect("record reconnect");
            let mut socket = accept_async(stream)
                .await
                .expect("reconnect WebSocket handshake");
            let command = socket
                .next()
                .await
                .expect("fresh subscribe")
                .expect("valid fresh subscribe")
                .into_text()
                .expect("text subscribe");
            let command: serde_json::Value =
                serde_json::from_str(&command).expect("fresh subscribe JSON");
            assert_eq!(command["eventName"], "subscribe");
            assert_eq!(command["eventData"]["thresholdLevel"], 6);
            assert_eq!(command["eventData"]["tickers"], json!(["AAPL"]));
            assert!(command["eventData"].get("subscriptionId").is_none());
            socket
                .send(Message::Close(None))
                .await
                .expect("reject reconnect before acknowledgement");
            rejected.send(attempt).expect("record reconnect rejection");
        }
    });
    let connector = TiingoConnector::with_endpoints(endpoint.clone(), endpoint)
        .expect("mock endpoint is valid");
    let (delay_requests, mut requested_delays) = tokio::sync::mpsc::unbounded_channel();
    let registry = MarketDataRegistry::with_connector_and_clocks(
        Some("test-key".into()),
        connector,
        Arc::new(FixedClock(Utc::now())),
        Arc::new(ManualReconnectClock {
            requests: delay_requests,
        }),
    );
    let started = registry
        .start(StartRequest {
            service: Service::Iex,
            symbols: vec!["AAPL".into()],
            threshold_level: None,
            confirm_iex_market_data_agreement: false,
        })
        .await
        .expect("subscription starts");

    disconnect.send(()).expect("disconnect initial socket");
    ready_for_reconnects
        .await
        .expect("mock server is ready for reconnects");
    for (index, expected_delay) in [250_u64, 500, 1_000, 2_000, 4_000].into_iter().enumerate() {
        let (actual_delay, release) = requested_delays
            .recv()
            .await
            .expect("worker requests reconnect delay");
        assert_eq!(actual_delay, Duration::from_millis(expected_delay));
        assert!(
            accepts.try_recv().is_err(),
            "there is no reconnect before the delay"
        );
        release.send(()).expect("release reconnect delay");
        let attempt = accepts.recv().await.expect("reconnect occurs after delay");
        assert_eq!(attempt, index + 1);
        assert!(
            accepts.try_recv().is_err(),
            "only one reconnect occurs per delay"
        );
        assert_eq!(
            rejections.recv().await.expect("server rejects reconnect"),
            attempt
        );
    }
    assert!(
        requested_delays.try_recv().is_err(),
        "there is no sixth reconnect delay"
    );
    server
        .await
        .expect("mock server handles all reconnect attempts");
    let failed = tokio::time::timeout(Duration::from_secs(1), registry.poll(&started.id, u64::MAX))
        .await
        .expect("reconnect exhaustion becomes terminal promptly")
        .expect("reconnect exhaustion remains inspectable");
    assert_eq!(failed.state, SubscriptionStatus::Failed);
    assert_eq!(failed.terminal_error, Some(TerminalErrorKind::Transport));

    registry.shutdown().await;
}

#[tokio::test]
async fn recognizable_entitlement_error_is_terminal_sanitized_and_never_retried() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock WebSocket server");
    let endpoint = format!("ws://{}", listener.local_addr().expect("listener address"));
    let accepts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let server_accepts = Arc::clone(&accepts);
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept client");
        server_accepts.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let mut socket = accept_async(stream).await.expect("WebSocket handshake");
        socket
            .next()
            .await
            .expect("subscribe command")
            .expect("valid subscribe command");
        socket
            .send(Message::text(
                json!({
                    "messageType": "E",
                    "response": {
                        "code": 403,
                        "message": "entitlement denied for test-key and upstream-secret"
                    }
                })
                .to_string(),
            ))
            .await
            .expect("send entitlement rejection");
        let _ = socket.next().await;
    });
    let connector = TiingoConnector::with_endpoints(endpoint.clone(), endpoint)
        .expect("mock endpoint is valid");
    let registry = MarketDataRegistry::with_connector(Some("test-key".into()), connector);

    let error = registry
        .start(StartRequest {
            service: Service::Iex,
            symbols: vec!["AAPL".into()],
            threshold_level: None,
            confirm_iex_market_data_agreement: false,
        })
        .await
        .expect_err("entitlement rejection fails start");

    assert_eq!(error.payload().kind, "entitlement");
    let snapshots = format!("{error:?} {} {registry:?}", error.payload().message);
    assert!(!snapshots.contains("test-key"));
    assert!(!snapshots.contains("upstream-secret"));
    server.await.expect("mock server observes cleanup close");
    assert_eq!(accepts.load(std::sync::atomic::Ordering::SeqCst), 1);
    registry.shutdown().await;
}

#[tokio::test]
async fn information_acknowledgement_401_and_403_are_sanitized_terminal_rejections() {
    for (code, expected_kind) in [(401, "authentication"), (403, "entitlement")] {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock WebSocket server");
        let endpoint = format!("ws://{}", listener.local_addr().expect("listener address"));
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept client");
            let mut socket = accept_async(stream).await.expect("WebSocket handshake");
            socket
                .next()
                .await
                .expect("subscribe command")
                .expect("valid subscribe command");
            socket
                .send(Message::text(
                    json!({
                        "messageType": "I",
                        "data": {"subscriptionId": "upstream-secret"},
                        "response": {
                            "code": code,
                            "message": "rejected test-key for upstream-secret"
                        }
                    })
                    .to_string(),
                ))
                .await
                .expect("send rejected acknowledgement");
            let _ = socket.next().await;
        });
        let connector = TiingoConnector::with_endpoints(endpoint.clone(), endpoint)
            .expect("mock endpoint is valid");
        let registry = MarketDataRegistry::with_connector(Some("test-key".into()), connector);

        let error = registry
            .start(StartRequest {
                service: Service::Iex,
                symbols: vec!["AAPL".into()],
                threshold_level: None,
                confirm_iex_market_data_agreement: false,
            })
            .await
            .expect_err("rejected acknowledgement fails start");

        assert_eq!(error.payload().kind, expected_kind);
        let snapshots = format!("{error:?} {} {registry:?}", error.payload().message);
        assert!(!snapshots.contains("test-key"));
        assert!(!snapshots.contains("upstream-secret"));
        server.await.expect("mock server observes cleanup");
        registry.shutdown().await;
    }
}

#[tokio::test]
async fn registry_allows_eight_active_sessions_and_releases_slot_after_stop() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock WebSocket server");
    let endpoint = format!("ws://{}", listener.local_addr().expect("listener address"));
    let server = tokio::spawn(async move {
        let mut handlers = tokio::task::JoinSet::new();
        for index in 0..9 {
            let (stream, _) = listener.accept().await.expect("accept client");
            handlers.spawn(async move {
                let mut socket = accept_async(stream).await.expect("WebSocket handshake");
                socket
                    .next()
                    .await
                    .expect("subscribe command")
                    .expect("valid subscribe command");
                socket
                    .send(Message::text(
                        json!({
                            "messageType": "I",
                            "data": {"subscriptionId": index},
                            "response": {"code": 200, "message": "subscribed"}
                        })
                        .to_string(),
                    ))
                    .await
                    .expect("send acknowledgement");
                while let Some(message) = socket.next().await {
                    if matches!(message, Ok(Message::Close(_))) {
                        break;
                    }
                }
            });
        }
        while let Some(result) = handlers.join_next().await {
            result.expect("connection handler exits cleanly");
        }
    });
    let connector = TiingoConnector::with_endpoints(endpoint.clone(), endpoint)
        .expect("mock endpoint is valid");
    let registry = MarketDataRegistry::with_connector(Some("test-key".into()), connector);
    let request = || StartRequest {
        service: Service::Iex,
        symbols: vec!["AAPL".into()],
        threshold_level: None,
        confirm_iex_market_data_agreement: false,
    };
    let mut sessions = Vec::new();
    for _ in 0..8 {
        sessions.push(
            registry
                .start(request())
                .await
                .expect("session within cap starts"),
        );
    }

    let error = registry
        .start(request())
        .await
        .expect_err("ninth concurrent session is rejected");
    assert_eq!(error.payload().kind, "validation");
    registry
        .stop(&sessions[0].id)
        .await
        .expect("stopping a session releases its slot");
    let replacement = registry
        .start(request())
        .await
        .expect("a replacement session starts after stop");
    assert_eq!(replacement.state, SubscriptionStatus::Active);

    registry.shutdown().await;
    server
        .await
        .expect("all nine accepted sockets close cleanly");
}

#[tokio::test]
async fn terminal_session_tombstones_retain_only_the_eight_newest_sessions() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock WebSocket server");
    let endpoint = format!("ws://{}", listener.local_addr().expect("listener address"));
    let server = tokio::spawn(async move {
        for index in 0..10 {
            let (stream, _) = listener.accept().await.expect("accept client");
            let mut socket = accept_async(stream).await.expect("WebSocket handshake");
            socket
                .next()
                .await
                .expect("subscribe command")
                .expect("valid subscribe command");
            socket
                .send(Message::text(
                    json!({
                        "messageType": "I",
                        "data": {"subscriptionId": format!("terminal-{index}")},
                        "response": {"code": 200, "message": "subscribed"}
                    })
                    .to_string(),
                ))
                .await
                .expect("send acknowledgement");
            while let Some(message) = socket.next().await {
                if matches!(message, Ok(Message::Close(_))) {
                    break;
                }
            }
        }
    });
    let connector = TiingoConnector::with_endpoints(endpoint.clone(), endpoint)
        .expect("mock endpoint is valid");
    let registry = MarketDataRegistry::with_connector(Some("test-key".into()), connector);
    let mut session_ids = Vec::new();
    for index in 0..10 {
        let started = registry
            .start(StartRequest {
                service: Service::Iex,
                symbols: vec![format!("S{index}")],
                threshold_level: None,
                confirm_iex_market_data_agreement: false,
            })
            .await
            .expect("subscription starts");
        registry
            .stop(&started.id)
            .await
            .expect("subscription stops cleanly");
        session_ids.push(started.id);
    }
    server.await.expect("mock server exits cleanly");

    let mut oldest_are_evicted = true;
    for session_id in &session_ids[..2] {
        let error = registry.poll(session_id, 0).await;
        oldest_are_evicted &= error
            .as_ref()
            .is_err_and(|error| error.payload().kind == "validation");
    }
    let mut newest_are_retained = true;
    for session_id in &session_ids[2..] {
        newest_are_retained &= registry
            .poll(session_id, 0)
            .await
            .is_ok_and(|poll| poll.state == SubscriptionStatus::Stopped);
    }
    registry.shutdown().await;
    assert!(
        oldest_are_evicted,
        "a later start evicts the oldest terminal tombstone"
    );
    assert!(
        newest_are_retained,
        "the eight newest terminal sessions remain inspectable"
    );
}

#[tokio::test]
async fn shutdown_cancels_joins_and_clears_every_session_idempotently() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock WebSocket server");
    let endpoint = format!("ws://{}", listener.local_addr().expect("listener address"));
    let server = tokio::spawn(async move {
        let mut handlers = Vec::new();
        for index in 0..3 {
            let (stream, _) = listener.accept().await.expect("accept client");
            let mut socket = accept_async(stream).await.expect("WebSocket handshake");
            socket
                .next()
                .await
                .expect("subscribe command")
                .expect("valid subscribe command");
            socket
                .send(Message::text(
                    json!({
                        "messageType": "I",
                        "data": {"subscriptionId": 61 + index},
                        "response": {"code": 200, "message": "subscribed"}
                    })
                    .to_string(),
                ))
                .await
                .expect("send acknowledgement");
            handlers.push(tokio::spawn(async move {
                while let Some(message) = socket.next().await {
                    if matches!(message, Ok(Message::Close(_))) {
                        return true;
                    }
                }
                false
            }));
        }
        for handler in handlers {
            assert!(handler.await.expect("socket handler joins"));
        }
    });
    let connector = TiingoConnector::with_endpoints(endpoint.clone(), endpoint)
        .expect("mock endpoint is valid");
    let registry = MarketDataRegistry::with_connector(Some("test-key".into()), connector);
    let mut session_ids = Vec::new();
    for symbol in ["AAPL", "MSFT", "NVDA"] {
        session_ids.push(
            registry
                .start(StartRequest {
                    service: Service::Iex,
                    symbols: vec![symbol.into()],
                    threshold_level: None,
                    confirm_iex_market_data_agreement: false,
                })
                .await
                .expect("subscription starts")
                .id,
        );
    }

    registry.shutdown().await;
    registry.shutdown().await;
    server.await.expect("all mock sockets close");
    for session_id in session_ids {
        let error = registry
            .poll(&session_id, 0)
            .await
            .expect_err("shutdown clears session IDs");
        assert_eq!(error.payload().kind, "validation");
    }
    let error = registry
        .start(StartRequest {
            service: Service::Iex,
            symbols: vec!["SPY".into()],
            threshold_level: None,
            confirm_iex_market_data_agreement: false,
        })
        .await
        .expect_err("shutdown registry cannot restart");
    assert!(error.to_string().contains("shutting down"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancelling_shutdown_keeps_join_ownership_until_a_resumed_shutdown_completes() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock WebSocket server");
    let endpoint = format!("ws://{}", listener.local_addr().expect("listener address"));
    let (closed, socket_closed) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept client");
        let mut socket = accept_async(stream).await.expect("WebSocket handshake");
        socket
            .next()
            .await
            .expect("subscribe command")
            .expect("valid subscribe command");
        socket
            .send(Message::text(
                json!({
                    "messageType": "I",
                    "data": {"subscriptionId": "blocked-worker-id"},
                    "response": {"code": 200, "message": "subscribed"}
                })
                .to_string(),
            ))
            .await
            .expect("send acknowledgement");
        socket
            .send(Message::text(
                json!({
                    "messageType": "A",
                    "service": "iex",
                    "data": ["2026-08-25T14:00:00Z", "AAPL", 100.0]
                })
                .to_string(),
            ))
            .await
            .expect("send event that enters the receive clock");
        while let Some(message) = socket.next().await {
            if matches!(message, Ok(Message::Close(_))) {
                break;
            }
        }
        let _ = closed.send(());
    });
    let (entered, clock_entered) = tokio::sync::oneshot::channel();
    let release = Arc::new((StdMutex::new(false), Condvar::new()));
    let connector = TiingoConnector::with_endpoints(endpoint.clone(), endpoint)
        .expect("mock endpoint is valid");
    let registry = MarketDataRegistry::with_connector_and_clock(
        Some("test-key".into()),
        connector,
        Arc::new(BlockingClock {
            now: Utc::now(),
            entered: StdMutex::new(Some(entered)),
            release: Arc::clone(&release),
        }),
    );
    registry
        .start(StartRequest {
            service: Service::Iex,
            symbols: vec!["AAPL".into()],
            threshold_level: None,
            confirm_iex_market_data_agreement: false,
        })
        .await
        .expect("subscription starts");
    clock_entered
        .await
        .expect("worker blocks inside the receive clock");

    let first_registry = registry.clone();
    let mut first_shutdown = tokio::spawn(async move { first_registry.shutdown().await });
    assert!(
        tokio::time::timeout(Duration::from_millis(100), &mut first_shutdown)
            .await
            .is_err(),
        "the first shutdown is waiting to join the blocked worker"
    );
    first_shutdown.abort();
    assert!(
        first_shutdown
            .await
            .expect_err("shutdown is cancelled")
            .is_cancelled()
    );

    let resumed_registry = registry.clone();
    let mut resumed_shutdown = tokio::spawn(async move { resumed_registry.shutdown().await });
    let retained_join_ownership =
        tokio::time::timeout(Duration::from_millis(100), &mut resumed_shutdown)
            .await
            .is_err();

    let (released, wake) = &*release;
    *released.lock().expect("release lock is healthy") = true;
    wake.notify_all();
    if retained_join_ownership {
        resumed_shutdown.await.expect("resumed shutdown joins");
    }
    tokio::time::timeout(Duration::from_secs(1), socket_closed)
        .await
        .expect("joined worker closes its socket")
        .expect("server records socket close");
    server.await.expect("mock server exits cleanly");
    assert!(
        retained_join_ownership,
        "resumed shutdown retains responsibility for the still-live worker"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn acknowledged_start_cannot_publish_active_after_concurrent_shutdown_completes() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock WebSocket server");
    let endpoint = format!("ws://{}", listener.local_addr().expect("listener address"));
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept client");
        let mut socket = accept_async(stream).await.expect("WebSocket handshake");
        socket
            .next()
            .await
            .expect("subscribe command")
            .expect("valid subscribe command");
        socket
            .send(Message::text(
                json!({
                    "messageType": "I",
                    "data": {"subscriptionId": "acknowledged-before-shutdown"},
                    "response": {"code": 200, "message": "subscribed"}
                })
                .to_string(),
            ))
            .await
            .expect("send acknowledgement");
        socket
            .send(Message::text(
                json!({
                    "messageType": "A",
                    "service": "iex",
                    "data": ["2026-08-25T14:00:00Z", "AAPL", 100.0]
                })
                .to_string(),
            ))
            .await
            .expect("send event that holds the worker");
        while let Some(message) = socket.next().await {
            if matches!(message, Ok(Message::Close(_))) {
                break;
            }
        }
    });
    let (entered, clock_entered) = tokio::sync::oneshot::channel();
    let release = Arc::new((StdMutex::new(false), Condvar::new()));
    let connector = TiingoConnector::with_endpoints(endpoint.clone(), endpoint)
        .expect("mock endpoint is valid");
    let registry = MarketDataRegistry::with_connector_and_clock(
        Some("test-key".into()),
        connector,
        Arc::new(BlockingClock {
            now: Utc::now(),
            entered: StdMutex::new(Some(entered)),
            release: Arc::clone(&release),
        }),
    );
    let mut start = Box::pin(registry.start(StartRequest {
        service: Service::Iex,
        symbols: vec!["AAPL".into()],
        threshold_level: None,
        confirm_iex_market_data_agreement: false,
    }));
    std::future::poll_fn(|context| match start.as_mut().poll(context) {
        Poll::Pending => Poll::Ready(()),
        Poll::Ready(_) => panic!("start waits for the server acknowledgement"),
    })
    .await;
    clock_entered
        .await
        .expect("acknowledged worker reaches the blocking receive clock");

    let shutdown_registry = registry.clone();
    let mut shutdown = tokio::spawn(async move { shutdown_registry.shutdown().await });
    assert!(
        tokio::time::timeout(Duration::from_millis(100), &mut shutdown)
            .await
            .is_err(),
        "shutdown waits for the held acknowledged worker"
    );
    let (released, wake) = &*release;
    *released.lock().expect("release lock is healthy") = true;
    wake.notify_all();
    shutdown.await.expect("shutdown completes");
    server.await.expect("mock server exits cleanly");

    let error = start
        .await
        .expect_err("an acknowledged start cannot publish after shutdown");
    assert_eq!(error.payload().kind, "validation");
    assert!(error.to_string().contains("shutting down"));
}

#[tokio::test]
async fn start_times_out_at_five_seconds_and_owns_socket_cleanup() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock WebSocket server");
    let endpoint = format!("ws://{}", listener.local_addr().expect("listener address"));
    let (subscribed, subscribe_received) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept client");
        let mut socket = accept_async(stream).await.expect("WebSocket handshake");
        socket
            .next()
            .await
            .expect("subscribe command")
            .expect("valid subscribe command");
        subscribed.send(()).expect("record subscribe command");
        while let Some(message) = socket.next().await {
            if matches!(message, Ok(Message::Close(_))) {
                break;
            }
        }
    });
    let connector = TiingoConnector::with_endpoints(endpoint.clone(), endpoint)
        .expect("mock endpoint is valid");
    let registry = MarketDataRegistry::with_connector(Some("test-key".into()), connector);
    let start_registry = registry.clone();
    let start = tokio::spawn(async move {
        start_registry
            .start(StartRequest {
                service: Service::Iex,
                symbols: vec!["AAPL".into()],
                threshold_level: None,
                confirm_iex_market_data_agreement: false,
            })
            .await
    });
    subscribe_received.await.expect("worker sends subscribe");

    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(5) - Duration::from_nanos(1)).await;
    assert!(
        !start.is_finished(),
        "start remains pending before five seconds"
    );
    tokio::time::advance(Duration::from_nanos(1)).await;
    let error = start
        .await
        .expect("start task joins")
        .expect_err("missing acknowledgement times out");
    assert_eq!(error.payload().kind, "timeout");
    server.await.expect("timed-out start closes its socket");

    registry.shutdown().await;
}

#[tokio::test]
async fn cancelling_start_promptly_closes_and_joins_its_unacknowledged_worker() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock WebSocket server");
    let endpoint = format!("ws://{}", listener.local_addr().expect("listener address"));
    let (subscribed, first_subscribe_received) = tokio::sync::oneshot::channel();
    let (cleaned, first_worker_cleaned) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept cancelled client");
        let mut socket = accept_async(stream)
            .await
            .expect("cancelled WebSocket handshake");
        socket
            .next()
            .await
            .expect("cancelled subscribe")
            .expect("valid cancelled subscribe");
        subscribed.send(()).expect("record cancelled subscribe");
        while let Some(message) = socket.next().await {
            if matches!(message, Ok(Message::Close(_))) {
                break;
            }
        }
        cleaned.send(()).expect("record cancelled worker cleanup");

        let (stream, _) = listener.accept().await.expect("accept replacement client");
        let mut socket = accept_async(stream)
            .await
            .expect("replacement WebSocket handshake");
        socket
            .next()
            .await
            .expect("replacement subscribe")
            .expect("valid replacement subscribe");
        socket
            .send(Message::text(
                json!({
                    "messageType": "I",
                    "data": {"subscriptionId": 58},
                    "response": {"code": 200, "message": "subscribed"}
                })
                .to_string(),
            ))
            .await
            .expect("acknowledge replacement");
        while let Some(message) = socket.next().await {
            if matches!(message, Ok(Message::Close(_))) {
                break;
            }
        }
    });
    let connector = TiingoConnector::with_endpoints(endpoint.clone(), endpoint)
        .expect("mock endpoint is valid");
    let registry = MarketDataRegistry::with_connector(Some("test-key".into()), connector);
    let pending_registry = registry.clone();
    let pending = tokio::spawn(async move {
        pending_registry
            .start(StartRequest {
                service: Service::Iex,
                symbols: vec!["AAPL".into()],
                threshold_level: None,
                confirm_iex_market_data_agreement: false,
            })
            .await
    });
    first_subscribe_received
        .await
        .expect("mock sees cancelled subscribe");

    pending.abort();
    assert!(
        pending
            .await
            .expect_err("start task is cancelled")
            .is_cancelled()
    );
    tokio::time::timeout(Duration::from_secs(1), first_worker_cleaned)
        .await
        .expect("cancelled start cleans up promptly")
        .expect("cleanup signal arrives");

    let replacement = registry
        .start(StartRequest {
            service: Service::Iex,
            symbols: vec!["MSFT".into()],
            threshold_level: None,
            confirm_iex_market_data_agreement: false,
        })
        .await
        .expect("replacement starts after cleanup");
    registry
        .stop(&replacement.id)
        .await
        .expect("stop replacement");
    registry.shutdown().await;
    server.await.expect("mock server exits cleanly");
}

#[tokio::test]
async fn poll_wait_is_bounded_to_five_seconds_and_cancellation_keeps_session_alive() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock WebSocket server");
    let endpoint = format!("ws://{}", listener.local_addr().expect("listener address"));
    let (send_data, receive_data) = tokio::sync::oneshot::channel();
    let (data_processed, await_data_processed) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept client");
        let mut socket = accept_async(stream).await.expect("WebSocket handshake");
        socket
            .next()
            .await
            .expect("subscribe command")
            .expect("valid subscribe command");
        socket
            .send(Message::text(
                json!({
                    "messageType": "I",
                    "data": {"subscriptionId": 49},
                    "response": {"code": 200, "message": "subscribed"}
                })
                .to_string(),
            ))
            .await
            .expect("send acknowledgement");
        receive_data.await.expect("test requests market event");
        socket
            .send(Message::text(
                json!({
                    "messageType": "A",
                    "service": "iex",
                    "data": ["2026-08-25T14:00:00Z", "AAPL", 100.0]
                })
                .to_string(),
            ))
            .await
            .expect("send market event");
        let ping_payload = vec![49];
        socket
            .send(Message::Ping(ping_payload.clone().into()))
            .await
            .expect("send market event processing barrier");
        loop {
            match socket.next().await {
                Some(Ok(Message::Pong(payload))) if payload.as_ref() == ping_payload => break,
                Some(Ok(_)) => {}
                Some(Err(error)) => panic!("receive market event barrier: {error}"),
                None => panic!("market event barrier socket remains open"),
            }
        }
        data_processed
            .send(())
            .expect("record processed market event");
        while let Some(message) = socket.next().await {
            if matches!(message, Ok(Message::Close(_))) {
                break;
            }
        }
    });
    let connector = TiingoConnector::with_endpoints(endpoint.clone(), endpoint)
        .expect("mock endpoint is valid");
    let registry = MarketDataRegistry::with_connector(Some("test-key".into()), connector);
    let started = registry
        .start(StartRequest {
            service: Service::Iex,
            symbols: vec!["AAPL".into()],
            threshold_level: None,
            confirm_iex_market_data_agreement: false,
        })
        .await
        .expect("subscription starts");

    tokio::time::pause();
    let waiting_registry = registry.clone();
    let waiting_id = started.id.clone();
    let waiting = tokio::spawn(async move { waiting_registry.poll(&waiting_id, 0).await });
    tokio::time::advance(Duration::from_secs(5) - Duration::from_nanos(1)).await;
    assert!(
        !waiting.is_finished(),
        "poll remains pending before five seconds"
    );
    tokio::time::advance(Duration::from_nanos(1)).await;
    let empty = waiting
        .await
        .expect("poll task joins")
        .expect("bounded poll succeeds");
    assert!(empty.events.is_empty());
    assert_eq!(empty.state, SubscriptionStatus::Active);

    let cancelled_registry = registry.clone();
    let cancelled_id = started.id.clone();
    let cancelled = tokio::spawn(async move { cancelled_registry.poll(&cancelled_id, 0).await });
    tokio::task::yield_now().await;
    cancelled.abort();
    let _ = cancelled.await;
    tokio::time::resume();
    send_data.send(()).expect("request market event");
    await_data_processed
        .await
        .expect("worker processes market event");
    let live = registry
        .poll(&started.id, 0)
        .await
        .expect("session remains live");
    assert_eq!(live.state, SubscriptionStatus::Active);
    assert_eq!(live.events.len(), 1);

    registry.shutdown().await;
    server.await.expect("mock server exits cleanly");
}

#[tokio::test]
async fn update_preserves_market_data_that_arrives_before_its_acknowledgement() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock WebSocket server");
    let endpoint = format!("ws://{}", listener.local_addr().expect("listener address"));
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept client");
        let mut socket = accept_async(stream).await.expect("WebSocket handshake");
        socket
            .next()
            .await
            .expect("subscribe command")
            .expect("valid subscribe command");
        socket
            .send(Message::text(
                json!({
                    "messageType": "I",
                    "data": {"subscriptionId": 50},
                    "response": {"code": 200, "message": "subscribed"}
                })
                .to_string(),
            ))
            .await
            .expect("send initial acknowledgement");
        socket
            .next()
            .await
            .expect("update command")
            .expect("valid update command");
        socket
            .send(Message::text(
                json!({
                    "messageType": "A",
                    "service": "iex",
                    "data": ["2026-08-25T14:00:00Z", "AAPL", 100.0]
                })
                .to_string(),
            ))
            .await
            .expect("send interleaved market event");
        socket
            .send(Message::text(
                json!({
                    "messageType": "I",
                    "data": {"subscriptionId": 51},
                    "response": {"code": 200, "message": "updated"}
                })
                .to_string(),
            ))
            .await
            .expect("send update acknowledgement");
        while let Some(message) = socket.next().await {
            if matches!(message, Ok(Message::Close(_))) {
                break;
            }
        }
    });
    let connector = TiingoConnector::with_endpoints(endpoint.clone(), endpoint)
        .expect("mock endpoint is valid");
    let registry = MarketDataRegistry::with_connector(Some("test-key".into()), connector);
    let started = registry
        .start(StartRequest {
            service: Service::Iex,
            symbols: vec!["AAPL".into()],
            threshold_level: None,
            confirm_iex_market_data_agreement: false,
        })
        .await
        .expect("subscription starts");

    let updated = registry
        .update(
            &started.id,
            UpdateRequest {
                add_symbols: vec!["MSFT".into()],
                remove_symbols: Vec::new(),
                threshold_level: None,
            },
        )
        .await
        .expect("interleaved market data does not break update ack");
    assert_eq!(updated.symbols, vec!["AAPL", "MSFT"]);
    let poll = registry
        .poll(&started.id, 0)
        .await
        .expect("market event was retained");
    assert_eq!(poll.events.len(), 1);
    assert_eq!(poll.events[0].symbol.as_deref(), Some("AAPL"));

    registry.shutdown().await;
    server.await.expect("mock server exits cleanly");
}

#[tokio::test]
async fn concurrent_poll_waits_for_update_ack_before_publishing_interleaved_data() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock WebSocket server");
    let endpoint = format!("ws://{}", listener.local_addr().expect("listener address"));
    let (event_sent, interleaved_event_sent) = tokio::sync::oneshot::channel();
    let (release_ack, ack_released) = tokio::sync::oneshot::channel();
    let (second_update_seen, second_update_received) = tokio::sync::oneshot::channel();
    let (release_second_ack, second_ack_released) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept client");
        let mut socket = accept_async(stream).await.expect("WebSocket handshake");
        socket
            .next()
            .await
            .expect("subscribe command")
            .expect("valid subscribe command");
        socket
            .send(Message::text(
                json!({
                    "messageType": "I",
                    "data": {"subscriptionId": "initial-upstream-id"},
                    "response": {"code": 200, "message": "subscribed"}
                })
                .to_string(),
            ))
            .await
            .expect("send initial acknowledgement");
        socket
            .next()
            .await
            .expect("update command")
            .expect("valid update command");
        socket
            .send(Message::text(
                json!({
                    "messageType": "U",
                    "futureId": "updated-upstream-secret",
                    "nested": {
                        "updated-upstream-secret": "updated-upstream-secret"
                    }
                })
                .to_string(),
            ))
            .await
            .expect("send interleaved event containing the future acknowledgement ID");
        event_sent
            .send(())
            .expect("signal interleaved event was sent");
        ack_released.await.expect("release update acknowledgement");
        socket
            .send(Message::text(
                json!({
                    "messageType": "I",
                    "data": {"subscriptionId": "updated-upstream-secret"},
                    "response": {"code": 200, "message": "updated"}
                })
                .to_string(),
            ))
            .await
            .expect("send update acknowledgement");
        socket
            .next()
            .await
            .expect("second update command")
            .expect("valid second update command");
        second_update_seen
            .send(())
            .expect("signal second update command was received");
        second_ack_released
            .await
            .expect("release second update acknowledgement");
        socket
            .send(Message::text(
                json!({
                    "messageType": "I",
                    "data": {"subscriptionId": "final-upstream-id"},
                    "response": {"code": 200, "message": "updated"}
                })
                .to_string(),
            ))
            .await
            .expect("send second update acknowledgement");
        while let Some(message) = socket.next().await {
            if matches!(message, Ok(Message::Close(_))) {
                break;
            }
        }
    });
    let connector = TiingoConnector::with_endpoints(endpoint.clone(), endpoint)
        .expect("mock endpoint is valid");
    let registry = MarketDataRegistry::with_connector(Some("test-key".into()), connector);
    let started = registry
        .start(StartRequest {
            service: Service::Iex,
            symbols: vec!["AAPL".into()],
            threshold_level: None,
            confirm_iex_market_data_agreement: false,
        })
        .await
        .expect("subscription starts");
    let update_registry = registry.clone();
    let update_id = started.id.clone();
    let update = tokio::spawn(async move {
        update_registry
            .update(
                &update_id,
                UpdateRequest {
                    add_symbols: vec!["MSFT".into()],
                    remove_symbols: vec!["AAPL".into()],
                    threshold_level: None,
                },
            )
            .await
    });
    interleaved_event_sent
        .await
        .expect("server sent interleaved event");
    let poll_registry = registry.clone();
    let poll_id = started.id.clone();
    let mut poll = tokio::spawn(async move { poll_registry.poll(&poll_id, 0).await });

    assert!(
        tokio::time::timeout(Duration::from_millis(500), &mut poll)
            .await
            .is_err(),
        "interleaved data remains private until its acknowledgement ID is known"
    );
    release_ack
        .send(())
        .expect("release update acknowledgement");
    second_update_received
        .await
        .expect("worker starts the second acknowledged mutation");
    assert!(
        !update.is_finished(),
        "the update remains pending while its second acknowledgement is held"
    );
    let poll = tokio::time::timeout(Duration::from_secs(1), poll)
        .await
        .expect("poll returns while the update is still pending")
        .expect("poll task joins")
        .expect("poll succeeds after acknowledgement");
    let serialized = serde_json::to_string(&poll).expect("serialize poll result");
    assert_eq!(poll.events.len(), 1);
    assert!(!serialized.contains("updated-upstream-secret"));
    assert!(
        !update.is_finished(),
        "sanitized data is poll-visible before the full update completes"
    );
    release_second_ack
        .send(())
        .expect("release second update acknowledgement");
    update
        .await
        .expect("update task joins")
        .expect("update succeeds");

    registry.shutdown().await;
    server.await.expect("mock server exits cleanly");
}

#[tokio::test]
async fn private_update_buffer_shares_the_queue_event_bound_without_dropping_prior_data() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock WebSocket server");
    let endpoint = format!("ws://{}", listener.local_addr().expect("listener address"));
    let (prior_polled, release_update) = tokio::sync::oneshot::channel();
    let (burst_sent, burst_finished) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept client");
        let mut socket = accept_async(stream).await.expect("WebSocket handshake");
        socket
            .next()
            .await
            .expect("subscribe command")
            .expect("valid subscribe command");
        socket
            .send(Message::text(
                json!({
                    "messageType": "I",
                    "data": {"subscriptionId": "initial-id"},
                    "response": {"code": 200, "message": "subscribed"}
                })
                .to_string(),
            ))
            .await
            .expect("send initial acknowledgement");
        socket
            .send(Message::text(
                json!({
                    "messageType": "U",
                    "prior": "retained"
                })
                .to_string(),
            ))
            .await
            .expect("send prior retained event");
        release_update.await.expect("prior event was polled");
        socket
            .next()
            .await
            .expect("update command")
            .expect("valid update command");
        for index in 0..2048 {
            if socket
                .send(Message::text(
                    json!({
                        "messageType": "U",
                        "buffered": index
                    })
                    .to_string(),
                ))
                .await
                .is_err()
            {
                break;
            }
        }
        let _ = burst_sent.send(());
        while let Some(message) = socket.next().await {
            if matches!(message, Ok(Message::Close(_))) {
                return true;
            }
        }
        true
    });
    let connector = TiingoConnector::with_endpoints(endpoint.clone(), endpoint)
        .expect("mock endpoint is valid");
    let registry = MarketDataRegistry::with_connector(Some("test-key".into()), connector);
    let started = registry
        .start(StartRequest {
            service: Service::Iex,
            symbols: vec!["AAPL".into()],
            threshold_level: None,
            confirm_iex_market_data_agreement: false,
        })
        .await
        .expect("subscription starts");
    let prior = registry
        .poll(&started.id, 0)
        .await
        .expect("prior event is poll-visible");
    let prior_bytes = serde_json::to_vec(&prior.events[0]).expect("serialize prior event");
    let prior_sequence = prior.events[0].sequence;
    prior_polled.send(()).expect("release update burst");
    let update_registry = registry.clone();
    let update_id = started.id.clone();
    let mut update = tokio::spawn(async move {
        update_registry
            .update(
                &update_id,
                UpdateRequest {
                    add_symbols: vec!["MSFT".into()],
                    remove_symbols: Vec::new(),
                    threshold_level: None,
                },
            )
            .await
    });
    let poll_registry = registry.clone();
    let poll_id = started.id.clone();
    let terminal_poll =
        tokio::spawn(async move { poll_registry.poll(&poll_id, prior_sequence).await });
    burst_finished.await.expect("server sent buffered burst");
    let error = tokio::time::timeout(Duration::from_secs(1), &mut update)
        .await
        .expect("private buffer reaches data_gap without waiting for an acknowledgement")
        .expect("update task joins")
        .expect_err("private buffer overflow rejects the update");
    assert_eq!(error.payload().kind, "websocket_protocol");
    let first_terminal = terminal_poll
        .await
        .expect("terminal poll task joins")
        .expect("concurrent terminal poll succeeds");
    let replayed_terminal = registry
        .poll(&started.id, prior_sequence)
        .await
        .expect("identical terminal cursor replays");
    assert_eq!(first_terminal.state, SubscriptionStatus::DataGap);
    assert_eq!(
        serde_json::to_vec(&first_terminal).expect("serialize first terminal poll"),
        serde_json::to_vec(&replayed_terminal).expect("serialize terminal replay")
    );
    let terminal = registry
        .poll(&started.id, 0)
        .await
        .expect("prior terminal queue remains inspectable");
    assert_eq!(terminal.state, SubscriptionStatus::DataGap);
    assert_eq!(terminal.events.len(), 1);
    assert_eq!(
        serde_json::to_vec(&terminal.events[0]).expect("serialize retained prior event"),
        prior_bytes
    );
    assert!(server.await.expect("mock server exits after data gap"));
    registry.shutdown().await;
}

#[tokio::test]
async fn data_received_while_waiting_for_update_ack_refreshes_liveness() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock WebSocket server");
    let endpoint = format!("ws://{}", listener.local_addr().expect("listener address"));
    let (update_seen, update_received) = tokio::sync::oneshot::channel();
    let (messages_sent, messages_received) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept client");
        let mut socket = accept_async(stream).await.expect("WebSocket handshake");
        socket
            .next()
            .await
            .expect("subscribe command")
            .expect("valid subscribe command");
        socket
            .send(Message::text(
                json!({
                    "messageType": "I",
                    "data": {"subscriptionId": 64},
                    "response": {"code": 200, "message": "subscribed"}
                })
                .to_string(),
            ))
            .await
            .expect("send initial acknowledgement");
        socket
            .next()
            .await
            .expect("update command")
            .expect("valid update command");
        update_seen.send(()).expect("record update command");
        socket
            .send(Message::text(
                json!({
                    "messageType": "A",
                    "service": "iex",
                    "data": ["2026-08-25T14:00:00Z", "AAPL", 100.0]
                })
                .to_string(),
            ))
            .await
            .expect("send interleaved market event");
        socket
            .send(Message::text(
                json!({
                    "messageType": "I",
                    "data": {"subscriptionId": 65},
                    "response": {"code": 200, "message": "updated"}
                })
                .to_string(),
            ))
            .await
            .expect("send update acknowledgement");
        messages_sent
            .send(())
            .expect("signal interleaved data and acknowledgement were sent");
        while let Some(message) = socket.next().await {
            if matches!(message, Ok(Message::Close(_))) {
                return;
            }
        }
    });
    let connector = TiingoConnector::with_endpoints(endpoint.clone(), endpoint)
        .expect("mock endpoint is valid");
    let (delay_requests, mut requested_delays) = tokio::sync::mpsc::unbounded_channel();
    let registry = MarketDataRegistry::with_connector_and_clocks(
        Some("test-key".into()),
        connector,
        Arc::new(FixedClock(Utc::now())),
        Arc::new(ManualReconnectClock {
            requests: delay_requests,
        }),
    );
    let started = registry
        .start(StartRequest {
            service: Service::Iex,
            symbols: vec!["AAPL".into()],
            threshold_level: None,
            confirm_iex_market_data_agreement: false,
        })
        .await
        .expect("subscription starts");

    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(74)).await;
    tokio::time::resume();
    let update_registry = registry.clone();
    let update_id = started.id.clone();
    let update = tokio::spawn(async move {
        update_registry
            .update(
                &update_id,
                UpdateRequest {
                    add_symbols: vec!["MSFT".into()],
                    remove_symbols: vec![],
                    threshold_level: None,
                },
            )
            .await
    });
    update_received.await.expect("server sees update command");
    messages_received
        .await
        .expect("server sends interleaved data and acknowledgement");
    let updated = update
        .await
        .expect("update task joins")
        .expect("update succeeds after interleaved data");
    tokio::time::pause();
    assert_eq!(updated.symbols, vec!["AAPL", "MSFT"]);
    let poll = registry
        .poll(&started.id, 0)
        .await
        .expect("interleaved event is retained");
    assert_eq!(poll.events.len(), 1);

    tokio::time::advance(Duration::from_secs(74)).await;
    assert!(
        requested_delays.try_recv().is_err(),
        "interleaved data establishes a fresh 75-second deadline"
    );
    tokio::time::advance(Duration::from_secs(1)).await;
    tokio::time::advance(Duration::from_millis(1)).await;
    let (delay, _) = requested_delays
        .recv()
        .await
        .expect("liveness failure enters reconnect backoff");
    assert_eq!(delay, Duration::from_millis(250));
    registry
        .stop(&started.id)
        .await
        .expect("stop cancels reconnect");
    registry.shutdown().await;
    server.await.expect("mock server exits cleanly");
}

#[tokio::test]
async fn transport_loss_during_update_preserves_buffered_data_through_reconnect() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock WebSocket server");
    let endpoint = format!("ws://{}", listener.local_addr().expect("listener address"));
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept initial client");
        let mut socket = accept_async(stream)
            .await
            .expect("initial WebSocket handshake");
        socket
            .next()
            .await
            .expect("initial subscribe")
            .expect("valid initial subscribe");
        socket
            .send(Message::text(
                json!({
                    "messageType": "I",
                    "data": {"subscriptionId": 66},
                    "response": {"code": 200, "message": "subscribed"}
                })
                .to_string(),
            ))
            .await
            .expect("send initial acknowledgement");
        socket
            .next()
            .await
            .expect("update command")
            .expect("valid update command");
        socket
            .send(Message::text(
                json!({
                    "messageType": "U",
                    "phase": "before disconnect",
                    "value": 123
                })
                .to_string(),
            ))
            .await
            .expect("send market data before transport loss");
        socket
            .send(Message::Close(None))
            .await
            .expect("disconnect before update acknowledgement");
        drop(socket);

        let (stream, _) = listener.accept().await.expect("accept reconnect");
        let mut socket = accept_async(stream)
            .await
            .expect("reconnect WebSocket handshake");
        let fresh_subscribe = socket
            .next()
            .await
            .expect("fresh subscribe")
            .expect("valid fresh subscribe")
            .into_text()
            .expect("text subscribe");
        let fresh_subscribe: serde_json::Value =
            serde_json::from_str(&fresh_subscribe).expect("fresh subscribe JSON");
        assert_eq!(fresh_subscribe["eventName"], "subscribe");
        assert_eq!(fresh_subscribe["eventData"]["tickers"], json!(["AAPL"]));
        assert!(fresh_subscribe["eventData"].get("subscriptionId").is_none());
        socket
            .send(Message::text(
                json!({
                    "messageType": "I",
                    "data": {"subscriptionId": 67},
                    "response": {"code": 200, "message": "subscribed"}
                })
                .to_string(),
            ))
            .await
            .expect("send reconnect acknowledgement");
        socket
            .send(Message::text(
                json!({
                    "messageType": "A",
                    "service": "iex",
                    "data": ["2026-08-25T14:00:01Z", "AAPL", 101.0]
                })
                .to_string(),
            ))
            .await
            .expect("send post-reconnect market event");
        while let Some(message) = socket.next().await {
            if matches!(message, Ok(Message::Close(_))) {
                break;
            }
        }
    });
    let connector = TiingoConnector::with_endpoints(endpoint.clone(), endpoint)
        .expect("mock endpoint is valid");
    let (delay_requests, mut requested_delays) = tokio::sync::mpsc::unbounded_channel();
    let registry = MarketDataRegistry::with_connector_and_clocks(
        Some("test-key".into()),
        connector,
        Arc::new(FixedClock(Utc::now())),
        Arc::new(ManualReconnectClock {
            requests: delay_requests,
        }),
    );
    let started = registry
        .start(StartRequest {
            service: Service::Iex,
            symbols: vec!["AAPL".into()],
            threshold_level: None,
            confirm_iex_market_data_agreement: false,
        })
        .await
        .expect("subscription starts");

    let error = registry
        .update(
            &started.id,
            UpdateRequest {
                add_symbols: vec!["MSFT".into()],
                remove_symbols: vec![],
                threshold_level: None,
            },
        )
        .await
        .expect_err("unacknowledged update reports transport failure");
    assert_eq!(error.payload().kind, "transport");
    let (delay, release) = tokio::time::timeout(Duration::from_secs(1), requested_delays.recv())
        .await
        .expect("transport failure enters reconnect promptly")
        .expect("worker requests reconnect delay");
    assert_eq!(delay, Duration::from_millis(250));
    let buffered = registry
        .poll(&started.id, 0)
        .await
        .expect("pre-disconnect data is available during recovery");
    assert_eq!(buffered.events.len(), 1);
    assert_eq!(buffered.events[0].payload["phase"], "before disconnect");
    release.send(()).expect("release reconnect delay");
    let recovered = registry
        .poll(&started.id, buffered.events[0].sequence)
        .await
        .expect("post-reconnect data is available");
    assert_eq!(recovered.state, SubscriptionStatus::Active);
    assert_eq!(recovered.events.len(), 1);
    assert_eq!(recovered.events[0].symbol.as_deref(), Some("AAPL"));

    registry.shutdown().await;
    server.await.expect("mock server exits cleanly");
}

#[tokio::test]
async fn acknowledged_remove_survives_add_failure_and_reconnect() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock WebSocket server");
    let endpoint = format!("ws://{}", listener.local_addr().expect("listener address"));
    let (reconnect_symbols, receive_reconnect_symbols) = tokio::sync::oneshot::channel();
    let (reconnect_ready, await_reconnect_ready) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept initial client");
        let mut socket = accept_async(stream)
            .await
            .expect("initial WebSocket handshake");
        socket
            .next()
            .await
            .expect("initial subscribe")
            .expect("valid initial subscribe");
        socket
            .send(Message::text(
                json!({
                    "messageType": "I",
                    "data": {"subscriptionId": "partial-initial-id"},
                    "response": {"code": 200, "message": "subscribed"}
                })
                .to_string(),
            ))
            .await
            .expect("send initial acknowledgement");

        let remove = socket
            .next()
            .await
            .expect("remove command")
            .expect("valid remove command")
            .into_text()
            .expect("text remove command");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&remove).expect("remove JSON"),
            json!({
                "eventName": "unsubscribe",
                "authorization": "test-key",
                "eventData": {
                    "subscriptionId": "partial-initial-id",
                    "tickers": ["AAPL"]
                }
            })
        );
        socket
            .send(Message::text(
                json!({
                    "messageType": "I",
                    "data": {"subscriptionId": "partial-remove-id"},
                    "response": {"code": 200, "message": "updated"}
                })
                .to_string(),
            ))
            .await
            .expect("acknowledge remove");

        let add = socket
            .next()
            .await
            .expect("add command")
            .expect("valid add command")
            .into_text()
            .expect("text add command");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&add).expect("add JSON"),
            json!({
                "eventName": "subscribe",
                "authorization": "test-key",
                "eventData": {"subscriptionId": "partial-remove-id", "tickers": ["NVDA"]}
            })
        );
        socket
            .send(Message::Close(None))
            .await
            .expect("disconnect before add acknowledgement");
        drop(socket);

        let (stream, _) = listener.accept().await.expect("accept reconnect");
        let mut socket = accept_async(stream)
            .await
            .expect("reconnect WebSocket handshake");
        let fresh_subscribe = socket
            .next()
            .await
            .expect("fresh subscribe")
            .expect("valid fresh subscribe")
            .into_text()
            .expect("text fresh subscribe");
        let fresh_subscribe: serde_json::Value =
            serde_json::from_str(&fresh_subscribe).expect("fresh subscribe JSON");
        reconnect_symbols
            .send(fresh_subscribe["eventData"]["tickers"].clone())
            .expect("record reconnect symbols");
        socket
            .send(Message::text(
                json!({
                    "messageType": "I",
                    "data": {"subscriptionId": "partial-reconnect-id"},
                    "response": {"code": 200, "message": "subscribed"}
                })
                .to_string(),
            ))
            .await
            .expect("acknowledge reconnect");
        let ping_payload = vec![0x72];
        socket
            .send(Message::Ping(ping_payload.clone().into()))
            .await
            .expect("send reconnect processing barrier");
        loop {
            match socket.next().await {
                Some(Ok(Message::Pong(payload))) if payload.as_ref() == ping_payload => break,
                Some(Ok(_)) => {}
                Some(Err(error)) => panic!("receive reconnect barrier: {error}"),
                None => panic!("reconnect barrier socket remains open"),
            }
        }
        reconnect_ready.send(()).expect("record reconnect ready");

        let add = socket
            .next()
            .await
            .expect("post-reconnect add command")
            .expect("valid post-reconnect add command")
            .into_text()
            .expect("text post-reconnect add command");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&add).expect("post-reconnect add JSON"),
            json!({
                "eventName": "subscribe",
                "authorization": "test-key",
                "eventData": {
                    "subscriptionId": "partial-reconnect-id",
                    "tickers": ["AAPL"]
                }
            })
        );
        socket
            .send(Message::text(
                json!({
                    "messageType": "I",
                    "data": {"subscriptionId": "partial-restored-id"},
                    "response": {"code": 200, "message": "updated"}
                })
                .to_string(),
            ))
            .await
            .expect("acknowledge post-reconnect add");
        while let Some(message) = socket.next().await {
            if matches!(message, Ok(Message::Close(_))) {
                break;
            }
        }
    });
    let connector = TiingoConnector::with_endpoints(endpoint.clone(), endpoint)
        .expect("mock endpoint is valid");
    let (delay_requests, mut requested_delays) = tokio::sync::mpsc::unbounded_channel();
    let registry = MarketDataRegistry::with_connector_and_clocks(
        Some("test-key".into()),
        connector,
        Arc::new(FixedClock(Utc::now())),
        Arc::new(ManualReconnectClock {
            requests: delay_requests,
        }),
    );
    let started = registry
        .start(StartRequest {
            service: Service::Iex,
            symbols: vec!["AAPL".into(), "MSFT".into()],
            threshold_level: None,
            confirm_iex_market_data_agreement: false,
        })
        .await
        .expect("subscription starts");

    let error = registry
        .update(
            &started.id,
            UpdateRequest {
                add_symbols: vec!["NVDA".into()],
                remove_symbols: vec!["AAPL".into()],
                threshold_level: None,
            },
        )
        .await
        .expect_err("unacknowledged add reports transport failure");
    let error = serde_json::to_value(error.payload()).expect("serialize partial update error");
    assert_eq!(error["kind"], "transport");
    assert_eq!(error["appliedSymbols"], json!(["MSFT"]));
    let (delay, release) = requested_delays
        .recv()
        .await
        .expect("worker requests reconnect delay");
    assert_eq!(delay, Duration::from_millis(250));
    release.send(()).expect("release reconnect delay");
    let reconnect_symbols = receive_reconnect_symbols
        .await
        .expect("reconnect symbol inventory is recorded");
    assert_eq!(reconnect_symbols, json!(["MSFT"]));
    await_reconnect_ready
        .await
        .expect("reconnect acknowledgement is processed");

    let updated = registry
        .update(
            &started.id,
            UpdateRequest {
                add_symbols: vec!["AAPL".into()],
                remove_symbols: vec![],
                threshold_level: None,
            },
        )
        .await
        .expect("removed symbol can be added again after reconnect");
    assert_eq!(updated.symbols, vec!["MSFT", "AAPL"]);

    registry.shutdown().await;
    server.await.expect("mock server exits cleanly");
}

#[tokio::test]
async fn sole_symbol_remove_ack_with_add_failure_never_reconnects_empty() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock WebSocket server");
    let endpoint = format!("ws://{}", listener.local_addr().expect("listener address"));
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept initial client");
        let mut socket = accept_async(stream)
            .await
            .expect("initial WebSocket handshake");
        socket
            .next()
            .await
            .expect("initial subscribe")
            .expect("valid initial subscribe");
        socket
            .send(Message::text(
                json!({
                    "messageType": "I",
                    "data": {"subscriptionId": "sole-initial-id"},
                    "response": {"code": 200, "message": "subscribed"}
                })
                .to_string(),
            ))
            .await
            .expect("send initial acknowledgement");

        let remove = socket
            .next()
            .await
            .expect("remove command")
            .expect("valid remove command")
            .into_text()
            .expect("text remove command");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&remove).expect("remove JSON"),
            json!({
                "eventName": "unsubscribe",
                "authorization": "test-key",
                "eventData": {
                    "subscriptionId": "sole-initial-id",
                    "tickers": ["AAPL"]
                }
            })
        );
        socket
            .send(Message::text(
                json!({
                    "messageType": "I",
                    "data": {"subscriptionId": "sole-remove-id"},
                    "response": {"code": 200, "message": "updated"}
                })
                .to_string(),
            ))
            .await
            .expect("acknowledge sole-symbol remove");

        let add = socket
            .next()
            .await
            .expect("add command")
            .expect("valid add command")
            .into_text()
            .expect("text add command");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&add).expect("add JSON"),
            json!({
                "eventName": "subscribe",
                "authorization": "test-key",
                "eventData": {"subscriptionId": "sole-remove-id", "tickers": ["MSFT"]}
            })
        );
        socket
            .send(Message::Close(None))
            .await
            .expect("disconnect before replacement add acknowledgement");
    });
    let connector = TiingoConnector::with_endpoints(endpoint.clone(), endpoint)
        .expect("mock endpoint is valid");
    let (delay_requests, mut requested_delays) = tokio::sync::mpsc::unbounded_channel();
    let registry = MarketDataRegistry::with_connector_and_clocks(
        Some("test-key".into()),
        connector,
        Arc::new(FixedClock(Utc::now())),
        Arc::new(ManualReconnectClock {
            requests: delay_requests,
        }),
    );
    let started = registry
        .start(StartRequest {
            service: Service::Iex,
            symbols: vec!["AAPL".into()],
            threshold_level: None,
            confirm_iex_market_data_agreement: false,
        })
        .await
        .expect("subscription starts");

    let error = registry
        .update(
            &started.id,
            UpdateRequest {
                add_symbols: vec!["MSFT".into()],
                remove_symbols: vec!["AAPL".into()],
                threshold_level: None,
            },
        )
        .await
        .expect_err("unacknowledged replacement reports transport failure");
    let error =
        serde_json::to_value(error.payload()).expect("serialize empty partial update error");
    assert_eq!(error["kind"], "transport");
    assert_eq!(error["appliedSymbols"], json!([]));
    tokio::task::yield_now().await;
    assert!(
        requested_delays.try_recv().is_err(),
        "an empty acknowledged inventory must not enter reconnect backoff"
    );

    let terminal = tokio::time::timeout(Duration::from_secs(1), registry.poll(&started.id, 0))
        .await
        .expect("empty partial state becomes terminal promptly")
        .expect("terminal state remains inspectable");
    assert_eq!(terminal.state, SubscriptionStatus::Failed);
    assert_eq!(terminal.terminal_error, Some(TerminalErrorKind::Transport));
    assert!(terminal.events.is_empty());

    registry.shutdown().await;
    server.await.expect("mock server exits cleanly");
}

#[tokio::test]
async fn update_acknowledgement_is_bounded_to_five_seconds_and_closes_uncertain_state() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock WebSocket server");
    let endpoint = format!("ws://{}", listener.local_addr().expect("listener address"));
    let (updated, update_received) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept client");
        let mut socket = accept_async(stream).await.expect("WebSocket handshake");
        socket
            .next()
            .await
            .expect("subscribe command")
            .expect("valid subscribe command");
        socket
            .send(Message::text(
                json!({
                    "messageType": "I",
                    "data": {"subscriptionId": 52},
                    "response": {"code": 200, "message": "subscribed"}
                })
                .to_string(),
            ))
            .await
            .expect("send initial acknowledgement");
        socket
            .next()
            .await
            .expect("update command")
            .expect("valid update command");
        updated.send(()).expect("record update command");
        while let Some(message) = socket.next().await {
            if matches!(message, Ok(Message::Close(_))) {
                break;
            }
        }
    });
    let connector = TiingoConnector::with_endpoints(endpoint.clone(), endpoint)
        .expect("mock endpoint is valid");
    let registry = MarketDataRegistry::with_connector(Some("test-key".into()), connector);
    let started = registry
        .start(StartRequest {
            service: Service::Iex,
            symbols: vec!["AAPL".into()],
            threshold_level: None,
            confirm_iex_market_data_agreement: false,
        })
        .await
        .expect("subscription starts");
    let update_registry = registry.clone();
    let update_id = started.id.clone();
    let update = tokio::spawn(async move {
        update_registry
            .update(
                &update_id,
                UpdateRequest {
                    add_symbols: vec!["MSFT".into()],
                    remove_symbols: Vec::new(),
                    threshold_level: None,
                },
            )
            .await
    });
    update_received.await.expect("worker sends update");

    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(5) - Duration::from_nanos(1)).await;
    assert!(
        !update.is_finished(),
        "update remains pending before five seconds"
    );
    tokio::time::advance(Duration::from_nanos(1)).await;
    let error = update
        .await
        .expect("update task joins")
        .expect_err("missing update acknowledgement times out");
    assert_eq!(error.payload().kind, "timeout");
    server.await.expect("uncertain update closes its socket");
    let failed = registry
        .poll(&started.id, u64::MAX)
        .await
        .expect("failed state remains inspectable");
    assert_eq!(failed.state, SubscriptionStatus::Failed);

    registry.shutdown().await;
}

#[tokio::test]
async fn shutdown_cancels_inflight_update_io_before_the_acknowledgement_deadline() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock WebSocket server");
    let endpoint = format!("ws://{}", listener.local_addr().expect("listener address"));
    let (update_seen, update_received) = tokio::sync::oneshot::channel();
    let (release_ack, ack_released) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept client");
        let mut socket = accept_async(stream).await.expect("WebSocket handshake");
        socket
            .next()
            .await
            .expect("subscribe command")
            .expect("valid subscribe command");
        socket
            .send(Message::text(
                json!({
                    "messageType": "I",
                    "data": {"subscriptionId": "update-cancel-id"},
                    "response": {"code": 200, "message": "subscribed"}
                })
                .to_string(),
            ))
            .await
            .expect("send initial acknowledgement");
        socket
            .next()
            .await
            .expect("update command")
            .expect("valid update command");
        update_seen.send(()).expect("record update command");
        let _ = ack_released.await;
        let _ = socket
            .send(Message::text(
                json!({
                    "messageType": "I",
                    "data": {"subscriptionId": "late-update-id"},
                    "response": {"code": 200, "message": "updated"}
                })
                .to_string(),
            ))
            .await;
        while let Some(message) = socket.next().await {
            if matches!(message, Ok(Message::Close(_))) {
                break;
            }
        }
    });
    let connector = TiingoConnector::with_endpoints(endpoint.clone(), endpoint)
        .expect("mock endpoint is valid");
    let registry = MarketDataRegistry::with_connector(Some("test-key".into()), connector);
    let started = registry
        .start(StartRequest {
            service: Service::Iex,
            symbols: vec!["AAPL".into()],
            threshold_level: None,
            confirm_iex_market_data_agreement: false,
        })
        .await
        .expect("subscription starts");
    let update_registry = registry.clone();
    let update_id = started.id.clone();
    let update = tokio::spawn(async move {
        update_registry
            .update(
                &update_id,
                UpdateRequest {
                    add_symbols: vec!["MSFT".into()],
                    remove_symbols: vec![],
                    threshold_level: None,
                },
            )
            .await
    });
    update_received.await.expect("server sees update command");

    let shutdown_registry = registry.clone();
    let mut shutdown = tokio::spawn(async move { shutdown_registry.shutdown().await });
    let shutdown_was_prompt = tokio::time::timeout(Duration::from_millis(500), &mut shutdown)
        .await
        .is_ok();
    let _ = release_ack.send(());
    if !shutdown_was_prompt {
        shutdown
            .await
            .expect("shutdown completes after test cleanup");
    }
    let _ = update.await.expect("update task joins");
    server.await.expect("mock server exits cleanly");
    assert!(
        shutdown_was_prompt,
        "shutdown cancellation interrupts update I/O before its five-second deadline"
    );
}

#[tokio::test]
async fn stop_cancels_inflight_update_io_before_the_acknowledgement_deadline() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock WebSocket server");
    let endpoint = format!("ws://{}", listener.local_addr().expect("listener address"));
    let (update_seen, update_received) = tokio::sync::oneshot::channel();
    let (release_ack, ack_released) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept client");
        let mut socket = accept_async(stream).await.expect("WebSocket handshake");
        socket
            .next()
            .await
            .expect("subscribe command")
            .expect("valid subscribe command");
        socket
            .send(Message::text(
                json!({
                    "messageType": "I",
                    "data": {"subscriptionId": "stop-update-id"},
                    "response": {"code": 200, "message": "subscribed"}
                })
                .to_string(),
            ))
            .await
            .expect("send initial acknowledgement");
        socket
            .next()
            .await
            .expect("remove command")
            .expect("valid remove command");
        socket
            .send(Message::text(
                json!({
                    "messageType": "I",
                    "data": {"subscriptionId": "stop-remove-id"},
                    "response": {"code": 200, "message": "updated"}
                })
                .to_string(),
            ))
            .await
            .expect("acknowledge remove");
        socket
            .next()
            .await
            .expect("add command")
            .expect("valid add command");
        update_seen.send(()).expect("record add command");
        let _ = ack_released.await;
        let _ = socket
            .send(Message::text(
                json!({
                    "messageType": "I",
                    "data": {"subscriptionId": "late-stop-update-id"},
                    "response": {"code": 200, "message": "updated"}
                })
                .to_string(),
            ))
            .await;
        while let Some(message) = socket.next().await {
            if matches!(message, Ok(Message::Close(_))) {
                break;
            }
        }
    });
    let connector = TiingoConnector::with_endpoints(endpoint.clone(), endpoint)
        .expect("mock endpoint is valid");
    let registry = MarketDataRegistry::with_connector(Some("test-key".into()), connector);
    let started = registry
        .start(StartRequest {
            service: Service::Iex,
            symbols: vec!["AAPL".into(), "MSFT".into()],
            threshold_level: None,
            confirm_iex_market_data_agreement: false,
        })
        .await
        .expect("subscription starts");
    let update_registry = registry.clone();
    let update_id = started.id.clone();
    let update = tokio::spawn(async move {
        update_registry
            .update(
                &update_id,
                UpdateRequest {
                    add_symbols: vec!["NVDA".into()],
                    remove_symbols: vec!["AAPL".into()],
                    threshold_level: None,
                },
            )
            .await
    });
    update_received.await.expect("server sees update command");

    let stop_registry = registry.clone();
    let stop_id = started.id.clone();
    let mut stop = tokio::spawn(async move { stop_registry.stop(&stop_id).await });
    let stop_was_prompt = tokio::time::timeout(Duration::from_millis(500), &mut stop)
        .await
        .is_ok();
    let _ = release_ack.send(());
    let stopped = if stop_was_prompt {
        None
    } else {
        Some(stop.await.expect("stop task joins after test cleanup"))
    };
    if let Some(stopped) = stopped {
        stopped.expect("stop succeeds after test cleanup");
    }
    let error = update
        .await
        .expect("update task joins")
        .expect_err("stop interrupts the pending add");
    let error = serde_json::to_value(error.payload()).expect("serialize cancelled update error");
    assert_eq!(error["kind"], "transport");
    assert_eq!(error["appliedSymbols"], json!(["MSFT"]));
    server.await.expect("mock server exits cleanly");
    registry.shutdown().await;
    assert!(
        stop_was_prompt,
        "explicit stop interrupts update I/O before its five-second deadline"
    );
}

#[tokio::test]
async fn retained_vendor_payload_and_debug_snapshots_redact_the_api_key() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock WebSocket server");
    let endpoint = format!("ws://{}", listener.local_addr().expect("listener address"));
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept client");
        let mut socket = accept_async(stream).await.expect("WebSocket handshake");
        socket
            .next()
            .await
            .expect("subscribe command")
            .expect("valid subscribe command");
        socket
            .send(Message::text(
                json!({
                    "messageType": "I",
                    "data": {"subscriptionId": 53},
                    "response": {"code": 200, "message": "subscribed"}
                })
                .to_string(),
            ))
            .await
            .expect("send acknowledgement");
        socket
            .send(Message::text(
                json!({
                    "messageType": "U",
                    "data": {
                        "subscriptionId": "upstream-secret",
                        "test-key-field": "vendor field",
                        "note": "vendor echoed test-key inside a message"
                    }
                })
                .to_string(),
            ))
            .await
            .expect("send raw vendor update");
        while let Some(message) = socket.next().await {
            if matches!(message, Ok(Message::Close(_))) {
                break;
            }
        }
    });
    let connector = TiingoConnector::with_endpoints(endpoint.clone(), endpoint)
        .expect("mock endpoint is valid");
    let registry = MarketDataRegistry::with_connector(Some("test-key".into()), connector);
    let started = registry
        .start(StartRequest {
            service: Service::Iex,
            symbols: vec!["AAPL".into()],
            threshold_level: None,
            confirm_iex_market_data_agreement: false,
        })
        .await
        .expect("subscription starts");
    let poll = registry
        .poll(&started.id, 0)
        .await
        .expect("raw update is retained");

    let serialized = serde_json::to_string(&poll).expect("serialize poll");
    let debug = format!("{poll:?} {registry:?}");
    assert!(!serialized.contains("test-key"));
    assert!(!serialized.contains("upstream-secret"));
    assert!(!debug.contains("test-key"));
    assert_eq!(
        poll.events[0].payload["data"]["note"],
        "vendor echoed [REDACTED] inside a message"
    );
    assert_eq!(
        poll.events[0].payload["data"]["subscriptionId"],
        "[REDACTED]"
    );

    let secret_endpoint = TiingoConnector::with_endpoints(
        "ws://127.0.0.1:1/test-key",
        "ws://127.0.0.1:1/upstream-secret",
    )
    .expect("credential-bearing test endpoints parse");
    let connector_debug = format!("{secret_endpoint:?}");
    assert!(!connector_debug.contains("test-key"));
    assert!(!connector_debug.contains("upstream-secret"));

    registry.shutdown().await;
    server.await.expect("mock server exits cleanly");
}

#[tokio::test]
async fn retained_events_redact_every_acknowledged_upstream_id_from_all_fields() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock WebSocket server");
    let endpoint = format!("ws://{}", listener.local_addr().expect("listener address"));
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept client");
        let mut socket = accept_async(stream).await.expect("WebSocket handshake");
        socket
            .next()
            .await
            .expect("subscribe command")
            .expect("valid subscribe command");
        socket
            .send(Message::text(
                json!({
                    "messageType": "I",
                    "data": {"subscriptionId": "initial-upstream-secret"},
                    "response": {"code": 200, "message": "subscribed"}
                })
                .to_string(),
            ))
            .await
            .expect("send initial acknowledgement");
        socket
            .next()
            .await
            .expect("update command")
            .expect("valid update command");
        let mut event = json!({
            "messageType": "A",
            "service": "iex",
            "data": ["initial-upstream-secret", "updated-upstream-secret", 100.0],
            "nested": {
                "updated-upstream-secret": "test-key plus initial-upstream-secret"
            }
        });
        event.as_object_mut().expect("event is an object").insert(
            "prefix-initial-upstream-secret-suffix".into(),
            json!(["updated-upstream-secret", "test-key"]),
        );
        socket
            .send(Message::text(event.to_string()))
            .await
            .expect("send secret-bearing market event before acknowledgement");
        socket
            .send(Message::text(
                json!({
                    "messageType": "I",
                    "data": {"subscriptionId": "updated-upstream-secret"},
                    "response": {"code": 200, "message": "updated"}
                })
                .to_string(),
            ))
            .await
            .expect("send update acknowledgement");
        while let Some(message) = socket.next().await {
            if matches!(message, Ok(Message::Close(_))) {
                break;
            }
        }
    });
    let connector = TiingoConnector::with_endpoints(endpoint.clone(), endpoint)
        .expect("mock endpoint is valid");
    let registry = MarketDataRegistry::with_connector(Some("test-key".into()), connector);
    let started = registry
        .start(StartRequest {
            service: Service::Iex,
            symbols: vec!["AAPL".into()],
            threshold_level: None,
            confirm_iex_market_data_agreement: false,
        })
        .await
        .expect("subscription starts");
    registry
        .update(
            &started.id,
            UpdateRequest {
                add_symbols: vec!["MSFT".into()],
                remove_symbols: vec![],
                threshold_level: None,
            },
        )
        .await
        .expect("subscription update succeeds");
    let poll = registry
        .poll(&started.id, 0)
        .await
        .expect("secret-bearing event is retained");

    let serialized = serde_json::to_string(&poll).expect("serialize poll result");
    let debug = format!("{poll:?} {registry:?}");
    for secret in [
        "test-key",
        "initial-upstream-secret",
        "updated-upstream-secret",
    ] {
        assert!(
            !serialized.contains(secret),
            "serialized event leaked {secret}"
        );
        assert!(!debug.contains(secret), "debug snapshot leaked {secret}");
    }
    assert_eq!(
        poll.events[0].vendor_timestamp.as_deref(),
        Some("[REDACTED]")
    );
    assert_eq!(poll.events[0].symbol.as_deref(), Some("[REDACTED]"));
    assert_eq!(poll.events[0].payload["data"][0], "[REDACTED]");
    assert_eq!(poll.events[0].payload["data"][1], "[REDACTED]");

    registry.shutdown().await;
    server.await.expect("mock server exits cleanly");
}

#[tokio::test]
async fn retained_events_redact_numeric_acknowledged_upstream_ids() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock WebSocket server");
    let endpoint = format!("ws://{}", listener.local_addr().expect("listener address"));
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept client");
        let mut socket = accept_async(stream).await.expect("WebSocket handshake");
        socket
            .next()
            .await
            .expect("subscribe command")
            .expect("valid subscribe command");
        socket
            .send(Message::text(
                json!({
                    "messageType": "I",
                    "data": {"subscriptionId": 987654321},
                    "response": {"code": 200, "message": "subscribed"}
                })
                .to_string(),
            ))
            .await
            .expect("send numeric acknowledgement");
        socket
            .send(Message::text(
                json!({
                    "messageType": "U",
                    "numericId": 987654321,
                    "numericIdFloat": 987654321.0,
                    "numericCredential": 123456789,
                    "id-987654321": "echo 987654321"
                })
                .to_string(),
            ))
            .await
            .expect("send numeric-ID-bearing event");
        while let Some(message) = socket.next().await {
            if matches!(message, Ok(Message::Close(_))) {
                break;
            }
        }
    });
    let connector = TiingoConnector::with_endpoints(endpoint.clone(), endpoint)
        .expect("mock endpoint is valid");
    let registry = MarketDataRegistry::with_connector(Some("123456789".into()), connector);
    let started = registry
        .start(StartRequest {
            service: Service::Iex,
            symbols: vec!["AAPL".into()],
            threshold_level: None,
            confirm_iex_market_data_agreement: false,
        })
        .await
        .expect("subscription starts");
    let poll = registry
        .poll(&started.id, 0)
        .await
        .expect("numeric-ID-bearing event is retained");

    let serialized = serde_json::to_string(&poll).expect("serialize poll result");
    assert!(!serialized.contains("987654321"));
    assert!(!serialized.contains("123456789"));
    assert_eq!(poll.events[0].payload["numericId"], "[REDACTED]");
    assert_eq!(poll.events[0].payload["numericIdFloat"], "[REDACTED]");
    assert_eq!(poll.events[0].payload["numericCredential"], "[REDACTED]");

    registry.shutdown().await;
    server.await.expect("mock server exits cleanly");
}

#[tokio::test]
async fn validated_market_array_preserves_numeric_prices_and_sizes_equal_to_upstream_id() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock WebSocket server");
    let endpoint = format!("ws://{}", listener.local_addr().expect("listener address"));
    let market_data = json!([
        "T",
        "2026-08-25T14:00:00Z",
        1_777_123_200_000_000_000_i64,
        "AAPL",
        5,
        5.0,
        5.0,
        5.0,
        5,
        5.0,
        5,
        0,
        0,
        0,
        5,
        5
    ]);
    let expected_market_data = market_data.clone();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept client");
        let mut socket = accept_async(stream).await.expect("WebSocket handshake");
        socket
            .next()
            .await
            .expect("subscribe command")
            .expect("valid subscribe command");
        socket
            .send(Message::text(
                json!({
                    "messageType": "I",
                    "data": {"subscriptionId": 5},
                    "response": {"code": 200, "message": "subscribed"}
                })
                .to_string(),
            ))
            .await
            .expect("send acknowledgement");
        socket
            .send(Message::text(
                json!({
                    "messageType": "A",
                    "service": "iex",
                    "data": market_data,
                    "subscriptionId": 5,
                    "nested": {"subscriptionId": 5},
                    "textualEcho": "subscription-5-active"
                })
                .to_string(),
            ))
            .await
            .expect("send validated market event");
        while let Some(message) = socket.next().await {
            if matches!(message, Ok(Message::Close(_))) {
                break;
            }
        }
    });
    let connector = TiingoConnector::with_endpoints(endpoint.clone(), endpoint)
        .expect("mock endpoint is valid");
    let registry = MarketDataRegistry::with_connector(Some("test-key".into()), connector);
    let started = registry
        .start(StartRequest {
            service: Service::Iex,
            symbols: vec!["AAPL".into()],
            threshold_level: Some(5),
            confirm_iex_market_data_agreement: true,
        })
        .await
        .expect("subscription starts");
    let poll = registry
        .poll(&started.id, 0)
        .await
        .expect("validated market event is retained");

    assert_eq!(poll.events[0].payload["data"], expected_market_data);
    assert_eq!(poll.events[0].payload["subscriptionId"], "[REDACTED]");
    assert_eq!(
        poll.events[0].payload["nested"]["subscriptionId"],
        "[REDACTED]"
    );
    assert_eq!(
        poll.events[0].payload["textualEcho"],
        "subscription-[REDACTED]-active"
    );

    registry.shutdown().await;
    server.await.expect("mock server exits cleanly");
}

#[tokio::test]
async fn string_upstream_id_redaction_preserves_adjacent_market_text() {
    let event = poll_single_raw_event(
        json!("ABC5"),
        json!({
            "messageType": "U",
            "exact": "ABC5",
            "bounded": "prefix-ABC5-suffix",
            "adjacent": "XABC5Y",
            "longerNumber": "ABC50",
            "safeABC5value": "unchanged",
            "prefix-ABC5-suffix": "redacted key"
        }),
    )
    .await;

    assert_eq!(event.payload["exact"], "[REDACTED]");
    assert_eq!(event.payload["bounded"], "prefix-[REDACTED]-suffix");
    assert_eq!(event.payload["adjacent"], "XABC5Y");
    assert_eq!(event.payload["longerNumber"], "ABC50");
    assert_eq!(event.payload["safeABC5value"], "unchanged");
    assert_eq!(event.payload["prefix-[REDACTED]-suffix"], "redacted key");
}

#[tokio::test]
async fn numeric_upstream_id_redaction_preserves_unrelated_market_values() {
    let event = poll_single_raw_event(
        json!(5),
        json!({
            "messageType": "U",
            "exactInteger": 5,
            "exactFloat": 5.0,
            "marketInteger": 105,
            "marketFloat": 105.5,
            "timestamp": "2026-08-25T14:05:00Z",
            "adjacentText": "105",
            "boundedText": "subscription-5-active"
        }),
    )
    .await;

    assert_eq!(event.payload["exactInteger"], "[REDACTED]");
    assert_eq!(event.payload["exactFloat"], "[REDACTED]");
    assert_eq!(event.payload["marketInteger"], 105);
    assert_eq!(event.payload["marketFloat"], 105.5);
    assert_eq!(event.payload["timestamp"], "2026-08-25T14:05:00Z");
    assert_eq!(event.payload["adjacentText"], "105");
    assert_eq!(
        event.payload["boundedText"],
        "subscription-[REDACTED]-active"
    );
}

#[tokio::test]
async fn large_numeric_upstream_id_redaction_distinguishes_exact_forms_from_adjacent_u64() {
    let event = poll_single_raw_event(
        json!(9_007_199_254_740_992_u64),
        json!({
            "messageType": "U",
            "exactInteger": 9_007_199_254_740_992_u64,
            "exactFloat": 9_007_199_254_740_992_f64,
            "adjacentInteger": 9_007_199_254_740_993_u64,
            "exactText": "9007199254740992",
            "boundedText": "subscription-9007199254740992-active",
            "adjacentText": "9007199254740993"
        }),
    )
    .await;

    assert_eq!(event.payload["exactInteger"], "[REDACTED]");
    assert_eq!(event.payload["exactFloat"], "[REDACTED]");
    assert_eq!(event.payload["adjacentInteger"], 9_007_199_254_740_993_u64);
    assert_eq!(event.payload["exactText"], "[REDACTED]");
    assert_eq!(
        event.payload["boundedText"],
        "subscription-[REDACTED]-active"
    );
    assert_eq!(event.payload["adjacentText"], "9007199254740993");
}

#[tokio::test]
async fn a_single_event_too_large_for_any_poll_response_is_terminal_data_gap() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock WebSocket server");
    let endpoint = format!("ws://{}", listener.local_addr().expect("listener address"));
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept client");
        let mut socket = accept_async(stream).await.expect("WebSocket handshake");
        socket
            .next()
            .await
            .expect("subscribe command")
            .expect("valid subscribe command");
        socket
            .send(Message::text(
                json!({
                    "messageType": "I",
                    "data": {"subscriptionId": 54},
                    "response": {"code": 200, "message": "subscribed"}
                })
                .to_string(),
            ))
            .await
            .expect("send acknowledgement");
        socket
            .send(Message::text(
                json!({
                    "messageType": "A",
                    "service": "iex",
                    "data": ["2026-08-25T14:00:00Z", "AAPL", 100.0],
                    "padding": "x".repeat(1_100_000)
                })
                .to_string(),
            ))
            .await
            .expect("send oversized market message");
        while let Some(message) = socket.next().await {
            if matches!(message, Ok(Message::Close(_))) {
                return true;
            }
        }
        false
    });
    let connector = TiingoConnector::with_endpoints(endpoint.clone(), endpoint)
        .expect("mock endpoint is valid");
    let registry = MarketDataRegistry::with_connector(Some("test-key".into()), connector);
    let started = registry
        .start(StartRequest {
            service: Service::Iex,
            symbols: vec!["AAPL".into()],
            threshold_level: None,
            confirm_iex_market_data_agreement: false,
        })
        .await
        .expect("subscription starts");

    let terminal =
        tokio::time::timeout(Duration::from_secs(1), registry.poll(&started.id, u64::MAX))
            .await
            .expect("unreturnable event becomes terminal promptly")
            .expect("terminal state remains inspectable");
    assert_eq!(terminal.state, SubscriptionStatus::DataGap);
    assert!(terminal.events.is_empty());
    assert!(server.await.expect("mock server exits"));
    registry.shutdown().await;
}

#[tokio::test]
async fn exact_active_poll_limit_is_rejected_when_terminal_wrapper_would_exceed_it() {
    let received_at = DateTime::parse_from_rfc3339("2026-08-25T14:00:00Z")
        .expect("fixed timestamp parses")
        .with_timezone(&Utc);
    let event_without_padding = MarketDataEvent {
        sequence: 1,
        received_at,
        vendor_timestamp: Some("2026-08-25T14:00:00Z".into()),
        symbol: Some("AAPL".into()),
        duplicate: false,
        out_of_order: false,
        payload: json!({
            "messageType": "A",
            "service": "iex",
            "data": ["2026-08-25T14:00:00Z", "AAPL", 100.0],
            "padding": ""
        }),
    };
    let active_without_padding = serde_json::to_vec(&PollResult {
        id: "0".repeat(32),
        state: SubscriptionStatus::Active,
        terminal_error: None,
        events: vec![event_without_padding.clone()],
    })
    .expect("serialize exact-bound fixture")
    .len();
    let padding = "x".repeat(1024 * 1024 - active_without_padding);
    let mut exact_event = event_without_padding;
    exact_event.payload["padding"] = json!(padding);
    assert_eq!(
        serde_json::to_vec(&PollResult {
            id: "0".repeat(32),
            state: SubscriptionStatus::Active,
            terminal_error: None,
            events: vec![exact_event.clone()],
        })
        .expect("serialize active boundary")
        .len(),
        1024 * 1024
    );
    assert_eq!(
        serde_json::to_vec(&PollResult {
            id: "0".repeat(32),
            state: SubscriptionStatus::DataGap,
            terminal_error: None,
            events: vec![exact_event],
        })
        .expect("serialize terminal boundary")
        .len(),
        1024 * 1024 + 2
    );

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock WebSocket server");
    let endpoint = format!("ws://{}", listener.local_addr().expect("listener address"));
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept client");
        let mut socket = accept_async(stream).await.expect("WebSocket handshake");
        socket
            .next()
            .await
            .expect("subscribe command")
            .expect("valid subscribe command");
        socket
            .send(Message::text(
                json!({
                    "messageType": "I",
                    "data": {"subscriptionId": "terminal-boundary-id"},
                    "response": {"code": 200, "message": "subscribed"}
                })
                .to_string(),
            ))
            .await
            .expect("send acknowledgement");
        socket
            .send(Message::text(
                json!({
                    "messageType": "A",
                    "service": "iex",
                    "data": ["2026-08-25T14:00:00Z", "AAPL", 100.0],
                    "padding": padding
                })
                .to_string(),
            ))
            .await
            .expect("send exact-bound market message");
        while let Some(message) = socket.next().await {
            if matches!(message, Ok(Message::Close(_))) {
                return true;
            }
        }
        false
    });
    let connector = TiingoConnector::with_endpoints(endpoint.clone(), endpoint)
        .expect("mock endpoint is valid");
    let registry = MarketDataRegistry::with_connector_and_clock(
        Some("test-key".into()),
        connector,
        Arc::new(FixedClock(received_at)),
    );
    let started = registry
        .start(StartRequest {
            service: Service::Iex,
            symbols: vec!["AAPL".into()],
            threshold_level: None,
            confirm_iex_market_data_agreement: false,
        })
        .await
        .expect("subscription starts");

    let terminal =
        match tokio::time::timeout(Duration::from_secs(1), registry.poll(&started.id, u64::MAX))
            .await
        {
            Ok(terminal) => terminal.expect("terminal state remains inspectable"),
            Err(_) => {
                registry.shutdown().await;
                server.await.expect("mock server exits after cleanup");
                panic!("worst-case admission rejects the event promptly");
            }
        };
    assert_eq!(terminal.state, SubscriptionStatus::DataGap);
    assert!(terminal.events.is_empty());
    assert!(server.await.expect("mock server observes terminal close"));
    registry.shutdown().await;
}

#[tokio::test]
async fn redaction_growth_data_gap_preserves_prior_exact_cursor_replay() {
    let received_at = DateTime::parse_from_rfc3339("2026-08-25T14:00:00Z")
        .expect("fixed timestamp parses")
        .with_timezone(&Utc);
    let redacted_without_padding = MarketDataEvent {
        sequence: 2,
        received_at,
        vendor_timestamp: None,
        symbol: None,
        duplicate: false,
        out_of_order: false,
        payload: json!({
            "messageType": "U",
            "upstream": "[REDACTED]",
            "padding": ""
        }),
    };
    let redacted_base_bytes = serde_json::to_vec(&PollResult {
        id: "0".repeat(32),
        state: SubscriptionStatus::Reconnecting,
        terminal_error: None,
        events: vec![redacted_without_padding.clone()],
    })
    .expect("serialize redacted boundary fixture")
    .len();
    let padding = "x".repeat(1024 * 1024 + 1 - redacted_base_bytes);
    let mut redacted_event = redacted_without_padding;
    redacted_event.payload["padding"] = json!(padding.clone());
    assert_eq!(
        serde_json::to_vec(&PollResult {
            id: "0".repeat(32),
            state: SubscriptionStatus::Reconnecting,
            terminal_error: None,
            events: vec![redacted_event.clone()],
        })
        .expect("serialize redacted over-bound fixture")
        .len(),
        1024 * 1024 + 1
    );
    let mut unredacted_event = redacted_event;
    unredacted_event.payload["upstream"] = json!("id");
    assert!(
        serde_json::to_vec(&PollResult {
            id: "0".repeat(32),
            state: SubscriptionStatus::Reconnecting,
            terminal_error: None,
            events: vec![unredacted_event],
        })
        .expect("serialize unredacted boundary fixture")
        .len()
            <= 1024 * 1024
    );

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock WebSocket server");
    let endpoint = format!("ws://{}", listener.local_addr().expect("listener address"));
    let (prior_polled, release_update) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept client");
        let mut socket = accept_async(stream).await.expect("WebSocket handshake");
        socket
            .next()
            .await
            .expect("subscribe command")
            .expect("valid subscribe command");
        socket
            .send(Message::text(
                json!({
                    "messageType": "I",
                    "data": {"subscriptionId": "initial-id"},
                    "response": {"code": 200, "message": "subscribed"}
                })
                .to_string(),
            ))
            .await
            .expect("send initial acknowledgement");
        socket
            .send(Message::text(
                json!({
                    "messageType": "U",
                    "prior": "retained"
                })
                .to_string(),
            ))
            .await
            .expect("send prior retained event");
        release_update.await.expect("prior event was polled");
        socket
            .next()
            .await
            .expect("update command")
            .expect("valid update command");
        socket
            .send(Message::text(
                json!({
                    "messageType": "U",
                    "upstream": "id",
                    "padding": padding
                })
                .to_string(),
            ))
            .await
            .expect("send exact-bound interleaved event");
        socket
            .send(Message::text(
                json!({
                    "messageType": "I",
                    "data": {"subscriptionId": "id"},
                    "response": {"code": 200, "message": "updated"}
                })
                .to_string(),
            ))
            .await
            .expect("send update acknowledgement");
        while let Some(message) = socket.next().await {
            if matches!(message, Ok(Message::Close(_))) {
                return true;
            }
        }
        false
    });
    let connector = TiingoConnector::with_endpoints(endpoint.clone(), endpoint)
        .expect("mock endpoint is valid");
    let registry = MarketDataRegistry::with_connector_and_clock(
        Some("test-key".into()),
        connector,
        Arc::new(FixedClock(received_at)),
    );
    let started = registry
        .start(StartRequest {
            service: Service::Iex,
            symbols: vec!["AAPL".into()],
            threshold_level: None,
            confirm_iex_market_data_agreement: false,
        })
        .await
        .expect("subscription starts");
    let prior = registry
        .poll(&started.id, 0)
        .await
        .expect("prior event is poll-visible");
    let prior_bytes = serde_json::to_vec(&prior.events[0]).expect("serialize prior event");
    prior_polled.send(()).expect("release exact-bound update");
    let error = registry
        .update(
            &started.id,
            UpdateRequest {
                add_symbols: vec!["MSFT".into()],
                remove_symbols: Vec::new(),
                threshold_level: None,
            },
        )
        .await
        .expect_err("redaction growth rejects the new event");
    assert_eq!(error.payload().kind, "websocket_protocol");
    let terminal = registry
        .poll(&started.id, 0)
        .await
        .expect("terminal queue remains inspectable");
    assert_eq!(terminal.state, SubscriptionStatus::DataGap);
    assert_eq!(terminal.events.len(), 1);
    assert_eq!(
        serde_json::to_vec(&terminal.events[0]).expect("serialize retained terminal event"),
        prior_bytes
    );
    assert_eq!(terminal.events[0].payload["prior"], "retained");
    assert!(server.await.expect("mock server exits after data gap"));
    registry.shutdown().await;
}

#[tokio::test]
async fn entitlement_rejection_after_activation_is_terminal_sanitized_and_not_retried() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock WebSocket server");
    let endpoint = format!("ws://{}", listener.local_addr().expect("listener address"));
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept client");
        let mut socket = accept_async(stream).await.expect("WebSocket handshake");
        socket
            .next()
            .await
            .expect("subscribe command")
            .expect("valid subscribe command");
        socket
            .send(Message::text(
                json!({
                    "messageType": "I",
                    "data": {"subscriptionId": 55},
                    "response": {"code": 200, "message": "subscribed"}
                })
                .to_string(),
            ))
            .await
            .expect("send acknowledgement");
        socket
            .send(Message::text(
                json!({
                    "messageType": "E",
                    "response": {
                        "message": "not entitled to test-key; upstream id is upstream-secret"
                    }
                })
                .to_string(),
            ))
            .await
            .expect("send entitlement rejection");
        while let Some(message) = socket.next().await {
            if matches!(message, Ok(Message::Close(_))) {
                return true;
            }
        }
        false
    });
    let connector = TiingoConnector::with_endpoints(endpoint.clone(), endpoint)
        .expect("mock endpoint is valid");
    let (delay_requests, mut requested_delays) = tokio::sync::mpsc::unbounded_channel();
    let registry = MarketDataRegistry::with_connector_and_clocks(
        Some("test-key".into()),
        connector,
        Arc::new(FixedClock(Utc::now())),
        Arc::new(ManualReconnectClock {
            requests: delay_requests,
        }),
    );
    let started = registry
        .start(StartRequest {
            service: Service::Iex,
            symbols: vec!["AAPL".into()],
            threshold_level: None,
            confirm_iex_market_data_agreement: false,
        })
        .await
        .expect("subscription starts");

    let terminal =
        tokio::time::timeout(Duration::from_secs(1), registry.poll(&started.id, u64::MAX))
            .await
            .expect("entitlement rejection becomes terminal promptly")
            .expect("terminal state remains inspectable");
    assert_eq!(terminal.state, SubscriptionStatus::Failed);
    assert_eq!(
        terminal.terminal_error,
        Some(TerminalErrorKind::Entitlement)
    );
    let snapshots = format!("{terminal:?} {registry:?}");
    assert!(!snapshots.contains("test-key"));
    assert!(!snapshots.contains("upstream-secret"));
    assert!(requested_delays.try_recv().is_err());
    assert!(server.await.expect("mock server exits"));
    let stopped = registry
        .stop(&started.id)
        .await
        .expect("terminal subscription cleanup succeeds");
    assert_eq!(stopped.state, SubscriptionStatus::Stopped);
    let retained = registry
        .poll_with_bounds(&started.id, u64::MAX, 1, Duration::ZERO)
        .await
        .expect("terminal tombstone remains inspectable after stop");
    assert_eq!(retained.state, SubscriptionStatus::Failed);
    assert_eq!(
        retained.terminal_error,
        Some(TerminalErrorKind::Entitlement)
    );
    registry.shutdown().await;
}

#[tokio::test]
async fn authentication_rejection_after_activation_retains_only_sanitized_classification() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock WebSocket server");
    let endpoint = format!("ws://{}", listener.local_addr().expect("listener address"));
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept client");
        let mut socket = accept_async(stream).await.expect("WebSocket handshake");
        socket
            .next()
            .await
            .expect("subscribe command")
            .expect("valid subscribe command");
        socket
            .send(Message::text(
                json!({
                    "messageType": "I",
                    "data": {"subscriptionId": "auth-upstream-secret"},
                    "response": {"code": 200, "message": "subscribed"}
                })
                .to_string(),
            ))
            .await
            .expect("send acknowledgement");
        socket
            .send(Message::text(
                json!({
                    "messageType": "I",
                    "data": {"subscriptionId": "auth-upstream-secret"},
                    "response": {
                        "code": 401,
                        "message": "invalid test-key and auth-upstream-secret"
                    }
                })
                .to_string(),
            ))
            .await
            .expect("send authentication rejection");
        while let Some(message) = socket.next().await {
            if matches!(message, Ok(Message::Close(_))) {
                break;
            }
        }
    });
    let connector = TiingoConnector::with_endpoints(endpoint.clone(), endpoint)
        .expect("mock endpoint is valid");
    let registry = MarketDataRegistry::with_connector(Some("test-key".into()), connector);
    let started = registry
        .start(StartRequest {
            service: Service::Iex,
            symbols: vec!["AAPL".into()],
            threshold_level: None,
            confirm_iex_market_data_agreement: false,
        })
        .await
        .expect("subscription starts");

    let terminal =
        tokio::time::timeout(Duration::from_secs(1), registry.poll(&started.id, u64::MAX))
            .await
            .expect("authentication rejection becomes terminal promptly")
            .expect("terminal state remains inspectable");
    assert_eq!(terminal.state, SubscriptionStatus::Failed);
    assert_eq!(
        terminal.terminal_error,
        Some(TerminalErrorKind::Authentication)
    );
    let serialized = serde_json::to_string(&terminal).expect("serialize terminal poll");
    assert!(!serialized.contains("test-key"));
    assert!(!serialized.contains("auth-upstream-secret"));

    server.await.expect("mock server exits");
    registry.shutdown().await;
}

#[tokio::test]
async fn malformed_binary_message_is_terminal_leak_free_and_not_retried() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock WebSocket server");
    let endpoint = format!("ws://{}", listener.local_addr().expect("listener address"));
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept client");
        let mut socket = accept_async(stream).await.expect("WebSocket handshake");
        socket
            .next()
            .await
            .expect("subscribe command")
            .expect("valid subscribe command");
        socket
            .send(Message::text(
                json!({
                    "messageType": "I",
                    "data": {"subscriptionId": 56},
                    "response": {"code": 200, "message": "subscribed"}
                })
                .to_string(),
            ))
            .await
            .expect("send acknowledgement");
        socket
            .send(Message::binary(
                b"malformed test-key upstream-secret".to_vec(),
            ))
            .await
            .expect("send malformed binary message");
        while let Some(message) = socket.next().await {
            if matches!(message, Ok(Message::Close(_))) {
                return true;
            }
        }
        false
    });
    let connector = TiingoConnector::with_endpoints(endpoint.clone(), endpoint)
        .expect("mock endpoint is valid");
    let (delay_requests, mut requested_delays) = tokio::sync::mpsc::unbounded_channel();
    let registry = MarketDataRegistry::with_connector_and_clocks(
        Some("test-key".into()),
        connector,
        Arc::new(FixedClock(Utc::now())),
        Arc::new(ManualReconnectClock {
            requests: delay_requests,
        }),
    );
    let started = registry
        .start(StartRequest {
            service: Service::Iex,
            symbols: vec!["AAPL".into()],
            threshold_level: None,
            confirm_iex_market_data_agreement: false,
        })
        .await
        .expect("subscription starts");

    let terminal =
        tokio::time::timeout(Duration::from_secs(1), registry.poll(&started.id, u64::MAX))
            .await
            .expect("malformed message becomes terminal promptly")
            .expect("terminal state remains inspectable");
    assert_eq!(terminal.state, SubscriptionStatus::Failed);
    assert_eq!(terminal.terminal_error, Some(TerminalErrorKind::Protocol));
    let snapshots = format!("{terminal:?} {registry:?}");
    assert!(!snapshots.contains("test-key"));
    assert!(!snapshots.contains("upstream-secret"));
    assert!(requested_delays.try_recv().is_err());
    assert!(server.await.expect("mock server exits"));
    registry.shutdown().await;
}

#[tokio::test]
async fn aggregate_queue_byte_overflow_is_terminal_data_gap_without_reconnect() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock WebSocket server");
    let endpoint = format!("ws://{}", listener.local_addr().expect("listener address"));
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept client");
        let mut socket = accept_async(stream).await.expect("WebSocket handshake");
        socket
            .next()
            .await
            .expect("subscribe command")
            .expect("valid subscribe command");
        socket
            .send(Message::text(
                json!({
                    "messageType": "I",
                    "data": {"subscriptionId": 57},
                    "response": {"code": 200, "message": "subscribed"}
                })
                .to_string(),
            ))
            .await
            .expect("send acknowledgement");
        let padding = "x".repeat(900_000);
        for index in 0..10 {
            if socket
                .send(Message::text(
                    json!({
                        "messageType": "A",
                        "service": "iex",
                        "data": ["2026-08-25T14:00:00Z", "AAPL", index as f64],
                        "padding": padding
                    })
                    .to_string(),
                ))
                .await
                .is_err()
            {
                return true;
            }
        }
        while let Some(message) = socket.next().await {
            if matches!(message, Ok(Message::Close(_))) {
                return true;
            }
        }
        false
    });
    let connector = TiingoConnector::with_endpoints(endpoint.clone(), endpoint)
        .expect("mock endpoint is valid");
    let (delay_requests, mut requested_delays) = tokio::sync::mpsc::unbounded_channel();
    let registry = MarketDataRegistry::with_connector_and_clocks(
        Some("test-key".into()),
        connector,
        Arc::new(FixedClock(Utc::now())),
        Arc::new(ManualReconnectClock {
            requests: delay_requests,
        }),
    );
    let started = registry
        .start(StartRequest {
            service: Service::Iex,
            symbols: vec!["AAPL".into()],
            threshold_level: None,
            confirm_iex_market_data_agreement: false,
        })
        .await
        .expect("subscription starts");

    let terminal =
        tokio::time::timeout(Duration::from_secs(2), registry.poll(&started.id, u64::MAX))
            .await
            .expect("byte overflow becomes terminal promptly")
            .expect("terminal state remains inspectable");
    assert_eq!(terminal.state, SubscriptionStatus::DataGap);
    assert!(terminal.events.is_empty());
    assert!(requested_delays.try_recv().is_err());
    assert!(server.await.expect("mock server exits"));
    registry.shutdown().await;
}

#[tokio::test]
async fn successful_reconnect_uses_fresh_subscribe_and_new_acknowledged_upstream_id() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock WebSocket server");
    let endpoint = format!("ws://{}", listener.local_addr().expect("listener address"));
    let (disconnect, disconnected) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept initial client");
        let mut socket = accept_async(stream)
            .await
            .expect("initial WebSocket handshake");
        socket
            .next()
            .await
            .expect("initial subscribe")
            .expect("valid initial subscribe");
        socket
            .send(Message::text(
                json!({
                    "messageType": "I",
                    "data": {"subscriptionId": "old-upstream-id"},
                    "response": {"code": 200, "message": "subscribed"}
                })
                .to_string(),
            ))
            .await
            .expect("send initial acknowledgement");
        disconnected.await.expect("test requests disconnect");
        socket
            .send(Message::Close(None))
            .await
            .expect("disconnect initial socket");
        drop(socket);

        let (stream, _) = listener.accept().await.expect("accept reconnect");
        let mut socket = accept_async(stream)
            .await
            .expect("reconnect WebSocket handshake");
        let fresh_subscribe = socket
            .next()
            .await
            .expect("fresh subscribe")
            .expect("valid fresh subscribe")
            .into_text()
            .expect("text subscribe");
        let fresh_subscribe: serde_json::Value =
            serde_json::from_str(&fresh_subscribe).expect("fresh subscribe JSON");
        assert_eq!(fresh_subscribe["eventName"], "subscribe");
        assert_eq!(fresh_subscribe["eventData"]["tickers"], json!(["AAPL"]));
        assert!(fresh_subscribe["eventData"].get("subscriptionId").is_none());
        socket
            .send(Message::text(
                json!({
                    "messageType": "I",
                    "data": {"subscriptionId": "new-upstream-id"},
                    "response": {"code": 200, "message": "subscribed"}
                })
                .to_string(),
            ))
            .await
            .expect("send reconnect acknowledgement");
        socket
            .send(Message::text(
                json!({
                    "messageType": "A",
                    "service": "iex",
                    "data": ["2026-08-25T14:00:01Z", "AAPL", 101.0]
                })
                .to_string(),
            ))
            .await
            .expect("send post-reconnect market message");

        let update = socket
            .next()
            .await
            .expect("post-reconnect update")
            .expect("valid update")
            .into_text()
            .expect("text update");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&update).expect("update JSON"),
            json!({
                "eventName": "subscribe",
                "authorization": "test-key",
                "eventData": {
                    "subscriptionId": "new-upstream-id",
                    "tickers": ["MSFT"]
                }
            })
        );
        socket
            .send(Message::text(
                json!({
                    "messageType": "I",
                    "data": {"subscriptionId": "updated-upstream-id"},
                    "response": {"code": 200, "message": "updated"}
                })
                .to_string(),
            ))
            .await
            .expect("send update acknowledgement");
        while let Some(message) = socket.next().await {
            if matches!(message, Ok(Message::Close(_))) {
                break;
            }
        }
    });
    let connector = TiingoConnector::with_endpoints(endpoint.clone(), endpoint)
        .expect("mock endpoint is valid");
    let (delay_requests, mut requested_delays) = tokio::sync::mpsc::unbounded_channel();
    let registry = MarketDataRegistry::with_connector_and_clocks(
        Some("test-key".into()),
        connector,
        Arc::new(FixedClock(Utc::now())),
        Arc::new(ManualReconnectClock {
            requests: delay_requests,
        }),
    );
    let started = registry
        .start(StartRequest {
            service: Service::Iex,
            symbols: vec!["AAPL".into()],
            threshold_level: None,
            confirm_iex_market_data_agreement: false,
        })
        .await
        .expect("subscription starts");

    disconnect.send(()).expect("disconnect initial socket");
    let (delay, release) = requested_delays
        .recv()
        .await
        .expect("worker requests reconnect delay");
    assert_eq!(delay, Duration::from_millis(250));
    release.send(()).expect("release reconnect delay");
    let post_reconnect = registry
        .poll(&started.id, 0)
        .await
        .expect("post-reconnect data is available");
    assert_eq!(post_reconnect.state, SubscriptionStatus::Active);
    assert_eq!(post_reconnect.events.len(), 1);

    let updated = registry
        .update(
            &started.id,
            UpdateRequest {
                add_symbols: vec!["MSFT".into()],
                remove_symbols: vec![],
                threshold_level: None,
            },
        )
        .await
        .expect("post-reconnect update succeeds");
    assert_eq!(updated.symbols, vec!["AAPL", "MSFT"]);
    let snapshots = format!("{started:?} {post_reconnect:?} {updated:?}");
    assert!(!snapshots.contains("old-upstream-id"));
    assert!(!snapshots.contains("new-upstream-id"));
    assert!(!snapshots.contains("updated-upstream-id"));
    assert!(requested_delays.try_recv().is_err());

    registry.shutdown().await;
    server.await.expect("mock server exits cleanly");
}

#[tokio::test]
async fn reconnect_establishment_expires_at_session_deadline_before_ack_timeout() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock WebSocket server");
    let endpoint = format!("ws://{}", listener.local_addr().expect("listener address"));
    let (heartbeat_requests, mut requested_heartbeats) =
        tokio::sync::mpsc::unbounded_channel::<(u8, tokio::sync::oneshot::Sender<()>)>();
    let (disconnect, disconnected) = tokio::sync::oneshot::channel();
    let (reconnect_seen, reconnect_received) = tokio::sync::oneshot::channel();
    let (reconnect_closed, reconnect_closed_received) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept initial client");
        let mut socket = accept_async(stream)
            .await
            .expect("initial WebSocket handshake");
        socket
            .next()
            .await
            .expect("initial subscribe")
            .expect("valid initial subscribe");
        socket
            .send(Message::text(
                json!({
                    "messageType": "I",
                    "data": {"subscriptionId": "expiry-initial-id"},
                    "response": {"code": 200, "message": "subscribed"}
                })
                .to_string(),
            ))
            .await
            .expect("send initial acknowledgement");
        let mut disconnected = Box::pin(disconnected);
        loop {
            tokio::select! {
                heartbeat = requested_heartbeats.recv() => {
                    let (cycle, heartbeat) =
                        heartbeat.expect("heartbeat request channel remains open");
                    socket
                        .send(Message::text(
                            json!({
                                "messageType": "H",
                                "response": {"code": 200, "message": "heartbeat"}
                            })
                            .to_string(),
                        ))
                        .await
                        .expect("send heartbeat");
                    let ping_payload = vec![cycle];
                    socket
                        .send(Message::Ping(ping_payload.clone().into()))
                        .await
                        .expect("send heartbeat processing barrier");
                    loop {
                        match socket.next().await {
                            Some(Ok(Message::Pong(payload))) if payload.as_ref() == ping_payload => {
                                break;
                            }
                            Some(Ok(_)) => {}
                            Some(Err(error)) => {
                                panic!("receive heartbeat processing barrier: {error}")
                            }
                            None => {
                                panic!("heartbeat processing barrier socket remains open in cycle {cycle}")
                            }
                        }
                    }
                    heartbeat
                        .send(())
                        .expect("record processed heartbeat barrier");
                }
                _ = &mut disconnected => {
                    socket
                        .send(Message::Close(None))
                        .await
                        .expect("disconnect initial socket");
                    break;
                }
            }
        }
        drop(socket);

        let (stream, _) = listener.accept().await.expect("accept reconnect");
        let mut socket = accept_async(stream)
            .await
            .expect("reconnect WebSocket handshake");
        socket
            .next()
            .await
            .expect("fresh subscribe")
            .expect("valid fresh subscribe");
        reconnect_seen.send(()).expect("record reconnect subscribe");
        while socket.next().await.is_some() {}
        reconnect_closed
            .send(())
            .expect("record reconnect socket close");
    });
    let connector = TiingoConnector::with_endpoints(endpoint.clone(), endpoint)
        .expect("mock endpoint is valid");
    let (delay_requests, mut requested_delays) = tokio::sync::mpsc::unbounded_channel();
    let registry = MarketDataRegistry::with_connector_and_clocks(
        Some("test-key".into()),
        connector,
        Arc::new(FixedClock(Utc::now())),
        Arc::new(ManualReconnectClock {
            requests: delay_requests,
        }),
    );
    let started = registry
        .start(StartRequest {
            service: Service::Iex,
            symbols: vec!["AAPL".into()],
            threshold_level: None,
            confirm_iex_market_data_agreement: false,
        })
        .await
        .expect("subscription starts");
    tokio::time::pause();
    const HEARTBEAT_CYCLES: u8 = 29;
    const HEARTBEAT_INTERVAL_SECONDS: u64 = 60;
    const PRE_RECONNECT_SECONDS: u64 = 58;
    const FINAL_EXPIRY_SECONDS: u64 = 2;
    const ABSOLUTE_LIFETIME_SECONDS: u64 = 1_800;
    const IDLE_LIFETIME_SECONDS: u64 = 300;
    const ACK_TIMEOUT_SECONDS: u64 = 5;
    const MAX_RECONNECT_SETUP_SECONDS: u64 = 1;
    const DIRECT_EXPIRY_CLOSE_BOUND_SECONDS: u64 = 1;
    let virtual_elapsed_seconds = u64::from(HEARTBEAT_CYCLES) * HEARTBEAT_INTERVAL_SECONDS
        + PRE_RECONNECT_SECONDS
        + FINAL_EXPIRY_SECONDS;
    assert_eq!(
        virtual_elapsed_seconds, ABSOLUTE_LIFETIME_SECONDS,
        "the test schedule reaches the absolute lifetime instead of the idle lifetime"
    );

    for cycle in 0..HEARTBEAT_CYCLES {
        tokio::time::advance(Duration::from_secs(HEARTBEAT_INTERVAL_SECONDS)).await;
        tokio::time::resume();
        let (heartbeat_sent, heartbeat_received) = tokio::sync::oneshot::channel();
        heartbeat_requests
            .send((cycle, heartbeat_sent))
            .expect("request heartbeat");
        heartbeat_received
            .await
            .unwrap_or_else(|_| panic!("worker processes heartbeat in cycle {cycle}"));
        if (cycle + 1) % 4 == 0 {
            let access = registry
                .poll_with_bounds(&started.id, u64::MAX, 1, Duration::ZERO)
                .await
                .expect("bounded public access refreshes idle expiry");
            assert_eq!(access.state, SubscriptionStatus::Active);
        }
        tokio::time::pause();
    }
    tokio::time::resume();
    let last_public_access = registry
        .poll_with_bounds(&started.id, u64::MAX, 1, Duration::ZERO)
        .await
        .expect("final bounded public access refreshes idle expiry");
    assert_eq!(last_public_access.state, SubscriptionStatus::Active);
    tokio::time::pause();
    let last_public_access_seconds = u64::from(HEARTBEAT_CYCLES) * HEARTBEAT_INTERVAL_SECONDS;
    assert_eq!(last_public_access_seconds, 1_740);
    let earliest_idle_deadline_seconds = last_public_access_seconds + IDLE_LIFETIME_SECONDS;
    assert_eq!(earliest_idle_deadline_seconds, 2_040);
    assert!(
        earliest_idle_deadline_seconds > ABSOLUTE_LIFETIME_SECONDS,
        "the refreshed idle deadline must remain after absolute expiry"
    );
    tokio::time::advance(Duration::from_secs(PRE_RECONNECT_SECONDS)).await;
    tokio::time::resume();
    disconnect.send(()).expect("request disconnect");
    let (delay, release) = requested_delays
        .recv()
        .await
        .expect("worker requests reconnect delay");
    assert_eq!(delay, Duration::from_millis(250));
    let reconnect_released_at = tokio::time::Instant::now();
    release.send(()).expect("release reconnect delay");
    reconnect_received
        .await
        .expect("fresh subscribe is sent before expiry");

    tokio::time::pause();
    let reconnect_setup_elapsed = tokio::time::Instant::now() - reconnect_released_at;
    assert!(
        reconnect_setup_elapsed < Duration::from_secs(MAX_RECONNECT_SETUP_SECONDS),
        "local reconnect setup preserves the reviewed acknowledgement-timeout margin"
    );
    let minimum_ack_remaining_at_expiry =
        ACK_TIMEOUT_SECONDS - MAX_RECONNECT_SETUP_SECONDS - FINAL_EXPIRY_SECONDS;
    assert_eq!(minimum_ack_remaining_at_expiry, 2);
    assert!(DIRECT_EXPIRY_CLOSE_BOUND_SECONDS < minimum_ack_remaining_at_expiry);
    tokio::time::advance(Duration::from_secs(FINAL_EXPIRY_SECONDS)).await;
    tokio::time::resume();
    tokio::time::timeout(
        Duration::from_secs(DIRECT_EXPIRY_CLOSE_BOUND_SECONDS),
        reconnect_closed_received,
    )
    .await
    .expect("absolute deadline closes reconnect before the acknowledgement fallback window")
    .expect("reconnect close signal remains available");
    let terminal = registry
        .poll(&started.id, u64::MAX)
        .await
        .expect("terminal state remains inspectable");
    registry.shutdown().await;
    server.await.expect("mock server exits cleanly");
    assert_eq!(terminal.state, SubscriptionStatus::Expired);
}
