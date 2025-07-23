use serde::{Deserialize, Serialize};
use chrono::{NaiveDate, NaiveDateTime};

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct ItemData {
    pub title: String,
    
    pub duration: Option<i64>,

    #[serde(rename = "durationMs", skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<i64>,

    #[serde(rename = "startMs", skip_serializing_if = "Option::is_none")]
    pub start_ms: Option<i64>, // For Unix timestamp in milliseconds

    #[serde(rename = "startDate", skip_serializing_if = "Option::is_none")]
    pub start_date: Option<NaiveDateTime>,
    
    
    
    #[serde(rename = "resource")]
    pub resource_index: Option<usize>,
    pub open: Option<bool>,
}