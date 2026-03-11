use std::fmt::{Display, Formatter};
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::model::ThreadDetail;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExportFormat {
    Json,
    Markdown,
    PromptPack,
}

impl Display for ExportFormat {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let value = match self {
            ExportFormat::Json => "json",
            ExportFormat::Markdown => "markdown",
            ExportFormat::PromptPack => "prompt-pack",
        };

        f.write_str(value)
    }
}

impl FromStr for ExportFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "json" => Ok(ExportFormat::Json),
            "markdown" => Ok(ExportFormat::Markdown),
            "prompt-pack" => Ok(ExportFormat::PromptPack),
            other => Err(format!(
                "invalid export format `{other}`; expected json|markdown|prompt-pack"
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExportDocument {
    pub format: ExportFormat,
    pub thread: ThreadDetail,
}

impl ExportDocument {
    pub fn new(format: ExportFormat, thread: ThreadDetail) -> Self {
        Self { format, thread }
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;
    use crate::model::{ThreadDetail, ThreadSummary};

    #[test]
    fn export_format_round_trips_with_serde_and_str() {
        let format = ExportFormat::PromptPack;
        let json = serde_json::to_string(&format).expect("serialize");
        assert_eq!(json, "\"prompt-pack\"");
        let decoded: ExportFormat = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded, format);
        assert_eq!(
            "prompt-pack".parse::<ExportFormat>().expect("parse"),
            format
        );
        assert_eq!(format.to_string(), "prompt-pack");
    }

    #[test]
    fn export_document_wraps_thread_detail() {
        let thread = ThreadDetail {
            summary: ThreadSummary {
                thread_id: "thr_export".into(),
                name: Some("Export me".into()),
                preview: None,
                created_at: chrono::Utc.with_ymd_and_hms(2026, 3, 11, 11, 0, 0).unwrap(),
                updated_at: None,
                cwd: None,
                source_kind: Some("local".into()),
                model_provider: Some("openai".into()),
                ephemeral: Some(false),
                status: Some("completed".into()),
            },
            turns: Vec::new(),
            items_count: 0,
            commands_count: 0,
            files_changed_count: 0,
        };

        let export = ExportDocument::new(ExportFormat::Json, thread.clone());
        let json = serde_json::to_value(&export).expect("serialize");
        assert_eq!(json["format"], "json");
        assert_eq!(json["thread"]["thread_id"], "thr_export");

        let decoded: ExportDocument = serde_json::from_value(json).expect("deserialize");
        assert_eq!(decoded.thread, thread);
        assert_eq!(decoded.format, ExportFormat::Json);
    }
}
