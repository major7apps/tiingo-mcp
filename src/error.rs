use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum TiingoError {
    #[error("configuration error: {0}")]
    Configuration(String),
    #[error("invalid request: {0}")]
    Validation(String),
    #[error("Tiingo rejected the API credential while requesting {capability}")]
    Authentication { capability: &'static str },
    #[error("account entitlement required for {capability}")]
    Entitlement { capability: &'static str },
    #[error("resource not found for {capability}")]
    NotFound { capability: &'static str },
    #[error("Tiingo rate limit reached while requesting {capability}")]
    RateLimit { capability: &'static str },
    #[error("transient Tiingo failure ({status}) while requesting {capability}")]
    Transient {
        capability: &'static str,
        status: u16,
    },
    #[error("Tiingo request timed out while requesting {capability}")]
    Timeout { capability: &'static str },
    #[error("Tiingo transport failed while requesting {capability}")]
    Transport { capability: &'static str },
    #[error("Tiingo returned invalid JSON for {capability}")]
    Decode { capability: &'static str },
    #[error("Tiingo response for {capability} exceeded {limit} bytes")]
    ResponseTooLarge {
        capability: &'static str,
        limit: usize,
    },
    #[error("Tiingo returned HTTP {status} for {capability}")]
    Upstream {
        capability: &'static str,
        status: u16,
        detail: String,
    },
}

#[derive(Debug, Serialize)]
pub struct ErrorPayload<'a> {
    pub kind: &'a str,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_code: Option<u16>,
}

impl TiingoError {
    pub fn status_is_retryable(status: u16) -> bool {
        matches!(status, 429 | 502 | 503 | 504)
    }

    pub fn from_status(capability: &'static str, status: u16, detail: String) -> Self {
        match status {
            401 => Self::Authentication { capability },
            403 => Self::Entitlement { capability },
            404 => Self::NotFound { capability },
            429 => Self::RateLimit { capability },
            502..=504 => Self::Transient { capability, status },
            _ => Self::Upstream {
                capability,
                status,
                detail: sanitize_detail(&detail, 512),
            },
        }
    }

    pub fn payload(&self) -> ErrorPayload<'static> {
        match self {
            Self::Configuration(_) => ErrorPayload {
                kind: "configuration",
                message:
                    "Tiingo is not configured. Set TIINGO_API_KEY before calling Tiingo tools."
                        .into(),
                status_code: None,
            },
            Self::Validation(detail) => ErrorPayload {
                kind: "validation",
                message: format!("{detail}. Correct the request and try again."),
                status_code: None,
            },
            Self::Authentication { capability } => payload(
                "authentication",
                format!(
                    "Tiingo rejected the credential for {capability}. Verify TIINGO_API_KEY and try again."
                ),
                Some(401),
            ),
            Self::Entitlement { capability } => payload(
                "entitlement",
                format!(
                    "Your Tiingo account is not entitled to {capability}. Choose an available capability or update account access."
                ),
                Some(403),
            ),
            Self::NotFound { capability } => payload(
                "not_found",
                format!(
                    "Tiingo could not find data for {capability}. Check the requested identifier and try again."
                ),
                Some(404),
            ),
            Self::RateLimit { capability } => payload(
                "rate_limit",
                format!(
                    "Tiingo rate limited {capability}. Wait briefly, then retry with a narrower request."
                ),
                Some(429),
            ),
            Self::Transient { capability, status } => payload(
                "transient",
                format!(
                    "Tiingo temporarily failed while requesting {capability}. Retry the request shortly."
                ),
                Some(*status),
            ),
            Self::Timeout { capability } => payload(
                "timeout",
                format!(
                    "Tiingo timed out while requesting {capability}. Retry with a narrower request."
                ),
                None,
            ),
            Self::Transport { capability } => payload(
                "transport",
                format!(
                    "Tiingo could not be reached for {capability}. Check connectivity and retry."
                ),
                None,
            ),
            Self::Decode { capability } => payload(
                "decode",
                format!("Tiingo returned an unreadable response for {capability}. Retry shortly."),
                None,
            ),
            Self::ResponseTooLarge { capability, limit } => payload(
                "response_too_large",
                format!(
                    "Tiingo response for {capability} exceeded {limit} bytes. Narrow dates, tickers, or limits and retry."
                ),
                None,
            ),
            Self::Upstream {
                capability,
                status,
                detail,
            } => {
                let detail = sanitize_detail(detail, 512);
                let suffix = (!detail.is_empty()).then(|| format!(" Details: {detail}"));
                payload(
                    "upstream",
                    format!(
                        "Tiingo returned HTTP {status} for {capability}. Retry later or adjust the request.{}",
                        suffix.unwrap_or_default()
                    ),
                    Some(*status),
                )
            }
        }
    }
}

fn payload(kind: &'static str, message: String, status_code: Option<u16>) -> ErrorPayload<'static> {
    ErrorPayload {
        kind,
        message,
        status_code,
    }
}

pub(crate) fn sanitize_detail(detail: &str, max_chars: usize) -> String {
    let filtered = detail
        .lines()
        .map(|line| {
            if line.to_ascii_lowercase().contains("authorization") {
                "authorization: [REDACTED]"
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    filtered.chars().take(max_chars).collect()
}
