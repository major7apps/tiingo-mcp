use std::fmt;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Number, Value};

use crate::{
    config::{CONSOLIDATED_WEBSOCKET_URL, IEX_WEBSOCKET_URL, MAX_WEBSOCKET_MESSAGE_BYTES},
    error::TiingoError,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Service {
    Iex,
    Consolidated,
}

impl Service {
    pub const fn code(self) -> &'static str {
        match self {
            Self::Iex => "iex",
            Self::Consolidated => "cons",
        }
    }

    pub const fn endpoint(self) -> &'static str {
        match self {
            Self::Iex => IEX_WEBSOCKET_URL,
            Self::Consolidated => CONSOLIDATED_WEBSOCKET_URL,
        }
    }

    fn accepts_threshold(self, threshold_level: u8) -> bool {
        match self {
            Self::Iex => matches!(threshold_level, 0 | 5 | 6),
            Self::Consolidated => matches!(threshold_level, 4 | 6),
        }
    }
}

#[derive(Clone, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct Authorization(String);

impl Authorization {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub(crate) fn redact(&self, value: &str) -> String {
        if self.0.is_empty() {
            value.to_owned()
        } else {
            value.replace(&self.0, "[REDACTED]")
        }
    }

    pub(crate) fn matches_number(&self, number: &Number) -> bool {
        self.0
            .parse::<u64>()
            .ok()
            .is_some_and(|value| json_number_matches_u64(number, value))
    }
}

pub(crate) fn redact_bounded_secret(value: &str, secret: &str) -> String {
    if secret.is_empty() {
        return value.to_owned();
    }
    let first_is_token = secret.chars().next().is_some_and(is_token_character);
    let last_is_token = secret.chars().next_back().is_some_and(is_token_character);
    let mut redacted = String::with_capacity(value.len());
    let mut cursor = 0;
    for (start, _) in value.match_indices(secret) {
        if start < cursor {
            continue;
        }
        let end = start + secret.len();
        let before_is_token = value[..start]
            .chars()
            .next_back()
            .is_some_and(is_token_character);
        let after_is_token = value[end..].chars().next().is_some_and(is_token_character);
        if (first_is_token && before_is_token) || (last_is_token && after_is_token) {
            continue;
        }
        redacted.push_str(&value[cursor..start]);
        redacted.push_str("[REDACTED]");
        cursor = end;
    }
    if cursor == 0 {
        value.to_owned()
    } else {
        redacted.push_str(&value[cursor..]);
        redacted
    }
}

pub(crate) fn json_number_matches_u64(number: &Number, value: u64) -> bool {
    number.as_u64() == Some(value)
        || (value <= (1_u64 << f64::MANTISSA_DIGITS)
            && number
                .as_f64()
                .is_some_and(|candidate| candidate == value as f64))
}

fn is_token_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '_'
}

impl fmt::Debug for Authorization {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("[REDACTED]")
    }
}

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(untagged)]
pub enum SubscriptionId {
    Number(u64),
    String(String),
}

impl fmt::Debug for SubscriptionId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("SubscriptionId")
            .field(&"[REDACTED]")
            .finish()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SubscriptionAction {
    Subscribe,
    Unsubscribe,
}

#[derive(Clone)]
pub struct ProtocolCodec {
    service: Service,
    threshold_level: u8,
}

impl ProtocolCodec {
    pub fn new(service: Service, threshold_level: u8) -> Result<Self, TiingoError> {
        if !service.accepts_threshold(threshold_level) {
            return Err(TiingoError::Validation(format!(
                "threshold level {threshold_level} is not supported for {} WebSocket data",
                service.code()
            )));
        }
        Ok(Self {
            service,
            threshold_level,
        })
    }

    pub fn initial_subscribe(
        &self,
        authorization: Authorization,
        tickers: Vec<String>,
    ) -> SubscriptionCommand {
        SubscriptionCommand {
            event_name: SubscriptionAction::Subscribe,
            authorization,
            event_data: EventData::Initial(InitialEventData {
                threshold_level: self.threshold_level,
                tickers,
            }),
        }
    }

    pub fn subscription_update(
        &self,
        action: SubscriptionAction,
        authorization: Authorization,
        subscription_id: SubscriptionId,
        tickers: Vec<String>,
    ) -> SubscriptionCommand {
        SubscriptionCommand {
            event_name: action,
            authorization,
            event_data: EventData::Update(UpdateEventData {
                subscription_id,
                tickers,
            }),
        }
    }

