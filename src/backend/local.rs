use std::collections::{BTreeMap, HashMap};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::{env, fs};

use chrono::{DateTime, Utc};
use regex::Regex;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;

use crate::model::{CommandExecutionItem, FileChangeItem, Item, ThreadDetail, ThreadSummary, Turn};
use crate::parser::jsonl::{parse_session_log_incremental, PendingCommandCall};
use crate::search_scope::SearchScope;
use crate::util::paths::{collect_session_log_files, discover_history_roots, HistoryRootCandidate};

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LocalDoctorRoot {
    pub path: PathBuf,
    pub source: String,
    pub exists: bool,
    pub session_files: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LocalDoctorReport {
    pub roots: Vec<LocalDoctorRoot>,
    pub parsed_threads: usize,
    pub malformed_files: usize,
    pub malformed_lines: usize,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GrepMatch {
    pub thread_id: String,
    pub turn_id: String,
    pub kind: String,
    pub text: String,
}

pub struct GrepReport {
    pub matches: Vec<GrepMatch>,
    pub thread_summaries: HashMap<String, ThreadSummary>,
}

pub struct LocalBackend {
    roots: Vec<HistoryRootCandidate>,
}

impl LocalBackend {
    pub fn discover() -> Self {
        Self {
            roots: discover_history_roots(),
        }
    }

    pub fn list_threads(&self) -> Result<Vec<ThreadSummary>, String> {
        let mut threads: Vec<_> = self
            .scan_threads()?
            .threads
            .into_iter()
            .map(|thread| thread.summary)
            .collect();

        threads.sort_by(|left, right| thread_sort_key(right).cmp(&thread_sort_key(left)));
        Ok(threads)
    }

    pub fn show_thread(
        &self,
        thread_id: &str,
        include_turns: bool,
    ) -> Result<Option<ThreadDetail>, String> {
        let mut thread = self
            .scan_threads()?
            .threads
            .into_iter()
            .find(|thread| thread.summary.thread_id == thread_id);

        if let Some(detail) = thread.as_mut() {
            if !include_turns {
                detail.turns.clear();
            }
        }

        Ok(thread)
    }

    pub fn list_thread_details(&self) -> Result<Vec<ThreadDetail>, String> {
        let mut threads = self.scan_threads()?.threads;
        threads.sort_by(|left, right| {
            thread_sort_key(&right.summary).cmp(&thread_sort_key(&left.summary))
        });
        Ok(threads)
    }

    pub fn grep(
        &self,
        pattern: &str,
        regex: bool,
        scope: SearchScope,
    ) -> Result<Vec<GrepMatch>, String> {
        Ok(self.grep_report(pattern, regex, scope)?.matches)
    }

    pub fn grep_report(
        &self,
        pattern: &str,
        regex: bool,
        scope: SearchScope,
    ) -> Result<GrepReport, String> {
        if scope == SearchScope::default() {
            return self.grep_report_messages_only(pattern, regex);
        }

        let matcher = if regex {
            Some(
                Regex::new(pattern)
                    .map_err(|error| format!("invalid regex `{pattern}`: {error}"))?,
            )
        } else {
            None
        };

        let scan = self.scan_threads()?;
        let mut matches = Vec::new();
        let mut thread_summaries = HashMap::new();
        for parsed in scan.threads {
            let thread_id = parsed.summary.thread_id.clone();
            thread_summaries.insert(thread_id.clone(), parsed.summary.clone());
            for turn in parsed.turns {
                for item in turn.items {
                    for text in item_texts(&item, scope) {
                        let matched = match &matcher {
                            Some(regex) => regex.is_match(&text),
                            None => text.contains(pattern),
                        };

                        if matched {
                            matches.push(GrepMatch {
                                thread_id: thread_id.clone(),
                                turn_id: turn.turn_id.clone(),
                                kind: item.kind().to_string(),
                                text,
                            });
                        }
                    }
                }
            }
        }

        Ok(GrepReport {
            matches,
            thread_summaries,
        })
    }

    pub fn doctor(&self) -> Result<LocalDoctorReport, String> {
        let scan = self.scan_threads()?;
        let mut roots = Vec::new();

        for root in &self.roots {
            let session_files = if root.exists {
                collect_session_log_files(&root.path).0.len()
            } else {
                0
            };
            roots.push(LocalDoctorRoot {
                path: root.path.clone(),
                source: root.source.clone(),
                exists: root.exists,
                session_files,
            });
        }

        Ok(LocalDoctorReport {
            roots,
            parsed_threads: scan.threads.len(),
            malformed_files: scan.malformed_files,
            malformed_lines: scan.malformed_lines,
            warnings: scan.warnings,
        })
    }

    fn scan_threads(&self) -> Result<LocalScan, String> {
        let mut session_files = Vec::new();
        let mut warnings = Vec::new();

        for root in &self.roots {
            if !root.exists {
                continue;
            }

            let (files, root_warnings) = collect_session_log_files(&root.path);
            session_files.extend(files);
            warnings.extend(root_warnings);
        }

        let mut threads_by_id = BTreeMap::new();
        let mut malformed_files = 0;
        let mut malformed_lines = 0;
        let mut pending_commands = BTreeMap::new();

        for file in session_files {
            let mut report =
                parse_session_log_incremental(&file, pending_commands.into_iter().collect())
                    .map_err(|error| format!("failed to read {}: {error}", file.display()))?;
            pending_commands = report.take_pending_commands().into_iter().collect();

            malformed_lines += report.malformed_lines;
            for warning in report.warnings {
                warnings.push(format!("{}: {}", file.display(), warning));
            }

            let Some(detail) = report.detail else {
                malformed_files += 1;
                continue;
            };

            if report.malformed_lines > 0 {
                malformed_files += 1;
            }
            match threads_by_id.get_mut(&detail.summary.thread_id) {
                Some(existing) => merge_thread_detail(existing, detail),
                None => {
                    threads_by_id.insert(detail.summary.thread_id.clone(), detail);
                }
            }
        }

        for pending in pending_commands.into_values() {
            attach_pending_command(&mut threads_by_id, pending, &mut warnings);
        }
        for thread in threads_by_id.values_mut() {
            recompute_thread_counts(thread);
        }
        apply_session_index_names(&mut threads_by_id);

        Ok(LocalScan {
            threads: threads_by_id.into_values().collect(),
            malformed_files,
            malformed_lines,
            warnings,
        })
    }

    fn grep_report_messages_only(&self, pattern: &str, regex: bool) -> Result<GrepReport, String> {
        let matcher = if regex {
            Some(
                Regex::new(pattern)
                    .map_err(|error| format!("invalid regex `{pattern}`: {error}"))?,
            )
        } else {
            None
        };

        let mut session_files = Vec::new();
        for root in &self.roots {
            if !root.exists {
                continue;
            }
            let (files, _) = collect_session_log_files(&root.path);
            session_files.extend(files);
        }
        session_files.sort();

        let mut matches = Vec::new();
        let mut thread_summaries = BTreeMap::new();

        for file in session_files {
            scan_messages_only_file(
                &file,
                pattern,
                matcher.as_ref(),
                &mut matches,
                &mut thread_summaries,
            )?;
        }

        apply_session_index_names_to_summaries(&mut thread_summaries);

        Ok(GrepReport {
            matches,
            thread_summaries: thread_summaries.into_iter().collect(),
        })
    }
}

struct LocalScan {
    threads: Vec<ThreadDetail>,
    malformed_files: usize,
    malformed_lines: usize,
    warnings: Vec<String>,
}

#[derive(serde::Deserialize)]
struct SessionIndexEntry {
    id: String,
    #[serde(default)]
    thread_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct FastRawEnvelope<T> {
    #[serde(default)]
    timestamp: Option<DateTime<Utc>>,
    payload: T,
}

#[derive(Debug, Deserialize)]
struct FastRawSessionMeta {
    id: String,
    timestamp: DateTime<Utc>,
    #[serde(default, alias = "title")]
    name: Option<String>,
    #[serde(default)]
    cwd: Option<PathBuf>,
    #[serde(default)]
    source: Option<Value>,
    #[serde(default)]
    originator: Option<String>,
    #[serde(default)]
    model_provider: Option<String>,
}

pub(crate) fn load_session_index_names() -> Result<HashMap<String, String>, String> {
    let Some(home) = env::var_os("HOME").map(PathBuf::from) else {
        return Ok(HashMap::new());
    };
    let path = home.join(".codex/session_index.jsonl");
    if !path.exists() {
        return Ok(HashMap::new());
    }

    let content = fs::read_to_string(&path)
        .map_err(|error| format!("failed to read session index {}: {error}", path.display()))?;
    let mut names = HashMap::new();

    for line in content.lines().filter(|line| !line.trim().is_empty()) {
        let Ok(entry) = serde_json::from_str::<SessionIndexEntry>(line) else {
            continue;
        };
        let Some(name) = entry.thread_name.filter(|value| !value.trim().is_empty()) else {
            continue;
        };
        names.insert(entry.id, name);
    }

    Ok(names)
}

fn apply_session_index_names(threads: &mut BTreeMap<String, ThreadDetail>) {
    let Ok(names) = load_session_index_names() else {
        return;
    };

    for (thread_id, name) in names {
        let Some(thread) = threads.get_mut(&thread_id) else {
            continue;
        };
        if thread.summary.name.is_none() {
            thread.summary.name = Some(name);
        }
    }
}

fn apply_session_index_names_to_summaries(threads: &mut BTreeMap<String, ThreadSummary>) {
    let Ok(names) = load_session_index_names() else {
        return;
    };

    for (thread_id, name) in names {
        let Some(thread) = threads.get_mut(&thread_id) else {
            continue;
        };
        if thread.name.is_none() {
            thread.name = Some(name);
        }
    }
}

fn merge_thread_detail(existing: &mut ThreadDetail, incoming: ThreadDetail) {
    merge_thread_summary(&mut existing.summary, incoming.summary);

    for turn in incoming.turns {
        merge_turn(&mut existing.turns, turn);
    }
    existing
        .turns
        .sort_by(|left, right| turn_sort_key(left).cmp(&turn_sort_key(right)));
    recompute_thread_counts(existing);
}

fn merge_thread_summary(existing: &mut ThreadSummary, incoming: ThreadSummary) {
    let existing_freshness = existing.updated_at.unwrap_or(existing.created_at);
    let incoming_freshness = incoming.updated_at.unwrap_or(incoming.created_at);
    let prefer_incoming =
        (incoming_freshness, incoming.created_at) >= (existing_freshness, existing.created_at);

    existing.created_at = existing.created_at.min(incoming.created_at);
    existing.updated_at = match (existing.updated_at, incoming.updated_at) {
        (Some(left), Some(right)) => Some(left.max(right)),
        (None, Some(right)) => Some(right),
        (left, None) => left,
    };

    merge_optional_field(&mut existing.name, incoming.name, prefer_incoming);
    merge_preview_field(&mut existing.preview, incoming.preview);
    merge_optional_field(&mut existing.cwd, incoming.cwd, prefer_incoming);
    merge_optional_field(
        &mut existing.source_kind,
        incoming.source_kind,
        prefer_incoming,
    );
    merge_optional_field(
        &mut existing.model_provider,
        incoming.model_provider,
        prefer_incoming,
    );
    merge_optional_field(&mut existing.ephemeral, incoming.ephemeral, prefer_incoming);
    merge_optional_field(&mut existing.status, incoming.status, prefer_incoming);
}

fn merge_optional_field<T>(existing: &mut Option<T>, incoming: Option<T>, prefer_incoming: bool) {
    if prefer_incoming {
        if incoming.is_some() {
            *existing = incoming;
        }
    } else if existing.is_none() {
        *existing = incoming;
    }
}

fn merge_preview_field(existing: &mut Option<String>, incoming: Option<String>) {
    if existing.is_none() && incoming.is_some() {
        *existing = incoming;
    }
}

fn scan_messages_only_file(
    path: &Path,
    pattern: &str,
    regex: Option<&Regex>,
    matches: &mut Vec<GrepMatch>,
    thread_summaries: &mut BTreeMap<String, ThreadSummary>,
) -> Result<(), String> {
    let file = fs::File::open(path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    let reader = BufReader::new(file);
    let mut state = FastGrepFileState::new(path);

    for line in reader.lines() {
        let line = match line {
            Ok(line) => line,
            Err(_) => continue,
        };
        if line.trim().is_empty() {
            continue;
        }
        let value: Value = match serde_json::from_str(&line) {
            Ok(value) => value,
            Err(_) => continue,
        };
        scan_message_only_value(value, &mut state, pattern, regex, matches, thread_summaries);
    }

    Ok(())
}

fn scan_message_only_value(
    value: Value,
    state: &mut FastGrepFileState,
    pattern: &str,
    regex: Option<&Regex>,
    matches: &mut Vec<GrepMatch>,
    thread_summaries: &mut BTreeMap<String, ThreadSummary>,
) {
    let line_timestamp = fast_top_level_timestamp(&value);
    let Some(kind) = value.get("type").and_then(Value::as_str) else {
        return;
    };

    match kind {
        "session_meta" => {
            let Ok(raw) = serde_json::from_value::<FastRawEnvelope<FastRawSessionMeta>>(value)
            else {
                return;
            };
            state.thread_id = Some(raw.payload.id.clone());
            let incoming = ThreadSummary {
                thread_id: raw.payload.id,
                name: raw.payload.name,
                preview: None,
                created_at: raw.payload.timestamp,
                updated_at: raw.timestamp.or(Some(raw.payload.timestamp)),
                cwd: raw.payload.cwd,
                source_kind: raw
                    .payload
                    .source
                    .as_ref()
                    .and_then(fast_source_kind_from_value)
                    .or(raw.payload.originator),
                model_provider: raw.payload.model_provider,
                ephemeral: None,
                status: Some("running".into()),
            };
            merge_fast_thread_summary(thread_summaries, incoming);
        }
        "turn_context" => {
            let Some(payload) = value.get("payload").and_then(Value::as_object) else {
                return;
            };
            state.current_turn_id = Some(match payload.get("turn_id").and_then(Value::as_str) {
                Some(turn_id) => turn_id.to_string(),
                None => state.next_implicit_turn_id(),
            });
        }
        "event_msg" => {
            let Some(payload) = value.get("payload").and_then(Value::as_object) else {
                return;
            };
            let Some(event_type) = payload.get("type").and_then(Value::as_str) else {
                return;
            };
            match event_type {
                "task_started" => {
                    state.current_turn_id =
                        Some(match payload.get("turn_id").and_then(Value::as_str) {
                            Some(turn_id) => turn_id.to_string(),
                            None => state.next_implicit_turn_id(),
                        });
                    note_summary_timestamp(
                        thread_summaries,
                        state.thread_id.as_deref(),
                        line_timestamp,
                    );
                }
                "user_message" => {
                    let Some(text) = payload.get("message").and_then(Value::as_str) else {
                        return;
                    };
                    let Some(turn_id) =
                        state.resolve_turn_id(payload.get("turn_id").and_then(Value::as_str))
                    else {
                        return;
                    };
                    note_preview(thread_summaries, state.thread_id.as_deref(), text);
                    push_grep_match(
                        matches,
                        state.thread_id.as_deref(),
                        &turn_id,
                        "user_message",
                        text,
                        pattern,
                        regex,
                    );
                    note_summary_timestamp(
                        thread_summaries,
                        state.thread_id.as_deref(),
                        line_timestamp,
                    );
                }
                "agent_message" => {
                    let Some(text) = payload.get("message").and_then(Value::as_str) else {
                        return;
                    };
                    let Some(turn_id) =
                        state.resolve_turn_id(payload.get("turn_id").and_then(Value::as_str))
                    else {
                        return;
                    };
                    push_grep_match(
                        matches,
                        state.thread_id.as_deref(),
                        &turn_id,
                        "agent_message",
                        text,
                        pattern,
                        regex,
                    );
                    note_summary_timestamp(
                        thread_summaries,
                        state.thread_id.as_deref(),
                        line_timestamp,
                    );
                }
                "task_complete" => {
                    note_summary_timestamp(
                        thread_summaries,
                        state.thread_id.as_deref(),
                        line_timestamp,
                    );
                }
                _ => {}
            }
        }
        "response_item" => {
            let Some(payload) = value.get("payload").and_then(Value::as_object) else {
                return;
            };
            if payload.get("type").and_then(Value::as_str) != Some("message") {
                return;
            }
            let Some(turn_id) =
                state.resolve_turn_id(payload.get("turn_id").and_then(Value::as_str))
            else {
                return;
            };
            let Some(text) = fast_extract_response_text(payload.get("content")).or_else(|| {
                payload
                    .get("text")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            }) else {
                return;
            };
            let kind = match payload.get("role").and_then(Value::as_str) {
                Some("assistant") => "agent_message",
                _ => "user_message",
            };
            if kind == "user_message" {
                note_preview(thread_summaries, state.thread_id.as_deref(), &text);
            }
            push_grep_match(
                matches,
                state.thread_id.as_deref(),
                &turn_id,
                kind,
                &text,
                pattern,
                regex,
            );
            note_summary_timestamp(thread_summaries, state.thread_id.as_deref(), line_timestamp);
        }
        _ => {}
    }
}

fn merge_fast_thread_summary(
    threads: &mut BTreeMap<String, ThreadSummary>,
    incoming: ThreadSummary,
) {
    match threads.get_mut(&incoming.thread_id) {
        Some(existing) => merge_thread_summary(existing, incoming),
        None => {
            threads.insert(incoming.thread_id.clone(), incoming);
        }
    }
}

fn note_preview(
    thread_summaries: &mut BTreeMap<String, ThreadSummary>,
    thread_id: Option<&str>,
    text: &str,
) {
    let Some(thread_id) = thread_id else {
        return;
    };
    let Some(summary) = thread_summaries.get_mut(thread_id) else {
        return;
    };
    if summary.preview.is_none() && !text.is_empty() {
        summary.preview = Some(text.to_string());
    }
}

fn note_summary_timestamp(
    thread_summaries: &mut BTreeMap<String, ThreadSummary>,
    thread_id: Option<&str>,
    timestamp: Option<DateTime<Utc>>,
) {
    let (Some(thread_id), Some(timestamp)) = (thread_id, timestamp) else {
        return;
    };
    let Some(summary) = thread_summaries.get_mut(thread_id) else {
        return;
    };
    summary.updated_at = Some(
        summary
            .updated_at
            .unwrap_or(summary.created_at)
            .max(timestamp),
    );
}

fn push_grep_match(
    matches: &mut Vec<GrepMatch>,
    thread_id: Option<&str>,
    turn_id: &str,
    kind: &str,
    text: &str,
    pattern: &str,
    regex: Option<&Regex>,
) {
    let Some(thread_id) = thread_id else {
        return;
    };
    let matched = match regex {
        Some(regex) => regex.is_match(text),
        None => text.contains(pattern),
    };
    if matched {
        matches.push(GrepMatch {
            thread_id: thread_id.to_string(),
            turn_id: turn_id.to_string(),
            kind: kind.to_string(),
            text: text.to_string(),
        });
    }
}

struct FastGrepFileState {
    thread_id: Option<String>,
    current_turn_id: Option<String>,
    implicit_turn_prefix: String,
    implicit_turn_counter: usize,
}

impl FastGrepFileState {
    fn new(path: &Path) -> Self {
        Self {
            thread_id: None,
            current_turn_id: None,
            implicit_turn_prefix: fast_implicit_turn_prefix(path),
            implicit_turn_counter: 0,
        }
    }

    fn next_implicit_turn_id(&mut self) -> String {
        self.implicit_turn_counter += 1;
        format!(
            "{}{}",
            self.implicit_turn_prefix, self.implicit_turn_counter
        )
    }

    fn resolve_turn_id(&self, explicit: Option<&str>) -> Option<String> {
        explicit
            .map(str::to_string)
            .or_else(|| self.current_turn_id.clone())
    }
}

fn fast_top_level_timestamp(value: &Value) -> Option<DateTime<Utc>> {
    value
        .get("timestamp")
        .and_then(Value::as_str)
        .and_then(|timestamp| DateTime::parse_from_rfc3339(timestamp).ok())
        .map(|timestamp| timestamp.with_timezone(&Utc))
}

fn fast_source_kind_from_value(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(value.clone()),
        Value::Object(object) => object.keys().next().cloned(),
        _ => None,
    }
}

fn fast_implicit_turn_prefix(path: &Path) -> String {
    use std::hash::{DefaultHasher, Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    path.hash(&mut hasher);
    format!("implicit:{}:", hasher.finish())
}

fn fast_extract_response_text(value: Option<&Value>) -> Option<String> {
    let value = value?;
    match value {
        Value::String(text) if !text.is_empty() => Some(text.clone()),
        Value::Array(parts) => {
            let texts: Vec<_> = parts
                .iter()
                .filter_map(fast_extract_response_part_text)
                .collect();
            if texts.is_empty() {
                None
            } else {
                Some(texts.join("\n\n"))
            }
        }
        Value::Object(_) => fast_extract_response_part_text(value),
        _ => None,
    }
}

fn fast_extract_response_part_text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) if !text.is_empty() => Some(text.clone()),
        Value::Object(object) => object
            .get("text")
            .and_then(Value::as_str)
            .filter(|text| !text.is_empty())
            .map(str::to_string)
            .or_else(|| fast_extract_response_text(object.get("content")))
            .or_else(|| fast_extract_response_text(object.get("summary"))),
        _ => None,
    }
}

fn merge_turn(turns: &mut Vec<Turn>, incoming: Turn) {
    if let Some(existing) = turns
        .iter_mut()
        .find(|turn| turn.turn_id == incoming.turn_id)
    {
        let existing_freshness = existing.completed_at.or(existing.started_at);
        let incoming_freshness = incoming.completed_at.or(incoming.started_at);
        let prefer_incoming = incoming_freshness >= existing_freshness;

        existing.started_at = match (existing.started_at, incoming.started_at) {
            (Some(left), Some(right)) => Some(left.min(right)),
            (None, Some(right)) => Some(right),
            (left, None) => left,
        };
        existing.completed_at = match (existing.completed_at, incoming.completed_at) {
            (Some(left), Some(right)) => Some(left.max(right)),
            (None, Some(right)) => Some(right),
            (left, None) => left,
        };
        if prefer_incoming {
            existing.status = incoming.status;
        }
        merge_turn_items(&mut existing.items, incoming.items);
        return;
    }

    turns.push(incoming);
}

fn merge_turn_items(existing: &mut Vec<Item>, incoming: Vec<Item>) {
    for item in incoming {
        if !merge_command_execution_item(existing, &item) {
            existing.push(item);
        }
    }
}

fn merge_command_execution_item(existing: &mut [Item], incoming: &Item) -> bool {
    let Item::CommandExecution(incoming_command) = incoming else {
        return false;
    };
    let Some(call_id) = command_call_id(incoming_command) else {
        return false;
    };

    let Some(Item::CommandExecution(existing_command)) = existing.iter_mut().find(|item| {
        matches!(
            item,
            Item::CommandExecution(command)
                if command_call_id(command) == Some(call_id)
        )
    }) else {
        return false;
    };

    merge_command_execution(existing_command, incoming_command);
    true
}

fn merge_command_execution(existing: &mut CommandExecutionItem, incoming: &CommandExecutionItem) {
    if existing.command.is_none() {
        existing.command = incoming.command.clone();
    }
    if existing.exit_code.is_none() {
        existing.exit_code = incoming.exit_code;
    }
    if existing.cwd.is_none() {
        existing.cwd = incoming.cwd.clone();
    }
    if existing.output.is_none() {
        existing.output = incoming.output.clone();
    }
    for (key, value) in &incoming.attributes {
        existing
            .attributes
            .entry(key.clone())
            .or_insert_with(|| value.clone());
    }
}

fn command_call_id(command: &CommandExecutionItem) -> Option<&str> {
    command
        .attributes
        .get("call_id")
        .and_then(serde_json::Value::as_str)
}

fn attach_pending_command(
    threads_by_id: &mut BTreeMap<String, ThreadDetail>,
    pending: PendingCommandCall,
    warnings: &mut Vec<String>,
) {
    let Some(thread) = threads_by_id.get_mut(&pending.thread_id) else {
        warnings.push(format!(
            "dropped pending command for missing thread {}",
            pending.thread_id
        ));
        return;
    };

    merge_turn(
        &mut thread.turns,
        Turn {
            turn_id: pending.turn_id,
            status: "in_progress".into(),
            started_at: None,
            completed_at: None,
            items: vec![Item::CommandExecution(pending.item)],
        },
    );
}

fn recompute_thread_counts(thread: &mut ThreadDetail) {
    thread.items_count = thread.turns.iter().map(|turn| turn.items.len()).sum();
    thread.commands_count = thread
        .turns
        .iter()
        .flat_map(|turn| turn.items.iter())
        .filter(|item| matches!(item, Item::CommandExecution(_)))
        .count();
    thread.files_changed_count = thread
        .turns
        .iter()
        .flat_map(|turn| turn.items.iter())
        .filter(|item| matches!(item, Item::FileChange(_)))
        .count();
}

fn thread_sort_key(summary: &ThreadSummary) -> (Option<DateTime<Utc>>, DateTime<Utc>, &str) {
    (
        summary.updated_at,
        summary.created_at,
        summary.thread_id.as_str(),
    )
}

fn turn_sort_key(turn: &Turn) -> (Option<DateTime<Utc>>, Option<DateTime<Utc>>, &str) {
    (turn.started_at, turn.completed_at, turn.turn_id.as_str())
}

fn item_texts(item: &Item, scope: SearchScope) -> Vec<String> {
    if !scope.includes_item(item) {
        return Vec::new();
    }

    match item {
        Item::UserMessage(message) | Item::AgentMessage(message) => {
            message.text.clone().into_iter().collect()
        }
        Item::CommandExecution(command) => command_texts(command),
        Item::FileChange(change) => file_change_texts(change),
        Item::ReasoningSummary(summary) => summary.text.clone().into_iter().collect(),
        Item::WebSearch(search) => [
            search.query.clone(),
            search.title.clone(),
            search.url.clone(),
        ]
        .into_iter()
        .flatten()
        .collect(),
        Item::McpToolCall(call) => {
            let mut texts = Vec::new();
            if let Some(server) = &call.server {
                texts.push(server.clone());
            }
            if let Some(tool) = &call.tool {
                texts.push(tool.clone());
            }
            if let Some(arguments) = &call.arguments {
                texts.push(arguments.to_string());
            }
            texts
        }
        Item::Other(other) => other.data.values().filter_map(value_as_text).collect(),
    }
}

fn command_texts(command: &CommandExecutionItem) -> Vec<String> {
    let mut texts = Vec::new();
    if let Some(value) = &command.command {
        texts.push(value.clone());
    }
    if let Some(value) = &command.output {
        texts.push(value.clone());
    }
    if let Some(value) = &command.cwd {
        texts.push(value.display().to_string());
    }
    texts
}

fn file_change_texts(change: &FileChangeItem) -> Vec<String> {
    let mut texts = Vec::new();
    if let Some(value) = &change.path {
        texts.push(value.display().to_string());
    }
    if let Some(value) = &change.summary {
        texts.push(value.clone());
    }
    if let Some(value) = &change.change_type {
        texts.push(value.clone());
    }
    texts
}

fn value_as_text(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(value) => Some(value.clone()),
        serde_json::Value::Number(value) => Some(value.to_string()),
        serde_json::Value::Bool(value) => Some(value.to_string()),
        _ => None,
    }
}

