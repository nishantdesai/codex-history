use std::fmt::{Display, Formatter};
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::model::{Item, ThreadDetail};

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

pub fn render_thread_export(format: ExportFormat, thread: &ThreadDetail) -> Result<String, String> {
    match format {
        ExportFormat::Json => {
            serde_json::to_string_pretty(&ExportDocument::new(format, thread.clone()))
                .map_err(|error| format!("failed to serialize export JSON: {error}"))
        }
        ExportFormat::Markdown => Ok(render_markdown_export(thread)),
        ExportFormat::PromptPack => Ok(render_prompt_pack_export(thread)),
    }
}

fn render_markdown_export(thread: &ThreadDetail) -> String {
    let mut lines = vec!["# Thread Export".to_string(), String::new()];

    push_section(&mut lines, "Metadata");
    lines.push(format!("- Thread ID: {}", thread.summary.thread_id));
    lines.push(format!(
        "- Name: {}",
        thread
            .summary
            .name
            .as_deref()
            .map(compact_text)
            .unwrap_or_else(|| "(unnamed)".into())
    ));
    lines.push(format!(
        "- Preview: {}",
        thread
            .summary
            .preview
            .as_deref()
            .map(compact_text)
            .unwrap_or_else(|| "(none)".into())
    ));
    lines.push(format!(
        "- Created At: {}",
        thread.summary.created_at.to_rfc3339()
    ));
    lines.push(format!(
        "- Updated At: {}",
        thread
            .summary
            .updated_at
            .unwrap_or(thread.summary.created_at)
            .to_rfc3339()
    ));
    if let Some(status) = &thread.summary.status {
        lines.push(format!("- Status: {status}"));
    }
    if let Some(cwd) = &thread.summary.cwd {
        lines.push(format!("- Cwd: `{}`", cwd.display()));
    }
    if let Some(source_kind) = &thread.summary.source_kind {
        lines.push(format!("- Source Kind: {source_kind}"));
    }
    if let Some(model_provider) = &thread.summary.model_provider {
        lines.push(format!("- Model Provider: {model_provider}"));
    }
    if let Some(ephemeral) = thread.summary.ephemeral {
        lines.push(format!("- Ephemeral: {ephemeral}"));
    }
    lines.push(format!("- Turns: {}", thread.turns.len()));
    lines.push(format!("- Items: {}", thread.items_count));
    lines.push(format!("- Commands: {}", thread.commands_count));
    lines.push(format!("- Files Changed: {}", thread.files_changed_count));
    lines.push(String::new());

    push_section(&mut lines, "Turns");
    if thread.turns.is_empty() {
        lines.push("- (none)".to_string());
        lines.push(String::new());
    } else {
        for turn in &thread.turns {
            lines.push(format!("### {}", turn.turn_id));
            lines.push(format!("- Status: {}", turn.status));
            if let Some(started_at) = turn.started_at {
                lines.push(format!("- Started At: {}", started_at.to_rfc3339()));
            }
            if let Some(completed_at) = turn.completed_at {
                lines.push(format!("- Completed At: {}", completed_at.to_rfc3339()));
            }
            if turn.items.is_empty() {
                lines.push("- Item: (none)".to_string());
            } else {
                for item in &turn.items {
                    lines.extend(render_markdown_item(item));
                }
            }
            lines.push(String::new());
        }
    }

    push_section(&mut lines, "Commands");
    let commands = command_entries(thread);
    if commands.is_empty() {
        lines.push("- (none)".to_string());
    } else {
        for (index, command) in commands.iter().enumerate() {
            lines.push(format!("### Command {}", index + 1));
            lines.push(format!(
                "- Command: `{}`",
                command.command.as_deref().unwrap_or("(unknown)")
            ));
            if let Some(cwd) = &command.cwd {
                lines.push(format!("- Cwd: `{}`", cwd.display()));
            }
            if let Some(exit_code) = command.exit_code {
                lines.push(format!("- Exit Code: {exit_code}"));
            }
            if let Some(output) = &command.output {
                lines.push(format!("- Output: {}", compact_text(output)));
            }
            lines.push(String::new());
        }
    }

    push_section(&mut lines, "File Changes");
    let file_changes = file_change_entries(thread);
    if file_changes.is_empty() {
        lines.push("- (none)".to_string());
    } else {
        for (index, change) in file_changes.iter().enumerate() {
            lines.push(format!("### File Change {}", index + 1));
            if let Some(path) = &change.path {
                lines.push(format!("- Path: `{}`", path.display()));
            }
            if let Some(change_type) = &change.change_type {
                lines.push(format!("- Change Type: {change_type}"));
            }
            if let Some(summary) = &change.summary {
                lines.push(format!("- Summary: {}", compact_text(summary)));
            }
            lines.push(String::new());
        }
    }

    push_section(&mut lines, "Extracted Notes");
    let notes = extracted_notes(thread);
    if notes.is_empty() {
        lines.push("- (none)".to_string());
    } else {
        for note in notes {
            lines.push(format!("- {note}"));
        }
    }

    lines.join("\n")
}

