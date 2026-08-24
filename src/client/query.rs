use crate::error::TiingoError;

#[derive(Clone, Copy, Debug, serde::Deserialize, serde::Serialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum EodResample {
    Daily,
    Weekly,
    Monthly,
    Annually,
}

impl EodResample {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Daily => "daily",
            Self::Weekly => "weekly",
            Self::Monthly => "monthly",
            Self::Annually => "annually",
        }
    }
}

#[derive(Clone, Copy, Debug, serde::Deserialize, serde::Serialize, schemars::JsonSchema)]
pub enum IntradayResample {
    #[serde(rename = "1min")]
    OneMinute,
    #[serde(rename = "5min")]
    FiveMinutes,
    #[serde(rename = "15min")]
    FifteenMinutes,
    #[serde(rename = "30min")]
    ThirtyMinutes,
    #[serde(rename = "1hour")]
    OneHour,
    #[serde(rename = "1day")]
    OneDay,
}

impl IntradayResample {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OneMinute => "1min",
            Self::FiveMinutes => "5min",
            Self::FifteenMinutes => "15min",
            Self::ThirtyMinutes => "30min",
            Self::OneHour => "1hour",
            Self::OneDay => "1day",
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct DateRange {
    pub start_date: Option<chrono::NaiveDate>,
    pub end_date: Option<chrono::NaiveDate>,
}

impl DateRange {
    pub fn append(self, query: &mut Vec<(&'static str, String)>) {
        if let Some(value) = self.start_date {
            query.push(("startDate", value.format("%Y-%m-%d").to_string()));
        }
        if let Some(value) = self.end_date {
            query.push(("endDate", value.format("%Y-%m-%d").to_string()));
        }
    }
}

pub fn validate_path_segment(value: &str) -> Result<(), TiingoError> {
    if value.is_empty()
        || !value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-' | ':')
        })
    {
        return Err(TiingoError::Validation(
            "ticker must contain only ASCII letters, digits, '.', '_', '-', or ':'".to_owned(),
        ));
    }
    Ok(())
}
