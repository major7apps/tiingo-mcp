use chrono::{DateTime, Utc};
use serde_json::json;
use tiingo_mcp::{
    error::TiingoError,
    websocket::protocol::{
        Authorization, ConsolidatedLiquidityUpdate, IexTopsUpdate, IexUpdateKind, Information,
        MarketData, ProtocolCodec, RawMessage, RawMessageType, ReceivedMessage, ReferenceUpdate,
        Response, ServerMessage, Service, SubscriptionAction, SubscriptionId,
    },
};

fn received_at() -> DateTime<Utc> {
    "2026-08-25T14:30:01Z".parse().unwrap()
}

#[test]
fn websocket_services_expose_only_the_approved_endpoints_and_codes() {
    assert_eq!(Service::Iex.code(), "iex");
    assert_eq!(Service::Iex.endpoint(), "wss://api.tiingo.com/iex");
    assert_eq!(Service::Consolidated.code(), "cons");
    assert_eq!(
        Service::Consolidated.endpoint(),
        "wss://api.tiingo.com/equity/intraday"
    );
}

#[test]
fn websocket_services_reject_unapproved_thresholds() {
    for threshold in [0, 5, 6] {
        ProtocolCodec::new(Service::Iex, threshold).unwrap();
    }
    for threshold in [4, 6] {
        ProtocolCodec::new(Service::Consolidated, threshold).unwrap();
    }

    for (service, threshold) in [
        (Service::Iex, 4),
        (Service::Iex, 7),
        (Service::Consolidated, 0),
        (Service::Consolidated, 5),
    ] {
        assert!(matches!(
            ProtocolCodec::new(service, threshold),
            Err(TiingoError::Validation(_))
        ));
    }
}

#[test]
fn initial_subscribe_serializes_the_exact_vendor_frame() {
    let codec = ProtocolCodec::new(Service::Iex, 6).unwrap();
    let frame = codec.initial_subscribe(Authorization::new("<token>"), vec!["spy".to_owned()]);

    assert_eq!(
        serde_json::to_string(&frame).unwrap(),
        r#"{"eventName":"subscribe","authorization":"<token>","eventData":{"thresholdLevel":6,"tickers":["spy"]}}"#
    );
}

#[test]
fn updates_serialize_the_acknowledged_subscription_id_and_ticker_array() {
    let codec = ProtocolCodec::new(Service::Iex, 6).unwrap();
    let authorization = Authorization::new("frame-secret");

    let subscribe = codec.subscription_update(
        SubscriptionAction::Subscribe,
        authorization.clone(),
        SubscriptionId::Number(61),
        vec!["aapl".to_owned(), "msft".to_owned()],
    );
    assert_eq!(
        serde_json::to_string(&subscribe).unwrap(),
        r#"{"eventName":"subscribe","authorization":"frame-secret","eventData":{"subscriptionId":61,"tickers":["aapl","msft"]}}"#
    );

    let unsubscribe = codec.subscription_update(
        SubscriptionAction::Unsubscribe,
        authorization,
        SubscriptionId::String("upstream-61".to_owned()),
        vec!["msft".to_owned()],
    );
    assert_eq!(
        serde_json::to_string(&unsubscribe).unwrap(),
        r#"{"eventName":"unsubscribe","authorization":"frame-secret","eventData":{"subscriptionId":"upstream-61","tickers":["msft"]}}"#
    );
}

#[test]
fn authorization_is_redacted_from_debug_output() {
    let codec = ProtocolCodec::new(Service::Iex, 6).unwrap();
    let authorization = Authorization::new("debug-frame-secret");
    let frame = codec.initial_subscribe(authorization.clone(), vec!["spy".to_owned()]);

    for rendered in [format!("{authorization:?}"), format!("{frame:?}")] {
        assert!(!rendered.contains("debug-frame-secret"));
        assert!(!rendered.to_ascii_lowercase().contains("authorization"));
        assert!(rendered.contains("[REDACTED]"));
    }
}