fn render_prompt_pack_export(thread: &ThreadDetail) -> String {
    let objective = export_objective(thread).unwrap_or_else(|| "(none)".into());
    let mut lines = vec![
        "OBJECTIVE".to_string(),
        objective,
        String::new(),
        "KEY CONTEXT".to_string(),
    ];

    if let Some(name) = &thread.summary.name {
        lines.push(format!("- name: {}", compact_text(name)));
    }
    lines.push(format!("- thread_id: {}", thread.summary.thread_id));
    lines.push(format!(
        "- created_at: {}",
        thread.summary.created_at.to_rfc3339()
    ));
    lines.push(format!(
        "- updated_at: {}",
        thread
            .summary
            .updated_at
            .unwrap_or(thread.summary.created_at)
            .to_rfc3339()
    ));
    if let Some(status) = &thread.summary.status {
        lines.push(format!("- status: {status}"));
    }
    if let Some(cwd) = &thread.summary.cwd {
        lines.push(format!("- cwd: {}", cwd.display()));
    }
    if let Some(source_kind) = &thread.summary.source_kind {
        lines.push(format!("- source_kind: {source_kind}"));
    }
    if let Some(model_provider) = &thread.summary.model_provider {
        lines.push(format!("- model_provider: {model_provider}"));
    }
    lines.push(format!("- turns: {}", thread.turns.len()));
    lines.push(format!("- items: {}", thread.items_count));
    lines.push(format!("- commands: {}", thread.commands_count));
    lines.push(format!("- files_changed: {}", thread.files_changed_count));
    lines.push(String::new());

    push_prompt_pack_list(&mut lines, "COMMANDS SEEN", command_values(thread));
    push_prompt_pack_list(&mut lines, "FILES TOUCHED", file_paths(thread));
    push_prompt_pack_list(&mut lines, "NOTABLE ERRORS", notable_errors(thread));
    push_prompt_pack_list(&mut lines, "USEFUL FOLLOW-UPS", useful_follow_ups(thread));

    lines.join("\n")
}

fn push_section(lines: &mut Vec<String>, title: &str) {
    if lines.last().is_some_and(|line| !line.is_empty()) {
        lines.push(String::new());
    }
    lines.push(format!("## {title}"));
}

fn push_prompt_pack_list(lines: &mut Vec<String>, heading: &str, values: Vec<String>) {
    lines.push(heading.to_string());
    if values.is_empty() {
        lines.push("- (none)".to_string());
    } else {
        for value in values {
            lines.push(format!("- {value}"));
        }
    }
    lines.push(String::new());
}

fn render_markdown_item(item: &Item) -> Vec<String> {
    match item {
        Item::UserMessage(message) => vec![format!(
            "- User Message: {}",
            compact_optional_text(message.text.as_deref())
        )],
        Item::AgentMessage(message) => vec![format!(
            "- Agent Message: {}",
            compact_optional_text(message.text.as_deref())
        )],
        Item::CommandExecution(command) => {
            let mut lines = vec![format!(
                "- Command: `{}`",
                command.command.as_deref().unwrap_or("(unknown)")
            )];
            if let Some(cwd) = &command.cwd {
                lines.push(format!("- Command Cwd: `{}`", cwd.display()));
            }
            if let Some(exit_code) = command.exit_code {
                lines.push(format!("- Command Exit Code: {exit_code}"));
            }
            if let Some(output) = &command.output {
                lines.push(format!("- Command Output: {}", compact_text(output)));
            }
            lines
        }
        Item::FileChange(change) => {
            let mut parts = Vec::new();
            if let Some(path) = &change.path {
                parts.push(path.display().to_string());
            }
            if let Some(change_type) = &change.change_type {
                parts.push(change_type.clone());
            }
            if let Some(summary) = &change.summary {
                parts.push(compact_text(summary));
            }
            vec![format!(
                "- File Change: {}",
                if parts.is_empty() {
                    "(unknown)".to_string()
                } else {
                    parts.join(" | ")
                }
            )]
        }
        Item::ReasoningSummary(summary) => vec![format!(
            "- Reasoning Summary: {}",
            compact_optional_text(summary.text.as_deref())
        )],
        Item::WebSearch(search) => vec![format!(
            "- Web Search: {}",
            compact_optional_text(
                search
                    .query
                    .as_deref()
                    .or(search.title.as_deref())
                    .or(search.url.as_deref())
            )
        )],
        Item::McpToolCall(call) => vec![format!(
            "- MCP Tool Call: {}",
            compact_optional_text(call.tool.as_deref().or(call.server.as_deref()))
        )],
        Item::Other(other) => vec![format!("- Other Item ({}): (preserved)", other.kind)],
    }
}

