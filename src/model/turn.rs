use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::model::Item;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Turn {
    pub turn_id: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub items: Vec<Item>,
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;
    use crate::model::{Item, MessageItem};

    #[test]
    fn turn_round_trips_with_nested_items() {
        let turn = Turn {
            turn_id: "turn_123".into(),
            status: "completed".into(),
            started_at: Some(Utc.with_ymd_and_hms(2026, 3, 11, 10, 0, 0).unwrap()),
            completed_at: Some(Utc.with_ymd_and_hms(2026, 3, 11, 10, 1, 0).unwrap()),
            items: vec![Item::UserMessage(MessageItem {
                text: Some("show me the diff".into()),
                attributes: Default::default(),
            })],
        };

        let json = serde_json::to_string(&turn).expect("serialize");
        let decoded: Turn = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded, turn);
    }
}