#[test]
fn decodes_the_exact_iex_reference_array_and_both_timestamps() {
    let codec = ProtocolCodec::new(Service::Iex, 6).unwrap();

    let message = codec
        .decode(
            br#"{"service":"iex","messageType":"A","data":["2026-08-25T14:30:00.123456789Z","spy",647.25]}"#,
            received_at(),
        )
        .unwrap();

    assert_eq!(
        message,
        ReceivedMessage {
            received_at: received_at(),
            message: ServerMessage::Market(MarketData::IexReference(ReferenceUpdate {
                vendor_timestamp: "2026-08-25T14:30:00.123456789Z".to_owned(),
                ticker: "spy".to_owned(),
                reference_price: 647.25,
            })),
        }
    );
}

#[test]
fn decodes_iex_trade_quote_and_break_arrays_with_documented_null_slots() {
    let codec = ProtocolCodec::new(Service::Iex, 0).unwrap();
    let fixtures = [
        (
            br#"{"service":"iex","messageType":"A","data":["T","2026-08-25T14:30:00.000000001Z",1787668200000000001,"spy",null,null,null,null,null,647.25,100,0,0,1,0,1]}"#.as_slice(),
            IexUpdateKind::Trade,
            None,
            Some(647.25),
        ),
        (
            br#"{"service":"iex","messageType":"A","data":["Q","2026-08-25T14:30:00.000000002Z",1787668200000000002,"spy",200,647.20,647.225,647.25,300,null,null,0,0,0,null,null]}"#.as_slice(),
            IexUpdateKind::Quote,
            Some(647.20),
            None,
        ),
        (
            br#"{"service":"iex","messageType":"A","data":["B","2026-08-25T14:30:00.000000003Z",1787668200000000003,"spy",null,null,null,null,null,647.10,50,0,1,0,null,null]}"#.as_slice(),
            IexUpdateKind::Break,
            None,
            Some(647.10),
        ),
    ];

    for (payload, expected_kind, expected_bid, expected_last) in fixtures {
        let ReceivedMessage {
            received_at: actual_received_at,
            message: ServerMessage::Market(MarketData::IexTops(update)),
        } = codec.decode(payload, received_at()).unwrap()
        else {
            panic!("expected an IEX TOPS update");
        };

        assert_eq!(actual_received_at, received_at());
        assert_eq!(update.update_kind, expected_kind);
        assert_eq!(update.vendor_timestamp.len(), 30);
        assert_eq!(update.ticker, "spy");
        assert_eq!(update.bid_price, expected_bid);
        assert_eq!(update.last_price, expected_last);
    }
}

#[test]
fn decodes_every_iex_tops_field_without_reordering() {
    let codec = ProtocolCodec::new(Service::Iex, 5).unwrap();

    let ReceivedMessage {
        message: ServerMessage::Market(MarketData::IexTops(update)),
        ..
    } = codec
        .decode(
            br#"{"service":"iex","messageType":"A","data":["T","2026-08-25T14:30:00Z",1787668200000000000,"spy",10,647.1,647.2,647.3,20,647.25,100,0,1,1,0,1]}"#,
            received_at(),
        )
        .unwrap()
    else {
        panic!("expected an IEX TOPS update");
    };

    assert_eq!(
        update,
        IexTopsUpdate {
            update_kind: IexUpdateKind::Trade,
            vendor_timestamp: "2026-08-25T14:30:00Z".to_owned(),
            epoch_nanoseconds: 1_787_668_200_000_000_000,
            ticker: "spy".to_owned(),
            bid_size: Some(10),
            bid_price: Some(647.1),
            mid_price: Some(647.2),
            ask_price: Some(647.3),
            ask_size: Some(20),
            last_price: Some(647.25),
            last_size: Some(100),
            halted: 0,
            after_hours: 1,
            intermarket_sweep_order: 1,
            odd_lot: Some(0),
            rule_611: Some(1),
        }
    );
}