fn export_objective(thread: &ThreadDetail) -> Option<String> {
    first_user_message(thread)
        .or_else(|| thread.summary.preview.clone())
        .or_else(|| thread.summary.name.clone())
        .map(|value| compact_text(&value))
}

fn first_user_message(thread: &ThreadDetail) -> Option<String> {
    for turn in &thread.turns {
        for item in &turn.items {
            if let Item::UserMessage(message) = item {
                if let Some(text) = message.text.as_ref().filter(|text| !text.trim().is_empty()) {
                    return Some(text.clone());
                }
            }
        }
    }
    None
}

fn extracted_notes(thread: &ThreadDetail) -> Vec<String> {
    let mut notes = Vec::new();
    for turn in &thread.turns {
        for item in &turn.items {
            match item {
                Item::ReasoningSummary(summary) => {
                    push_unique(&mut notes, summary.text.clone());
                }
                Item::AgentMessage(message) => {
                    push_unique(&mut notes, message.text.clone());
                }
                _ => {}
            }
        }
    }
    notes
}

fn useful_follow_ups(thread: &ThreadDetail) -> Vec<String> {
    let mut follow_ups = extracted_notes(thread);
    for turn in &thread.turns {
        if turn.status != "completed" {
            push_unique(
                &mut follow_ups,
                Some(format!("Turn {} is still {}", turn.turn_id, turn.status)),
            );
        }
    }
    follow_ups
}

fn command_entries(thread: &ThreadDetail) -> Vec<&crate::model::CommandExecutionItem> {
    let mut commands = Vec::new();
    for turn in &thread.turns {
        for item in &turn.items {
            if let Item::CommandExecution(command) = item {
                commands.push(command);
            }
        }
    }
    commands
}

fn file_change_entries(thread: &ThreadDetail) -> Vec<&crate::model::FileChangeItem> {
    let mut changes = Vec::new();
    for turn in &thread.turns {
        for item in &turn.items {
            if let Item::FileChange(change) = item {
                changes.push(change);
            }
        }
    }
    changes
}

fn command_values(thread: &ThreadDetail) -> Vec<String> {
    let mut commands = Vec::new();
    for command in command_entries(thread) {
        push_unique(&mut commands, command.command.clone());
    }
    commands
}

fn file_paths(thread: &ThreadDetail) -> Vec<String> {
    let mut paths = Vec::new();
    for change in file_change_entries(thread) {
        if let Some(path) = &change.path {
            push_unique(&mut paths, Some(path.display().to_string()));
        }
    }
    paths
}

fn notable_errors(thread: &ThreadDetail) -> Vec<String> {
    let mut errors = Vec::new();

    for command in command_entries(thread) {
        let command_text = command.command.as_deref().unwrap_or("(unknown)");
        if let Some(exit_code) = command.exit_code.filter(|exit_code| *exit_code != 0) {
            push_unique(
                &mut errors,
                Some(format!("{command_text} exited with code {exit_code}")),
            );
        }
        if let Some(output) = &command.output {
            let compact = compact_text(output);
            let lower = compact.to_ascii_lowercase();
            if ["error", "failed", "panic", "traceback", "fatal"]
                .iter()
                .any(|needle| lower.contains(needle))
            {
                push_unique(&mut errors, Some(compact));
            }
        }
    }

    errors
}

fn push_unique(values: &mut Vec<String>, value: Option<String>) {
    let Some(value) = value else {
        return;
    };
    let value = compact_text(&value);
    if value.is_empty() || values.iter().any(|existing| existing == &value) {
        return;
    }
    values.push(value);
}

fn compact_optional_text(value: Option<&str>) -> String {
    value.map(compact_text).unwrap_or_else(|| "(none)".into())
}

