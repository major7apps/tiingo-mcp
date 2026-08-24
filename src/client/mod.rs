use std::time::{Duration, SystemTime};

use bytes::BytesMut;
use futures_util::StreamExt;
use reqwest::{
    Client, StatusCode,
    header::{AUTHORIZATION, HeaderMap, HeaderValue, RETRY_AFTER},
};
use serde_json::Value;
use url::Url;

use crate::{
    config::Config,
    error::{TiingoError, sanitize_detail},
};

pub mod corporate_actions;
pub mod crypto;
pub mod eod;
pub mod forex;
pub mod fundamentals;
pub mod iex;
pub mod news;
pub mod query;

#[derive(Clone, Debug)]
pub struct TiingoClient {
    http: Client,
    config: Config,
}

impl TiingoClient {
    pub fn new(config: Config) -> anyhow::Result<Self> {
        config.retry.validate()?;
        let http = Client::builder().timeout(config.request_timeout).build()?;
        Ok(Self { http, config })
    }

    pub fn from_env() -> anyhow::Result<Self> {
        Self::new(Config::from_env()?)
    }

    pub async fn get_json(
        &self,
        capability: &'static str,
        path: &str,
        query: &[(&str, String)],
    ) -> Result<Value, TiingoError> {
        let url = self
            .config
            .base_url
            .join(path)
            .map_err(|_| TiingoError::Validation("invalid Tiingo route".to_owned()))?;
        if url.origin() != self.config.base_url.origin() {
            return Err(TiingoError::Validation(
                "Tiingo route must use the configured origin".to_owned(),
            ));
        }
        let api_key = self.config.api_key.as_deref().ok_or_else(|| {
            TiingoError::Configuration("set TIINGO_API_KEY before calling Tiingo tools".to_owned())
        })?;
        let mut authorization =
            HeaderValue::from_str(&format!("Token {api_key}")).map_err(|_| {
                TiingoError::Configuration("TIINGO_API_KEY contains invalid characters".to_owned())
            })?;
        authorization.set_sensitive(true);
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, authorization);

        self.send_with_retry(capability, url, headers, query, api_key)
            .await
    }

    async fn send_with_retry(
        &self,
        capability: &'static str,
        url: Url,
        headers: HeaderMap,
        query: &[(&str, String)],
        api_key: &str,
    ) -> Result<Value, TiingoError> {
        let mut last_error = None;
        for attempt in 1..=self.config.retry.max_attempts {
            match self
                .send_once(capability, url.clone(), headers.clone(), query, api_key)
                .await
            {
                Ok(value) => return Ok(value),
                Err(failure) if failure.retryable && attempt < self.config.retry.max_attempts => {
                    let delay = retry_delay(failure.retry_after, self.backoff(attempt));
                    last_error = Some(failure.error);
                    tokio::time::sleep(delay).await;
                }
                Err(failure) => return Err(failure.error),
            }
        }
        Err(last_error.expect("RetryPolicy::validate rejects zero attempts"))
    }

    fn backoff(&self, attempt: u8) -> Duration {
        let exponent = u32::from(attempt.saturating_sub(1));
        let base = self
            .config
            .retry
            .base_delay
            .saturating_mul(1_u32 << exponent);
        let jitter_limit = self.config.retry.jitter_max.as_millis() as u64;
        let jitter = Duration::from_millis(rand::random_range(0..=jitter_limit));
        base.saturating_add(jitter).min(self.config.retry.max_delay)
    }

    async fn send_once(
        &self,
        capability: &'static str,
        url: Url,
        headers: HeaderMap,
        query: &[(&str, String)],
        api_key: &str,
    ) -> Result<Value, AttemptFailure> {
        let response = self
            .http
            .get(url)
            .headers(headers)
            .query(query)
            .send()
            .await
            .map_err(|error| AttemptFailure::transport(capability, error))?;
        let status = response.status();
        let retry_after = response
            .headers()
            .get(RETRY_AFTER)
            .and_then(parse_retry_after);

        if !status.is_success() {
            let detail = bounded_error_text(response, api_key, 512).await;
            tracing::warn!(
                capability,
                status = status.as_u16(),
                detail,
                "Tiingo request failed"
            );
            return Err(AttemptFailure::status(
                capability,
                status,
                detail,
                retry_after,
            ));
        }

        let mut body = BytesMut::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|error| AttemptFailure::transport(capability, error))?;
            if body.len().saturating_add(chunk.len()) > self.config.max_response_bytes {
                return Err(AttemptFailure::terminal(TiingoError::ResponseTooLarge {
                    capability,
                    limit: self.config.max_response_bytes,
                }));
            }
            body.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&body)
            .map_err(|_| AttemptFailure::terminal(TiingoError::Decode { capability }))
    }
}

struct AttemptFailure {
    error: TiingoError,
    retryable: bool,
    retry_after: Option<Duration>,
}

impl AttemptFailure {
    fn terminal(error: TiingoError) -> Self {
        Self {
            error,
            retryable: false,
            retry_after: None,
        }
    }

    fn transport(capability: &'static str, error: reqwest::Error) -> Self {
        let tiingo_error = if error.is_timeout() {
            TiingoError::Timeout { capability }
        } else {
            TiingoError::Transport { capability }
        };
        Self {
            error: tiingo_error,
            retryable: error.is_timeout() || error.is_connect(),
            retry_after: None,
        }
    }

    fn status(
        capability: &'static str,
        status: StatusCode,
        detail: String,
        retry_after: Option<Duration>,
    ) -> Self {
        Self {
            retryable: TiingoError::status_is_retryable(status.as_u16()),
            error: TiingoError::from_status(capability, status.as_u16(), detail),
            retry_after,
        }
    }
}

fn parse_retry_after(value: &HeaderValue) -> Option<Duration> {
    let text = value.to_str().ok()?;
    if let Ok(seconds) = text.parse::<u64>() {
        return Some(Duration::from_secs(seconds));
    }
    httpdate::parse_http_date(text)
        .ok()?
        .duration_since(SystemTime::now())
        .ok()
}

fn retry_delay(retry_after: Option<Duration>, fallback: Duration) -> Duration {
    retry_after.unwrap_or(fallback)
}

async fn bounded_error_text(
    response: reqwest::Response,
    api_key: &str,
    max_chars: usize,
) -> String {
    let max_bytes = max_chars.saturating_mul(4);
    let mut bytes = BytesMut::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let Ok(chunk) = chunk else { break };
        let remaining = max_bytes.saturating_sub(bytes.len());
        if remaining == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..chunk.len().min(remaining)]);
    }
    let text = String::from_utf8_lossy(&bytes).replace(api_key, "[REDACTED]");
    sanitize_detail(&text, max_chars)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_retry_after_is_not_capped() {
        let retry_after = parse_retry_after(&HeaderValue::from_static("60"));
        assert_eq!(
            retry_delay(retry_after, Duration::ZERO),
            Duration::from_secs(60)
        );
    }
}