#[test]
fn decodes_the_exact_consolidated_reference_array() {
    let codec = ProtocolCodec::new(Service::Consolidated, 6).unwrap();

    let ReceivedMessage {
        message: ServerMessage::Market(MarketData::ConsolidatedReference(update)),
        ..
    } = codec
        .decode(
            br#"{"service":"cons","messageType":"A","data":["2026-08-25T14:30:00Z","spy",647.25]}"#,
            received_at(),
        )
        .unwrap()
    else {
        panic!("expected a consolidated reference update");
    };

    assert_eq!(
        update,
        ReferenceUpdate {
            vendor_timestamp: "2026-08-25T14:30:00Z".to_owned(),
            ticker: "spy".to_owned(),
            reference_price: 647.25,
        }
    );
}

#[test]
fn decodes_the_exact_consolidated_liquidity_array() {
    let codec = ProtocolCodec::new(Service::Consolidated, 4).unwrap();

    let ReceivedMessage {
        message: ServerMessage::Market(MarketData::ConsolidatedLiquidity(update)),
        ..
    } = codec
        .decode(
            br#"{"service":"cons","messageType":"A","data":["2026-08-25T14:30:00Z","spy",0.0004,120,647.10,647.25,647.40,180]}"#,
            received_at(),
        )
        .unwrap()
    else {
        panic!("expected a consolidated liquidity update");
    };

    assert_eq!(
        update,
        ConsolidatedLiquidityUpdate {
            vendor_timestamp: "2026-08-25T14:30:00Z".to_owned(),
            ticker: "spy".to_owned(),
            liquidity_spread: 0.0004,
            liquidity_bid_size: 120,
            liquidity_bid_price: 647.10,
            reference_price: 647.25,
            liquidity_ask_price: 647.40,
            liquidity_ask_size: 180,
        }
    );
}

#[test]
fn decodes_information_with_numeric_and_string_subscription_ids() {
    let codec = ProtocolCodec::new(Service::Iex, 6).unwrap();

    for (payload, subscription_id) in [
        (
            br#"{"data":{"subscriptionId":61},"response":{"message":"Success","code":200},"messageType":"I"}"#.as_slice(),
            SubscriptionId::Number(61),
        ),
        (
            br#"{"data":{"subscriptionId":"upstream-61"},"response":{"message":"Success","code":200},"messageType":"I"}"#.as_slice(),
            SubscriptionId::String("upstream-61".to_owned()),
        ),
    ] {
        assert_eq!(
            codec.decode(payload, received_at()).unwrap(),
            ReceivedMessage {
                received_at: received_at(),
                message: ServerMessage::Information(Information {
                    subscription_id,
                    response: Response {
                        code: 200,
                        message: "Success".to_owned(),
                    },
                }),
            }
        );
    }
}

#[test]
fn decodes_heartbeat_response() {
    let codec = ProtocolCodec::new(Service::Consolidated, 6).unwrap();

    assert_eq!(
        codec
            .decode(
                br#"{"response":{"message":"HeartBeat","code":200},"messageType":"H"}"#,
                received_at(),
            )
            .unwrap(),
        ReceivedMessage {
            received_at: received_at(),
            message: ServerMessage::Heartbeat(Response {
                code: 200,
                message: "HeartBeat".to_owned(),
            }),
        }
    );
}

