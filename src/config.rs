use std::{env, time::Duration};

use url::Url;

pub const MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct RetryPolicy {
    pub max_attempts: u8,
    pub base_delay: Duration,
    pub max_delay: Duration,
    pub jitter_max: Duration,
}

impl RetryPolicy {
    pub fn production() -> Self {
        Self {
            max_attempts: 3,
            base_delay: Duration::from_millis(200),
            max_delay: Duration::from_secs(30),
            jitter_max: Duration::from_millis(250),
        }
    }

    pub fn test() -> Self {
        Self {
            max_attempts: 3,
            base_delay: Duration::ZERO,
            max_delay: Duration::ZERO,
            jitter_max: Duration::ZERO,
        }
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        if !(1..=3).contains(&self.max_attempts) {
            anyhow::bail!("retry policy must allow between one and three attempts");
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct Config {
    pub api_key: Option<String>,
    pub base_url: Url,
    pub request_timeout: Duration,
    pub retry: RetryPolicy,
    pub max_response_bytes: usize,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        Ok(Self {
            api_key: env::var("TIINGO_API_KEY")
                .ok()
                .filter(|value| !value.is_empty()),
            base_url: Url::parse("https://api.tiingo.com")?,
            request_timeout: Duration::from_secs(30),
            retry: RetryPolicy::production(),
            max_response_bytes: MAX_RESPONSE_BYTES,
        })
    }
}