fn compact_text(value: &str) -> String {
    value
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" / ")
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use chrono::TimeZone;
    use serde_json::json;

    use super::*;
    use crate::model::{
        CommandExecutionItem, FileChangeItem, MessageItem, ReasoningSummaryItem, ThreadDetail,
        ThreadSummary, Turn,
    };

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

    fn sample_thread() -> ThreadDetail {
        ThreadDetail {
            summary: ThreadSummary {
                thread_id: "thr_export".into(),
                name: Some("Parser regression".into()),
                preview: Some("Please inspect the parser regression.".into()),
                created_at: chrono::Utc.with_ymd_and_hms(2026, 3, 11, 11, 0, 0).unwrap(),
                updated_at: Some(chrono::Utc.with_ymd_and_hms(2026, 3, 11, 11, 5, 0).unwrap()),
                cwd: Some(PathBuf::from("/workspace/project")),
                source_kind: Some("local".into()),
                model_provider: Some("openai".into()),
                ephemeral: Some(false),
                status: Some("completed".into()),
            },
            turns: vec![
                Turn {
                    turn_id: "turn_1".into(),
                    status: "completed".into(),
                    started_at: Some(chrono::Utc.with_ymd_and_hms(2026, 3, 11, 11, 0, 0).unwrap()),
                    completed_at: Some(
                        chrono::Utc.with_ymd_and_hms(2026, 3, 11, 11, 2, 0).unwrap(),
                    ),
                    items: vec![
                        Item::UserMessage(MessageItem {
                            text: Some("Please inspect the parser regression.".into()),
                            attributes: BTreeMap::new(),
                        }),
                        Item::CommandExecution(CommandExecutionItem {
                            command: Some("cargo test cli::tests".into()),
                            exit_code: Some(101),
                            cwd: Some(PathBuf::from("/workspace/project")),
                            output: Some("error: test failed".into()),
                            attributes: BTreeMap::from([("call_id".into(), json!("call_1"))]),
                        }),
                        Item::FileChange(FileChangeItem {
                            path: Some(PathBuf::from("src/cli/mod.rs")),
                            change_type: Some("modified".into()),
                            summary: Some("Tightened argument parsing.".into()),
                            attributes: BTreeMap::new(),
                        }),
                        Item::ReasoningSummary(ReasoningSummaryItem {
                            text: Some(
                                "Parser validation should reject leftover arguments.".into(),
                            ),
                            attributes: BTreeMap::new(),
                        }),
                    ],
                },
                Turn {
                    turn_id: "turn_2".into(),
                    status: "completed".into(),
                    started_at: Some(chrono::Utc.with_ymd_and_hms(2026, 3, 11, 11, 3, 0).unwrap()),
                    completed_at: Some(
                        chrono::Utc.with_ymd_and_hms(2026, 3, 11, 11, 5, 0).unwrap(),
                    ),
                    items: vec![Item::AgentMessage(MessageItem {
                        text: Some("Add export tests before shipping.".into()),
                        attributes: BTreeMap::new(),
                    })],
                },
            ],
            items_count: 5,
            commands_count: 1,
            files_changed_count: 1,
        }
    }

    #[test]
    fn renders_markdown_export_with_expected_sections() {
        let rendered = render_thread_export(ExportFormat::Markdown, &sample_thread())
            .expect("markdown render");

        assert!(rendered.contains("# Thread Export"));
        assert!(rendered.contains("## Metadata"));
        assert!(rendered.contains("## Turns"));
        assert!(rendered.contains("## Commands"));
        assert!(rendered.contains("## File Changes"));
        assert!(rendered.contains("## Extracted Notes"));
        assert!(rendered.contains("- Command Output: error: test failed"));
        assert!(rendered.contains("- Path: `src/cli/mod.rs`"));
        assert!(rendered.contains("- Add export tests before shipping."));
    }

    #[test]
    fn normalizes_multiline_metadata_in_markdown_export() {
        let mut thread = sample_thread();
        thread.summary.name = Some("Parser regression\nphase 2".into());
        thread.summary.preview = Some("Inspect parser regression.\nKeep markdown valid.".into());

        let rendered =
            render_thread_export(ExportFormat::Markdown, &thread).expect("markdown render");

        assert!(rendered.contains("- Name: Parser regression / phase 2"));
        assert!(rendered.contains("- Preview: Inspect parser regression. / Keep markdown valid."));
    }

    #[test]
    fn renders_prompt_pack_export_with_handoff_sections() {
        let rendered = render_thread_export(ExportFormat::PromptPack, &sample_thread())
            .expect("prompt-pack render");

        assert!(rendered.contains("OBJECTIVE"));
        assert!(rendered.contains("Please inspect the parser regression."));
        assert!(rendered.contains("COMMANDS SEEN"));
        assert!(rendered.contains("- cargo test cli::tests"));
        assert!(rendered.contains("FILES TOUCHED"));
        assert!(rendered.contains("- src/cli/mod.rs"));
        assert!(rendered.contains("NOTABLE ERRORS"));
        assert!(rendered.contains("- cargo test cli::tests exited with code 101"));
        assert!(rendered.contains("- error: test failed"));
        assert!(rendered.contains("USEFUL FOLLOW-UPS"));
        assert!(rendered.contains("- Parser validation should reject leftover arguments."));
    }
}
