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

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize, schemars::JsonSchema)]
#[serde(try_from = "String", into = "String")]
pub enum IntradayResample {
    OneMinute,
    FiveMinutes,
    FifteenMinutes,
    ThirtyMinutes,
    OneHour,
    OneDay,
    Custom(String),
}

impl IntradayResample {
    /// Validate the supported interval, including manually constructed custom variants.
    pub fn validate(&self) -> Result<(), TiingoError> {
        validate_frequency(self.as_str(), true).map_err(TiingoError::Validation)
    }

    /// Reject day multiples for market endpoints while preserving the legacy one-day interval.
    pub fn validate_market_interval(&self) -> Result<(), TiingoError> {
        self.validate()?;
        if self.as_str().ends_with("day") && self.as_str() != "1day" {
            return Err(TiingoError::Validation(
                "this market endpoint accepts min or hour intervals; legacy 1day remains supported"
                    .into(),
            ));
        }
        Ok(())
    }

    /// Return the interval string used in Tiingo request parameters.
    pub fn as_str(&self) -> &str {
        match self {
            Self::OneMinute => "1min",
            Self::FiveMinutes => "5min",
            Self::FifteenMinutes => "15min",
            Self::ThirtyMinutes => "30min",
            Self::OneHour => "1hour",
            Self::OneDay => "1day",
            Self::Custom(value) => value,
        }
    }
}

impl TryFrom<String> for IntradayResample {
    type Error = String;

    /// Parse a canonical interval while preserving the existing named variants.
    fn try_from(value: String) -> Result<Self, Self::Error> {
        validate_frequency(&value, true)?;
        Ok(match value.as_str() {
            "1min" => Self::OneMinute,
            "5min" => Self::FiveMinutes,
            "15min" => Self::FifteenMinutes,
            "30min" => Self::ThirtyMinutes,
            "1hour" => Self::OneHour,
            "1day" => Self::OneDay,
            _ => Self::Custom(value),
        })
    }
}

impl From<IntradayResample> for String {
    /// Return the stored Tiingo interval string without changing its spelling.
    fn from(value: IntradayResample) -> Self {
        value.as_str().to_owned()
    }
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize, schemars::JsonSchema)]
#[serde(try_from = "String", into = "String")]
pub enum IexResample {
    OneMinute,
    FiveMinutes,
    FifteenMinutes,
    ThirtyMinutes,
    OneHour,
    Custom(String),
}

impl IexResample {
    /// Validate the supported interval, including manually constructed custom variants.
    pub fn validate(&self) -> Result<(), TiingoError> {
        validate_frequency(self.as_str(), false).map_err(TiingoError::Validation)
    }

    /// Return the interval string used in Tiingo request parameters.
    pub fn as_str(&self) -> &str {
        match self {
            Self::OneMinute => "1min",
            Self::FiveMinutes => "5min",
            Self::FifteenMinutes => "15min",
            Self::ThirtyMinutes => "30min",
            Self::OneHour => "1hour",
            Self::Custom(value) => value,
        }
    }
}

impl TryFrom<String> for IexResample {
    type Error = String;

    /// Parse a canonical interval while preserving the existing named variants.
    fn try_from(value: String) -> Result<Self, Self::Error> {
        validate_frequency(&value, false)?;
        Ok(match value.as_str() {
            "1min" => Self::OneMinute,
            "5min" => Self::FiveMinutes,
            "15min" => Self::FifteenMinutes,
            "30min" => Self::ThirtyMinutes,
            "1hour" => Self::OneHour,
            _ => Self::Custom(value),
        })
    }
}

impl From<IexResample> for String {
    /// Return the stored Tiingo interval string without changing its spelling.
    fn from(value: IexResample) -> Self {
        value.as_str().to_owned()
    }
}

/// Require a positive u32 interval with a supported unit and no leading zeroes.
fn validate_frequency(value: &str, allow_days: bool) -> Result<(), String> {
    let number = value
        .strip_suffix("min")
        .or_else(|| value.strip_suffix("hour"))
        .or_else(|| allow_days.then(|| value.strip_suffix("day")).flatten());
    if matches!(number, Some(number) if !number.starts_with('0')
        && number.bytes().all(|byte| byte.is_ascii_digit())
        && number.parse::<u32>().is_ok_and(|number| number > 0))
    {
        return Ok(());
    }
    Err(if allow_days {
        "resample_freq must be a positive integer followed by min, hour, or day"
    } else {
        "resample_freq must be a positive integer followed by min or hour"
    }
    .to_owned())
}