#[test]
fn preserves_the_complete_undocumented_update_delete_and_error_payloads() {
    let codec = ProtocolCodec::new(Service::Iex, 6).unwrap();
    let fixtures = [
        ("U", RawMessageType::Update),
        ("D", RawMessageType::Delete),
        ("E", RawMessageType::Error),
    ];

    for (message_type, expected_type) in fixtures {
        let payload = json!({
            "service": "iex",
            "messageType": message_type,
            "data": {"vendorOwned": [1, null, {"nested": true}]},
            "response": {"code": 409, "message": "vendor detail"}
        });
        let bytes = serde_json::to_vec(&payload).unwrap();

        assert_eq!(
            codec.decode(&bytes, received_at()).unwrap(),
            ReceivedMessage {
                received_at: received_at(),
                message: ServerMessage::Raw(RawMessage {
                    message_type: expected_type,
                    payload,
                }),
            }
        );
    }
}

#[test]
fn raw_vendor_payloads_are_preserved_but_never_debug_printed() {
    let codec = ProtocolCodec::new(Service::Iex, 6).unwrap();
    let decoded = codec
        .decode(
            br#"{"messageType":"E","authorization":"debug-vendor-secret","data":{"token":"debug-vendor-secret"}}"#,
            received_at(),
        )
        .unwrap();

    let ServerMessage::Raw(raw) = &decoded.message else {
        panic!("expected a raw vendor message");
    };
    assert_eq!(raw.payload["authorization"], "debug-vendor-secret");
    let rendered = format!("{decoded:?}");
    assert!(!rendered.contains("debug-vendor-secret"));
    assert!(!rendered.to_ascii_lowercase().contains("authorization"));
    assert!(rendered.contains("[REDACTED]"));
}

#[test]
fn vendor_response_messages_are_preserved_but_never_debug_printed() {
    let codec = ProtocolCodec::new(Service::Iex, 6).unwrap();
    let decoded = codec
        .decode(
            br#"{"data":{"subscriptionId":61},"response":{"message":"Authorization error-secret","code":401},"messageType":"I"}"#,
            received_at(),
        )
        .unwrap();

    let ServerMessage::Information(information) = &decoded.message else {
        panic!("expected an information message");
    };
    assert_eq!(information.response.message, "Authorization error-secret");
    let rendered = format!("{decoded:?}");
    assert!(!rendered.contains("error-secret"));
    assert!(!rendered.to_ascii_lowercase().contains("authorization"));
    assert!(rendered.contains("[REDACTED]"));
}

fn assert_protocol_error(codec: &ProtocolCodec, payload: &[u8]) {
    assert!(matches!(
        codec.decode(payload, received_at()),
        Err(TiingoError::WebSocketProtocol { .. })
    ));
}