    pub fn decode(
        &self,
        payload: &[u8],
        received_at: DateTime<Utc>,
    ) -> Result<ReceivedMessage, TiingoError> {
        if payload.len() > MAX_WEBSOCKET_MESSAGE_BYTES {
            return Err(protocol_error("message exceeds the configured size bound"));
        }
        let envelope: Value = serde_json::from_slice(payload)
            .map_err(|_| protocol_error("message is not one complete JSON object"))?;
        let object = envelope
            .as_object()
            .ok_or_else(|| protocol_error("message envelope must be an object"))?;
        let message_type = object
            .get("messageType")
            .and_then(Value::as_str)
            .ok_or_else(|| protocol_error("messageType must be a string"))?;
        let message = match message_type {
            "A" => ServerMessage::Market(self.decode_market(object)?),
            "I" => ServerMessage::Information(parse_information(object)?),
            "H" => ServerMessage::Heartbeat(parse_response(object)?),
            "U" | "D" | "E" => ServerMessage::Raw(RawMessage {
                message_type: match message_type {
                    "U" => RawMessageType::Update,
                    "D" => RawMessageType::Delete,
                    "E" => RawMessageType::Error,
                    _ => unreachable!(),
                },
                payload: envelope,
            }),
            _ => return Err(protocol_error("unsupported messageType")),
        };

        Ok(ReceivedMessage {
            received_at,
            message,
        })
    }

