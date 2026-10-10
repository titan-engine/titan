use bevy_remote::{error_codes, BrpError, BrpResult};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;

pub(crate) fn invalid(message: impl ToString) -> BrpError {
    BrpError {
        code: error_codes::INVALID_PARAMS,
        message: message.to_string(),
        data: None,
    }
}

pub(crate) fn params<T: DeserializeOwned>(value: Option<Value>) -> BrpResult<T> {
    let value = value
        .filter(|v| !v.is_null())
        .unwrap_or_else(|| serde_json::json!({}));
    if !value.is_object() {
        return Err(invalid("Expected a parameter object"));
    }
    serde_json::from_value(value).map_err(invalid)
}

pub(crate) fn default_limit() -> usize {
    64
}

pub(crate) fn check_limit(limit: usize) -> BrpResult<()> {
    if !(1..=256).contains(&limit) {
        return Err(invalid("limit must be an integer in 1..=256"));
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ListParams {
    #[serde(default = "default_limit")]
    pub limit: usize,
}

/// A bounded list. `total` always describes the full list before truncation.
#[derive(Serialize)]
pub(crate) struct Page<T> {
    items: Vec<T>,
    total: usize,
    truncated: bool,
}

pub(crate) fn page<T>(mut items: Vec<T>, limit: usize) -> Page<T> {
    let total = items.len();
    items.truncate(limit);
    page_with_total(items, total)
}

pub(crate) fn page_with_total<T>(items: Vec<T>, total: usize) -> Page<T> {
    Page {
        truncated: total > items.len(),
        items,
        total,
    }
}

pub(crate) fn names(mut items: Vec<String>, limit: usize) -> Page<String> {
    items.sort();
    items.dedup();
    page(items, limit)
}

pub(crate) fn value(value: impl Serialize) -> BrpResult {
    serde_json::to_value(value).map_err(|error| BrpError {
        code: error_codes::INTERNAL_ERROR,
        message: error.to_string(),
        data: None,
    })
}