#[derive(Clone, Copy, Debug, serde::Deserialize, serde::Serialize, schemars::JsonSchema)]
pub enum NewsSort {
    #[serde(rename = "crawlDate")]
    CrawlDate,
    #[serde(rename = "publishedDate")]
    PublishedDate,
}

impl NewsSort {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CrawlDate => "crawlDate",
            Self::PublishedDate => "publishedDate",
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct NewsQuery {
    pub tickers: Option<String>,
    pub tags: Option<String>,
    pub source: Option<String>,
    pub start_date: Option<chrono::NaiveDate>,
    pub end_date: Option<chrono::NaiveDate>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
    pub sort_by: Option<NewsSort>,
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
        || matches!(value, "." | "..")
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

pub fn validate_column_list(columns: Option<&[String]>) -> Result<Option<String>, TiingoError> {
    let Some(columns) = columns else {
        return Ok(None);
    };
    if columns.is_empty() || columns.len() > 32 {
        return Err(TiingoError::Validation(
            "columns must contain between 1 and 32 identifiers".to_owned(),
        ));
    }
    if columns.iter().any(|column| {
        let mut characters = column.chars();
        !matches!(characters.next(), Some(character) if character.is_ascii_alphabetic() || character == '_')
            || !characters.all(|character| character.is_ascii_alphanumeric() || character == '_')
    }) {
        return Err(TiingoError::Validation(
            "columns must contain only ASCII identifier names".to_owned(),
        ));
    }
    Ok(Some(columns.join(",")))
}

pub fn normalize_symbol_list(values: &[String]) -> Result<String, TiingoError> {
    if values.is_empty() || values.len() > 100 {
        return Err(TiingoError::Validation(
            "tickers must contain between 1 and 100 values".to_owned(),
        ));
    }
    let mut normalized = Vec::with_capacity(values.len());
    for value in values {
        let value = value.trim();
        validate_path_segment(value)?;
        normalized.push(value.to_ascii_lowercase());
    }
    Ok(normalized.join(","))
}

pub fn validate_ticker_metadata_columns(columns: &[String]) -> Result<(), TiingoError> {
    if columns.is_empty() || columns.len() > 32 {
        return Err(TiingoError::Validation(
            "columns must contain between 1 and 32 ticker metadata fields".to_owned(),
        ));
    }
    if columns.iter().any(|column| {
        !matches!(
            column.as_str(),
            "ticker"
                | "permaTicker"
                | "name"
                | "openfigi"
                | "exchange"
                | "assetType"
                | "isActive"
                | "startDate"
                | "endDate"
        )
    }) {
        return Err(TiingoError::Validation(
            "columns contain an unsupported ticker metadata field".to_owned(),
        ));
    }
    Ok(())
}

/// Validate a sortable response-field identifier, optionally prefixed with `-`.
pub fn validate_sort(sort: Option<&str>) -> Result<Option<String>, TiingoError> {
    let Some(sort) = sort else {
        return Ok(None);
    };
    let field = sort.strip_prefix('-').unwrap_or(sort);
    let mut chars = field.chars();
    if !matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        || !chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return Err(TiingoError::Validation(
            "sort must be a field identifier with an optional leading '-'".into(),
        ));
    }
    Ok(Some(sort.to_owned()))
}

/// Validate one to 100 exchange identifiers while preserving identifier case and order.
pub fn validate_exchange_list(exchanges: Option<&[String]>) -> Result<Option<String>, TiingoError> {
    let Some(exchanges) = exchanges else {
        return Ok(None);
    };
    if exchanges.is_empty() || exchanges.len() > 100 {
        return Err(TiingoError::Validation(
            "exchanges must contain between 1 and 100 identifiers".into(),
        ));
    }
    for exchange in exchanges {
        validate_path_segment(exchange)?;
    }
    Ok(Some(exchanges.join(",")))
}