    fn decode_market(
        &self,
        object: &serde_json::Map<String, Value>,
    ) -> Result<MarketData, TiingoError> {
        let service = object
            .get("service")
            .and_then(Value::as_str)
            .ok_or_else(|| protocol_error("market message service must be a string"))?;
        if service != self.service.code() {
            return Err(protocol_error(
                "market message service does not match the subscription",
            ));
        }
        let data = object
            .get("data")
            .and_then(Value::as_array)
            .ok_or_else(|| protocol_error("market message data must be an array"))?;

        match (self.service, self.threshold_level) {
            (Service::Iex, 6) => Ok(MarketData::IexReference(parse_reference(data)?)),
            (Service::Iex, 0 | 5) => Ok(MarketData::IexTops(parse_iex_tops(data)?)),
            (Service::Consolidated, 6) => {
                Ok(MarketData::ConsolidatedReference(parse_reference(data)?))
            }
            (Service::Consolidated, 4) => Ok(MarketData::ConsolidatedLiquidity(
                parse_consolidated_liquidity(data)?,
            )),
            _ => unreachable!("ProtocolCodec::new validates service thresholds"),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ReceivedMessage {
    pub received_at: DateTime<Utc>,
    pub message: ServerMessage,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ServerMessage {
    Market(MarketData),
    Information(Information),
    Heartbeat(Response),
    Raw(RawMessage),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Information {
    pub subscription_id: SubscriptionId,
    pub response: Response,
}

#[derive(Clone, Eq, PartialEq)]
pub struct Response {
    pub code: i64,
    pub message: String,
}

impl fmt::Debug for Response {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Response")
            .field("code", &self.code)
            .field("message", &"[REDACTED]")
            .finish()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RawMessageType {
    Update,
    Delete,
    Error,
}

#[derive(Clone, PartialEq)]
pub struct RawMessage {
    pub message_type: RawMessageType,
    pub payload: Value,
}

impl fmt::Debug for RawMessage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RawMessage")
            .field("message_type", &self.message_type)
            .field("payload", &"[REDACTED]")
            .finish()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum MarketData {
    IexReference(ReferenceUpdate),
    IexTops(IexTopsUpdate),
    ConsolidatedReference(ReferenceUpdate),
    ConsolidatedLiquidity(ConsolidatedLiquidityUpdate),
}

#[derive(Clone, Debug, PartialEq)]
pub struct ReferenceUpdate {
    pub vendor_timestamp: String,
    pub ticker: String,
    pub reference_price: f64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IexUpdateKind {
    Trade,
    Quote,
    Break,
}

#[derive(Clone, Debug, PartialEq)]
pub struct IexTopsUpdate {
    pub update_kind: IexUpdateKind,
    pub vendor_timestamp: String,
    pub epoch_nanoseconds: i64,
    pub ticker: String,
    pub bid_size: Option<i32>,
    pub bid_price: Option<f64>,
    pub mid_price: Option<f64>,
    pub ask_price: Option<f64>,
    pub ask_size: Option<i32>,
    pub last_price: Option<f64>,
    pub last_size: Option<i32>,
    pub halted: i32,
    pub after_hours: i32,
    pub intermarket_sweep_order: i32,
    pub odd_lot: Option<i32>,
    pub rule_611: Option<i32>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ConsolidatedLiquidityUpdate {
    pub vendor_timestamp: String,
    pub ticker: String,
    pub liquidity_spread: f64,
    pub liquidity_bid_size: i32,
    pub liquidity_bid_price: f64,
    pub reference_price: f64,
    pub liquidity_ask_price: f64,
    pub liquidity_ask_size: i32,
}

fn parse_reference(data: &[Value]) -> Result<ReferenceUpdate, TiingoError> {
    require_length(data, 3)?;
    Ok(ReferenceUpdate {
        vendor_timestamp: required_string(&data[0])?,
        ticker: required_string(&data[1])?,
        reference_price: required_f64(&data[2])?,
    })
}

fn parse_information(object: &serde_json::Map<String, Value>) -> Result<Information, TiingoError> {
    let data = object
        .get("data")
        .and_then(Value::as_object)
        .ok_or_else(|| protocol_error("information data must be an object"))?;
    let subscription_id = serde_json::from_value(
        data.get("subscriptionId")
            .cloned()
            .ok_or_else(|| protocol_error("information subscriptionId is required"))?,
    )
    .map_err(|_| protocol_error("subscriptionId must be an unsigned integer or string"))?;
    Ok(Information {
        subscription_id,
        response: parse_response(object)?,
    })
}

fn parse_response(object: &serde_json::Map<String, Value>) -> Result<Response, TiingoError> {
    let response = object
        .get("response")
        .and_then(Value::as_object)
        .ok_or_else(|| protocol_error("response must be an object"))?;
    Ok(Response {
        code: response
            .get("code")
            .and_then(Value::as_i64)
            .ok_or_else(|| protocol_error("response code must be an integer"))?,
        message: response
            .get("message")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| protocol_error("response message must be a string"))?,
    })
}

fn parse_iex_tops(data: &[Value]) -> Result<IexTopsUpdate, TiingoError> {
    require_length(data, 16)?;
    let update_kind = match required_string(&data[0])?.as_str() {
        "T" => IexUpdateKind::Trade,
        "Q" => IexUpdateKind::Quote,
        "B" => IexUpdateKind::Break,
        _ => return Err(protocol_error("IEX update kind must be T, Q, or B")),
    };
    Ok(IexTopsUpdate {
        update_kind,
        vendor_timestamp: required_string(&data[1])?,
        epoch_nanoseconds: required_i64(&data[2])?,
        ticker: required_string(&data[3])?,
        bid_size: optional_i32(&data[4])?,
        bid_price: optional_f64(&data[5])?,
        mid_price: optional_f64(&data[6])?,
        ask_price: optional_f64(&data[7])?,
        ask_size: optional_i32(&data[8])?,
        last_price: optional_f64(&data[9])?,
        last_size: optional_i32(&data[10])?,
        halted: required_i32(&data[11])?,
        after_hours: required_i32(&data[12])?,
        intermarket_sweep_order: required_i32(&data[13])?,
        odd_lot: optional_i32(&data[14])?,
        rule_611: optional_i32(&data[15])?,
    })
}

fn parse_consolidated_liquidity(
    data: &[Value],
) -> Result<ConsolidatedLiquidityUpdate, TiingoError> {
    require_length(data, 8)?;
    Ok(ConsolidatedLiquidityUpdate {
        vendor_timestamp: required_string(&data[0])?,
        ticker: required_string(&data[1])?,
        liquidity_spread: required_f64(&data[2])?,
        liquidity_bid_size: required_i32(&data[3])?,
        liquidity_bid_price: required_f64(&data[4])?,
        reference_price: required_f64(&data[5])?,
        liquidity_ask_price: required_f64(&data[6])?,
        liquidity_ask_size: required_i32(&data[7])?,
    })
}

fn require_length(data: &[Value], expected: usize) -> Result<(), TiingoError> {
    if data.len() != expected {
        return Err(protocol_error("market message data has the wrong length"));
    }
    Ok(())
}

fn required_string(value: &Value) -> Result<String, TiingoError> {
    value
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| protocol_error("market message field must be a string"))
}

fn required_i64(value: &Value) -> Result<i64, TiingoError> {
    value
        .as_i64()
        .ok_or_else(|| protocol_error("market message field must be an integer"))
}

fn required_i32(value: &Value) -> Result<i32, TiingoError> {
    required_i64(value)?
        .try_into()
        .map_err(|_| protocol_error("market message integer is out of range"))
}

fn required_f64(value: &Value) -> Result<f64, TiingoError> {
    let value = value
        .as_f64()
        .filter(|value| value.is_finite())
        .ok_or_else(|| protocol_error("market message field must be a finite number"))?;
    Ok(value)
}

fn optional_i32(value: &Value) -> Result<Option<i32>, TiingoError> {
    if value.is_null() {
        return Ok(None);
    }
    required_i32(value).map(Some)
}

fn optional_f64(value: &Value) -> Result<Option<f64>, TiingoError> {
    if value.is_null() {
        return Ok(None);
    }
    required_f64(value).map(Some)
}

fn protocol_error(reason: &'static str) -> TiingoError {
    TiingoError::WebSocketProtocol { reason }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionCommand {
    event_name: SubscriptionAction,
    authorization: Authorization,
    event_data: EventData,
}

impl fmt::Debug for SubscriptionCommand {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SubscriptionCommand")
            .field("event_name", &self.event_name)
            .field("credential", &self.authorization)
            .field("event_data", &self.event_data)
            .finish()
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(untagged)]
enum EventData {
    Initial(InitialEventData),
    Update(UpdateEventData),
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct InitialEventData {
    threshold_level: u8,
    tickers: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct UpdateEventData {
    subscription_id: SubscriptionId,
    tickers: Vec<String>,
}