#[test]
fn rejects_partial_and_coalesced_json_inside_one_completed_websocket_message() {
    let codec = ProtocolCodec::new(Service::Iex, 6).unwrap();
    let complete =
        br#"{"service":"iex","messageType":"A","data":["2026-08-25T14:30:00Z","spy",647.25]}"#;
    let split = complete.len() / 2;

    assert_protocol_error(&codec, &complete[..split]);
    assert_protocol_error(&codec, &complete[split..]);

    let mut coalesced = complete.to_vec();
    coalesced
        .extend_from_slice(br#"{"response":{"message":"HeartBeat","code":200},"messageType":"H"}"#);
    assert_protocol_error(&codec, &coalesced);
}

#[test]
fn rejects_invalid_json_utf8_and_non_object_envelopes_without_panicking() {
    let codec = ProtocolCodec::new(Service::Iex, 6).unwrap();

    for payload in [
        br#"{"messageType":}"#.as_slice(),
        &[0xff, 0xfe, 0xfd],
        br#"[]"#.as_slice(),
        br#"null"#.as_slice(),
    ] {
        assert_protocol_error(&codec, payload);
    }
}

#[test]
fn rejects_unknown_or_wrongly_typed_global_message_types() {
    let codec = ProtocolCodec::new(Service::Iex, 6).unwrap();

    for payload in [
        br#"{}"#.as_slice(),
        br#"{"messageType":7}"#.as_slice(),
        br#"{"messageType":"Z"}"#.as_slice(),
        br#"{"messageType":"AA"}"#.as_slice(),
    ] {
        assert_protocol_error(&codec, payload);
    }
}

#[test]
fn rejects_market_data_from_the_wrong_or_wrongly_typed_service() {
    let codec = ProtocolCodec::new(Service::Iex, 6).unwrap();

    for payload in [
        br#"{"service":"cons","messageType":"A","data":["2026-08-25T14:30:00Z","spy",647.25]}"#
            .as_slice(),
        br#"{"service":7,"messageType":"A","data":["2026-08-25T14:30:00Z","spy",647.25]}"#
            .as_slice(),
        br#"{"messageType":"A","data":["2026-08-25T14:30:00Z","spy",647.25]}"#.as_slice(),
    ] {
        assert_protocol_error(&codec, payload);
    }
}

#[test]
fn rejects_arrays_that_do_not_match_the_configured_service_threshold() {
    let fixtures = [
        (
            ProtocolCodec::new(Service::Iex, 6).unwrap(),
            br#"{"service":"iex","messageType":"A","data":["Q","2026-08-25T14:30:00Z",1,"spy",1,1.0,1.0,1.0,1,null,null,0,0,0,null,null]}"#.as_slice(),
        ),
        (
            ProtocolCodec::new(Service::Iex, 0).unwrap(),
            br#"{"service":"iex","messageType":"A","data":["2026-08-25T14:30:00Z","spy",647.25]}"#.as_slice(),
        ),
        (
            ProtocolCodec::new(Service::Consolidated, 4).unwrap(),
            br#"{"service":"cons","messageType":"A","data":["2026-08-25T14:30:00Z","spy",647.25]}"#.as_slice(),
        ),
        (
            ProtocolCodec::new(Service::Consolidated, 6).unwrap(),
            br#"{"service":"cons","messageType":"A","data":["2026-08-25T14:30:00Z","spy",0.1,1,1.0,1.0,1.0,1]}"#.as_slice(),
        ),
    ];

    for (codec, payload) in fixtures {
        assert_protocol_error(&codec, payload);
    }
}

#[test]
fn rejects_wrong_market_array_lengths_and_field_types() {
    let reference = ProtocolCodec::new(Service::Iex, 6).unwrap();
    for payload in [
        br#"{"service":"iex","messageType":"A","data":{}}"#.as_slice(),
        br#"{"service":"iex","messageType":"A","data":[]}"#.as_slice(),
        br#"{"service":"iex","messageType":"A","data":["date","spy",1.0,2.0]}"#.as_slice(),
        br#"{"service":"iex","messageType":"A","data":[7,"spy",1.0]}"#.as_slice(),
        br#"{"service":"iex","messageType":"A","data":["date",7,1.0]}"#.as_slice(),
        br#"{"service":"iex","messageType":"A","data":["date","spy","1.0"]}"#.as_slice(),
        br#"{"service":"iex","messageType":"A","data":["date","spy",null]}"#.as_slice(),
    ] {
        assert_protocol_error(&reference, payload);
    }

    let tops = ProtocolCodec::new(Service::Iex, 0).unwrap();
    for payload in [
        br#"{"service":"iex","messageType":"A","data":["X","date",1,"spy",1,1.0,1.0,1.0,1,1.0,1,0,0,0,0,0]}"#.as_slice(),
        br#"{"service":"iex","messageType":"A","data":["Q",7,1,"spy",1,1.0,1.0,1.0,1,null,null,0,0,0,null,null]}"#.as_slice(),
        br#"{"service":"iex","messageType":"A","data":["Q","date",1.5,"spy",1,1.0,1.0,1.0,1,null,null,0,0,0,null,null]}"#.as_slice(),
        br#"{"service":"iex","messageType":"A","data":["Q","date",1,7,1,1.0,1.0,1.0,1,null,null,0,0,0,null,null]}"#.as_slice(),
        br#"{"service":"iex","messageType":"A","data":["Q","date",1,"spy","1",1.0,1.0,1.0,1,null,null,0,0,0,null,null]}"#.as_slice(),
        br#"{"service":"iex","messageType":"A","data":["Q","date",1,"spy",1,"1.0",1.0,1.0,1,null,null,0,0,0,null,null]}"#.as_slice(),
        br#"{"service":"iex","messageType":"A","data":["Q","date",1,"spy",1,1.0,1.0,1.0,1,null,null,null,0,0,null,null]}"#.as_slice(),
        br#"{"service":"iex","messageType":"A","data":["Q","date",1,"spy",1,1.0,1.0,1.0,1,null,null,0,null,0,null,null]}"#.as_slice(),
        br#"{"service":"iex","messageType":"A","data":["Q","date",1,"spy",1,1.0,1.0,1.0,1,null,null,0,0,null,null,null]}"#.as_slice(),
    ] {
        assert_protocol_error(&tops, payload);
    }

    let liquidity = ProtocolCodec::new(Service::Consolidated, 4).unwrap();
    for payload in [
        br#"{"service":"cons","messageType":"A","data":["date","spy","0.1",1,1.0,1.0,1.0,1]}"#
            .as_slice(),
        br#"{"service":"cons","messageType":"A","data":["date","spy",0.1,1.5,1.0,1.0,1.0,1]}"#
            .as_slice(),
        br#"{"service":"cons","messageType":"A","data":["date","spy",0.1,1,1.0,1.0,1.0,null]}"#
            .as_slice(),
    ] {
        assert_protocol_error(&liquidity, payload);
    }
}

#[test]
fn rejects_non_finite_market_numbers_instead_of_converting_them_to_null() {
    let codec = ProtocolCodec::new(Service::Iex, 6).unwrap();
    assert_protocol_error(
        &codec,
        br#"{"service":"iex","messageType":"A","data":["date","spy",1e400]}"#,
    );
}

#[test]
fn rejects_malformed_information_and_heartbeat_fields() {
    let codec = ProtocolCodec::new(Service::Iex, 6).unwrap();

    for payload in [
        br#"{"data":{},"response":{"message":"Success","code":200},"messageType":"I"}"#.as_slice(),
        br#"{"data":{"subscriptionId":-1},"response":{"message":"Success","code":200},"messageType":"I"}"#.as_slice(),
        br#"{"data":{"subscriptionId":1.5},"response":{"message":"Success","code":200},"messageType":"I"}"#.as_slice(),
        br#"{"data":{"subscriptionId":61},"response":{"message":"Success","code":"200"},"messageType":"I"}"#.as_slice(),
        br#"{"data":{"subscriptionId":61},"response":{"message":200,"code":200},"messageType":"I"}"#.as_slice(),
        br#"{"messageType":"H"}"#.as_slice(),
        br#"{"response":[],"messageType":"H"}"#.as_slice(),
    ] {
        assert_protocol_error(&codec, payload);
    }
}

#[test]
fn protocol_errors_never_echo_authorization_or_tokens() {
    let codec = ProtocolCodec::new(Service::Iex, 6).unwrap();
    let error = codec
        .decode(
            br#"{"authorization":"error-secret","token":"error-secret","service":"wrong","messageType":"A","data":[]}"#,
            received_at(),
        )
        .unwrap_err();

    for rendered in [
        error.to_string(),
        format!("{error:?}"),
        error.payload().message,
    ] {
        assert!(!rendered.contains("error-secret"));
        assert!(!rendered.to_ascii_lowercase().contains("authorization"));
    }
}

#[test]
fn rejects_vendor_messages_above_the_shared_response_bound() {
    let codec = ProtocolCodec::new(Service::Iex, 6).unwrap();
    let oversized = format!(
        r#"{{"messageType":"E","data":"{}"}}"#,
        "x".repeat(8 * 1024 * 1024)
    );

    assert_protocol_error(&codec, oversized.as_bytes());
}