pub fn fixture_root(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures/local_history")
        .join(name)
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::sync::{Mutex, OnceLock};

    use super::*;
    use crate::search_scope::SearchScope;

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn lists_threads_from_fixture_root() {
        let _guard = env_lock().lock().expect("lock");
        env::set_var("CODEX_HISTORY_HOME", fixture_root("sample_root"));
        let backend = LocalBackend::discover();

        let threads = backend.list_threads().expect("list threads");
        assert_eq!(threads.len(), 3);
        assert_eq!(
            threads
                .iter()
                .filter(|thread| thread.thread_id == "thr_simple")
                .count(),
            1
        );
        assert!(threads
            .iter()
            .any(|thread| thread.thread_id == "thr_simple"));
        assert!(threads
            .iter()
            .any(|thread| thread.thread_id == "thr_command"));
        assert!(threads
            .iter()
            .any(|thread| thread.thread_id == "thr_malformed"));
        env::remove_var("CODEX_HISTORY_HOME");
    }

    #[test]
    fn show_merges_multiple_shards_for_a_single_thread() {
        let _guard = env_lock().lock().expect("lock");
        env::set_var("CODEX_HISTORY_HOME", fixture_root("sample_root"));
        let backend = LocalBackend::discover();

        let detail = backend
            .show_thread("thr_simple", true)
            .expect("show thread")
            .expect("thread detail");

        assert_eq!(detail.turns.len(), 3);
        assert_eq!(detail.commands_count, 2);
        assert!(detail
            .turns
            .iter()
            .flat_map(|turn| turn.items.iter())
            .any(|item| matches!(item, Item::CommandExecution(command) if command.command.as_deref() == Some("cargo test -- --help") && command.output.as_deref() == Some("help ok"))));
        env::remove_var("CODEX_HISTORY_HOME");
    }

    #[test]
    fn grep_finds_matches_without_index() {
        let _guard = env_lock().lock().expect("lock");
        env::set_var("CODEX_HISTORY_HOME", fixture_root("sample_root"));
        let backend = LocalBackend::discover();

        let matches = backend
            .grep("leftover argv", false, SearchScope::default())
            .expect("grep");
        assert!(!matches.is_empty());
        assert!(matches.iter().any(|entry| entry.kind == "agent_message"));
        let shell_matches = backend
            .grep(
                "help ok",
                false,
                SearchScope {
                    include_thinking: false,
                    include_tools: true,
                },
            )
            .expect("grep shell output");
        assert!(shell_matches
            .iter()
            .any(|entry| entry.thread_id == "thr_simple" && entry.kind == "command_execution"));
        env::remove_var("CODEX_HISTORY_HOME");
    }

    #[test]
    fn doctor_reports_discovery_and_malformed_lines() {
        let _guard = env_lock().lock().expect("lock");
        env::set_var("CODEX_HISTORY_HOME", fixture_root("sample_root"));
        let backend = LocalBackend::discover();

        let report = backend.doctor().expect("doctor");
        assert_eq!(report.roots.len(), 1);
        assert_eq!(report.parsed_threads, 3);
        assert!(report.malformed_files >= 2);
        assert!(report.malformed_lines >= 2);
        assert!(report.warnings.iter().any(
            |warning| warning.contains("thr_missing_meta.jsonl: missing valid thread metadata")
        ));
        env::remove_var("CODEX_HISTORY_HOME");
    }

    #[test]
    fn show_merges_command_output_across_shards() {
        let _guard = env_lock().lock().expect("lock");
        env::set_var("CODEX_HISTORY_HOME", fixture_root("cross_shard_root"));
        let backend = LocalBackend::discover();

        let detail = backend
            .show_thread("thr_cross_shard", true)
            .expect("show thread")
            .expect("thread detail");

        assert_eq!(detail.turns.len(), 1);
        assert_eq!(detail.commands_count, 1);
        assert!(detail.turns[0].items.iter().any(|item| matches!(
            item,
            Item::CommandExecution(command)
                if command.command.as_deref() == Some("cargo test parser")
                    && command.output.as_deref() == Some("tests ok")
        )));

        let grep_matches = backend
            .grep(
                "tests ok",
                false,
                SearchScope {
                    include_thinking: false,
                    include_tools: true,
                },
            )
            .expect("grep output");
        assert!(grep_matches.iter().any(|entry| {
            entry.thread_id == "thr_cross_shard"
                && entry.turn_id == "turn_cross_shard_1"
                && entry.kind == "command_execution"
        }));
        env::remove_var("CODEX_HISTORY_HOME");
    }
}
