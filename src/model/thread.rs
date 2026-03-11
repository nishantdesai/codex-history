use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::model::Turn;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreadSummary {
    pub thread_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
    pub created_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ephemeral: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThreadDetail {
    #[serde(flatten)]
    pub summary: ThreadSummary,
    #[serde(default)]
    pub turns: Vec<Turn>,
    #[serde(default)]
    pub items_count: usize,
    #[serde(default)]
    pub commands_count: usize,
    #[serde(default)]
    pub files_changed_count: usize,
}

impl From<ThreadDetail> for ThreadSummary {
    fn from(detail: ThreadDetail) -> Self {
        detail.summary
    }
}

impl From<&ThreadDetail> for ThreadSummary {
    fn from(detail: &ThreadDetail) -> Self {
        detail.summary.clone()
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn sample_summary() -> ThreadSummary {
        ThreadSummary {
            thread_id: "thr_123".into(),
            name: Some("Fix parser".into()),
            preview: Some("Investigating strict argv parsing".into()),
            created_at: Utc.with_ymd_and_hms(2026, 3, 11, 9, 30, 0).unwrap(),
            updated_at: Some(Utc.with_ymd_and_hms(2026, 3, 11, 9, 45, 0).unwrap()),
            cwd: Some(PathBuf::from("/workspace/project")),
            source_kind: Some("local".into()),
            model_provider: Some("openai".into()),
            ephemeral: Some(false),
            status: Some("completed".into()),
        }
    }

    #[test]
    fn thread_summary_round_trips_with_serde() {
        let summary = sample_summary();
        let json = serde_json::to_string(&summary).expect("serialize");
        let decoded: ThreadSummary = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded, summary);
    }

    #[test]
    fn thread_detail_converts_to_summary() {
        let detail = ThreadDetail {
            summary: sample_summary(),
            turns: Vec::new(),
            items_count: 3,
            commands_count: 1,
            files_changed_count: 2,
        };

        let owned_summary: ThreadSummary = detail.clone().into();
        let borrowed_summary: ThreadSummary = (&detail).into();

        assert_eq!(owned_summary, detail.summary);
        assert_eq!(borrowed_summary, detail.summary);
    }
}
