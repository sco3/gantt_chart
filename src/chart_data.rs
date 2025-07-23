use serde::{Deserialize, Serialize};
use chrono::NaiveDate;
use crate::item_data::ItemData;

#[derive(Deserialize, Serialize, Debug)]
pub struct ChartData {
    pub title: String,
    #[serde(rename = "markedDate")]
    pub marked_date: Option<NaiveDate>,
    pub resources: Vec<String>,
    pub items: Vec<ItemData>,
}