use std::collections::{BTreeMap, HashMap};
use std::fs::File;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::{self, BufRead, BufReader};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::Value;

use crate::model::{
    CommandExecutionItem, Item, McpToolCallItem, ThreadDetail, ThreadSummary, Turn, UnknownItem,
};

#[derive(Debug, Clone, PartialEq)]
pub struct ParseReport {
    pub detail: Option<ThreadDetail>,
    pub malformed_lines: usize,
    pub unknown_event_lines: usize,
    pub warnings: Vec<String>,
    pending_commands: HashMap<String, PendingCommandCall>,
}

pub fn parse_session_log(path: &Path) -> io::Result<ParseReport> {
    parse_session_log_with_pending(path, HashMap::new(), true)
}

pub(crate) fn parse_session_log_incremental(
    path: &Path,
    pending_commands: HashMap<String, PendingCommandCall>,
) -> io::Result<ParseReport> {
    parse_session_log_with_pending(path, pending_commands, false)
}

fn parse_session_log_with_pending(
    path: &Path,
    pending_commands: HashMap<String, PendingCommandCall>,
    materialize_pending_commands: bool,
) -> io::Result<ParseReport> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut state = ParseState::new(path, pending_commands);

    for line in reader.lines() {
        let line = match line {
            Ok(line) => line,
            Err(_) => {
                state.malformed_lines += 1;
                continue;
            }
        };

        if line.trim().is_empty() {
            continue;
        }

        let value: Value = match serde_json::from_str(&line) {
            Ok(value) => value,
            Err(_) => {
                state.malformed_lines += 1;
                continue;
            }
        };

        let line_timestamp = top_level_timestamp(&value);
        let Some(event_type) = value
            .get("type")
            .and_then(Value::as_str)
            .map(str::to_string)
        else {
            state.malformed_lines += 1;
            continue;
        };

        let parsed = match event_type.as_str() {
            "session_meta" => attach_session_meta_event(value, &mut state),
            "turn_context" => attach_turn_context_event(value, &mut state, line_timestamp),
            "response_item" => attach_response_item_event(value, &mut state),
            "event_msg" => attach_runtime_event(value, &mut state, line_timestamp),
            "thread" => attach_legacy_thread_event(value, &mut state),
            "turn" => attach_legacy_turn_event(value, &mut state),
            "item" => attach_legacy_item_event(value, &mut state),
            other => attach_legacy_unknown_event(other, value, &mut state),
        };

        if parsed {
            if let Some(timestamp) = line_timestamp {
                state.note_timestamp(timestamp);
            }
        } else {
            state.malformed_lines += 1;
        }
    }

    let malformed_lines = state.malformed_lines;
    let unknown_event_lines = state.unknown_event_lines;
    let (detail, warnings, pending_commands) = state.finish(materialize_pending_commands);

    Ok(ParseReport {
        detail,
        malformed_lines,
        unknown_event_lines,
        warnings,
        pending_commands,
    })
}

#[derive(Debug)]
struct ParseState {
    thread: Option<ThreadSummary>,
    turns: Vec<Turn>,
    turn_index: HashMap<String, usize>,
    current_turn_id: Option<String>,
    pending_commands: HashMap<String, PendingCommandCall>,
    malformed_lines: usize,
    unknown_event_lines: usize,
    implicit_turn_prefix: String,
    implicit_turn_counter: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PendingCommandCall {
    pub(crate) thread_id: String,
    pub(crate) turn_id: String,
    pub(crate) item: CommandExecutionItem,
}

impl ParseState {
    fn new(path: &Path, pending_commands: HashMap<String, PendingCommandCall>) -> Self {
        Self {
            thread: None,
            turns: Vec::new(),
            turn_index: HashMap::new(),
            current_turn_id: None,
            pending_commands,
            malformed_lines: 0,
            unknown_event_lines: 0,
            implicit_turn_prefix: implicit_turn_prefix(path),
            implicit_turn_counter: 0,
        }
    }

    fn note_timestamp(&mut self, timestamp: DateTime<Utc>) {
        if let Some(thread) = self.thread.as_mut() {
            thread.updated_at = Some(timestamp);
        }
    }

    fn ensure_turn(&mut self, turn_id: &str, started_at: Option<DateTime<Utc>>) -> usize {
        if let Some(index) = self.turn_index.get(turn_id).copied() {
            if self.turns[index].started_at.is_none() {
                self.turns[index].started_at = started_at;
            }
            self.current_turn_id = Some(turn_id.to_string());
            return index;
        }

        let next_index = self.turns.len();
        self.turn_index.insert(turn_id.to_string(), next_index);
        self.turns.push(Turn {
            turn_id: turn_id.to_string(),
            status: "in_progress".into(),
            started_at,
            completed_at: None,
            items: Vec::new(),
        });
        self.current_turn_id = Some(turn_id.to_string());
        next_index
    }

    fn begin_turn(&mut self, explicit: Option<&str>, started_at: Option<DateTime<Utc>>) -> String {
        match explicit {
            Some(turn_id) => self.claim_explicit_turn_id(turn_id, started_at),
            None => {
                let turn_id = self.next_implicit_turn_id();
                self.ensure_turn(&turn_id, started_at);
                turn_id
            }
        }
    }

    fn resolve_turn_id(
        &mut self,
        explicit: Option<&str>,
        started_at: Option<DateTime<Utc>>,
    ) -> Option<String> {
        explicit
            .map(|turn_id| self.claim_explicit_turn_id(turn_id, started_at))
            .or_else(|| self.current_turn_id.clone())
    }

    fn claim_explicit_turn_id(
        &mut self,
        turn_id: &str,
        started_at: Option<DateTime<Utc>>,
    ) -> String {
        if !self.turn_index.contains_key(turn_id) {
            if self.promote_current_implicit_turn(turn_id, started_at) {
                return turn_id.to_string();
            }
            self.ensure_turn(turn_id, started_at);
            return turn_id.to_string();
        }

        let index = self.ensure_turn(turn_id, started_at);
        self.current_turn_id = Some(self.turns[index].turn_id.clone());
        turn_id.to_string()
    }

    fn promote_current_implicit_turn(
        &mut self,
        explicit_turn_id: &str,
        started_at: Option<DateTime<Utc>>,
    ) -> bool {
        let Some(current_turn_id) = self.current_turn_id.clone() else {
            return false;
        };
        if !self.is_implicit_turn_id(&current_turn_id) {
            return false;
        }

        let Some(index) = self.turn_index.remove(&current_turn_id) else {
            return false;
        };
        self.turns[index].turn_id = explicit_turn_id.to_string();
        if self.turns[index].started_at.is_none() {
            self.turns[index].started_at = started_at;
        }
        self.turn_index.insert(explicit_turn_id.to_string(), index);
        self.current_turn_id = Some(explicit_turn_id.to_string());
        true
    }

    fn next_implicit_turn_id(&mut self) -> String {
        self.implicit_turn_counter += 1;
        format!(
            "{}{}",
            self.implicit_turn_prefix, self.implicit_turn_counter
        )
    }

    fn is_implicit_turn_id(&self, turn_id: &str) -> bool {
        turn_id.starts_with(&self.implicit_turn_prefix)
    }

    fn add_item(&mut self, turn_id: &str, item: Item) -> bool {
        let Some(index) = self.turn_index.get(turn_id).copied() else {
            return false;
        };

        if self
            .thread
            .as_ref()
            .and_then(|thread| thread.preview.as_ref())
            .is_none()
        {
            if let Item::UserMessage(message) = &item {
                let preview_is_user_message = message
                    .attributes
                    .get("role")
                    .and_then(Value::as_str)
                    .map(|role| role == "user")
                    .unwrap_or(true);
                if preview_is_user_message {
                    if let Some(text) = message.text.as_ref().filter(|text| !text.is_empty()) {
                        if let Some(thread) = self.thread.as_mut() {
                            thread.preview = Some(text.clone());
                        }
                    }
                }
            }
        }

        self.turns[index].items.push(item);
        true
    }

    fn update_turn_status(
        &mut self,
        turn_id: &str,
        status: &str,
        completed_at: Option<DateTime<Utc>>,
    ) -> bool {
        let index = self.ensure_turn(turn_id, None);
        self.turns[index].status = status.to_string();
        if completed_at.is_some() {
            self.turns[index].completed_at = completed_at;
        }
        true
    }

    fn finish(
        mut self,
        materialize_pending_commands: bool,
    ) -> (
        Option<ThreadDetail>,
        Vec<String>,
        HashMap<String, PendingCommandCall>,
    ) {
        let mut pending_commands = std::mem::take(&mut self.pending_commands);
        if materialize_pending_commands {
            for pending in pending_commands.drain().map(|(_, pending)| pending) {
                let _ = self.add_item(&pending.turn_id, Item::CommandExecution(pending.item));
            }
        }

        let Some(mut summary) = self.thread else {
            return (
                None,
                vec!["missing valid thread metadata".into()],
                HashMap::new(),
            );
        };
        if summary.updated_at.is_none() {
            summary.updated_at = Some(summary.created_at);
        }
        let items_count = self.turns.iter().map(|turn| turn.items.len()).sum();
        let commands_count = self
            .turns
            .iter()
            .flat_map(|turn| turn.items.iter())
            .filter(|item| matches!(item, Item::CommandExecution(_)))
            .count();
        let files_changed_count = self
            .turns
            .iter()
            .flat_map(|turn| turn.items.iter())
            .filter(|item| matches!(item, Item::FileChange(_)))
            .count();

        (
            Some(ThreadDetail {
                summary,
                turns: self.turns,
                items_count,
                commands_count,
                files_changed_count,
            }),
            Vec::new(),
            if materialize_pending_commands {
                HashMap::new()
            } else {
                pending_commands
            },
        )
    }
}

impl ParseReport {
    pub(crate) fn take_pending_commands(&mut self) -> HashMap<String, PendingCommandCall> {
        std::mem::take(&mut self.pending_commands)
    }
}

fn attach_session_meta_event(value: Value, state: &mut ParseState) -> bool {
    let Ok(raw) = serde_json::from_value::<RawEnvelope<RawSessionMeta>>(value) else {
        return false;
    };

    state.thread = Some(ThreadSummary {
        thread_id: raw.payload.id,
        name: None,
        preview: None,
        created_at: raw.payload.timestamp,
        updated_at: raw.timestamp.or(Some(raw.payload.timestamp)),
        cwd: raw.payload.cwd,
        source_kind: raw
            .payload
            .source
            .as_ref()
            .and_then(source_kind_from_value)
            .or(raw.payload.originator),
        model_provider: raw.payload.model_provider,
        ephemeral: None,
        status: Some("running".into()),
    });
    true
}

fn attach_turn_context_event(
    value: Value,
    state: &mut ParseState,
    line_timestamp: Option<DateTime<Utc>>,
) -> bool {
    let Ok(raw) = serde_json::from_value::<RawEnvelope<Value>>(value) else {
        return false;
    };
    let Some(payload) = raw.payload.as_object() else {
        return false;
    };

    state.begin_turn(
        payload.get("turn_id").and_then(Value::as_str),
        line_timestamp.or(raw.timestamp),
    );
    true
}

fn attach_runtime_event(
    value: Value,
    state: &mut ParseState,
    line_timestamp: Option<DateTime<Utc>>,
) -> bool {
    let Ok(raw) = serde_json::from_value::<RawEnvelope<Value>>(value) else {
        return false;
    };
    let Some(payload) = raw.payload.as_object() else {
        return false;
    };
    let Some(kind) = payload.get("type").and_then(Value::as_str) else {
        return false;
    };

    match kind {
        "task_started" => {
            state.begin_turn(
                payload.get("turn_id").and_then(Value::as_str),
                line_timestamp.or(raw.timestamp),
            );
            if let Some(thread) = state.thread.as_mut() {
                thread.status = Some("running".into());
            }
            true
        }
        "task_complete" => {
            let Some(turn_id) = state.resolve_turn_id(
                payload.get("turn_id").and_then(Value::as_str),
                line_timestamp.or(raw.timestamp),
            ) else {
                return false;
            };
            let completed =
                state.update_turn_status(&turn_id, "completed", line_timestamp.or(raw.timestamp));
            if completed {
                if let Some(thread) = state.thread.as_mut() {
                    thread.status = Some("completed".into());
                }
            }
            completed
        }
        "user_message" => {
            let Some(turn_id) = state.resolve_turn_id(
                payload.get("turn_id").and_then(Value::as_str),
                line_timestamp.or(raw.timestamp),
            ) else {
                return false;
            };
            let Some(text) = payload.get("message").and_then(Value::as_str) else {
                return false;
            };
            state.add_item(&turn_id, Item::UserMessage(message_item(text)))
        }
        "agent_message" => {
            let Some(turn_id) = state.resolve_turn_id(
                payload.get("turn_id").and_then(Value::as_str),
                line_timestamp.or(raw.timestamp),
            ) else {
                return false;
            };
            let Some(text) = payload.get("message").and_then(Value::as_str) else {
                return false;
            };
            state.add_item(&turn_id, Item::AgentMessage(message_item(text)))
        }
        "agent_reasoning" => {
            let Some(turn_id) = state.resolve_turn_id(
                payload.get("turn_id").and_then(Value::as_str),
                line_timestamp.or(raw.timestamp),
            ) else {
                return false;
            };
            let Some(text) = payload.get("text").and_then(Value::as_str) else {
                return false;
            };
            let item = serde_json::from_value(serde_json::json!({
                "kind": "reasoning_summary",
                "text": text,
            }))
            .expect("reasoning summary should deserialize");
            state.add_item(&turn_id, item)
        }
        other => {
            let Some(turn_id) = state.resolve_turn_id(
                payload.get("turn_id").and_then(Value::as_str),
                line_timestamp.or(raw.timestamp),
            ) else {
                return false;
            };
            state.unknown_event_lines += 1;
            state.add_item(
                &turn_id,
                Item::Other(UnknownItem {
                    kind: format!("event:{other}"),
                    data: payload_without_keys(payload, &["type", "turn_id"]),
                }),
            )
        }
    }
}

fn attach_response_item_event(value: Value, state: &mut ParseState) -> bool {
    let Ok(raw) = serde_json::from_value::<RawEnvelope<Value>>(value) else {
        return false;
    };
    let Some(payload) = raw.payload.as_object() else {
        return false;
    };
    let Some(kind) = payload.get("type").and_then(Value::as_str) else {
        return false;
    };

    match kind {
        "function_call" => attach_function_call(payload, state),
        "function_call_output" => attach_function_call_output(payload, state),
        "message" => attach_response_message(payload, state),
        "reasoning" => attach_response_reasoning(payload, state),
        other => {
            let Some(turn_id) =
                state.resolve_turn_id(payload.get("turn_id").and_then(Value::as_str), None)
            else {
                return false;
            };
            state.unknown_event_lines += 1;
            state.add_item(
                &turn_id,
                Item::Other(UnknownItem {
                    kind: other.to_string(),
                    data: payload_without_keys(payload, &["type", "turn_id"]),
                }),
            )
        }
    }
}

fn attach_function_call(payload: &serde_json::Map<String, Value>, state: &mut ParseState) -> bool {
    let Some(turn_id) = state.resolve_turn_id(payload.get("turn_id").and_then(Value::as_str), None)
    else {
        return false;
    };
    let Some(thread_id) = state.thread.as_ref().map(|thread| thread.thread_id.clone()) else {
        return false;
    };
    let Some(name) = payload.get("name").and_then(Value::as_str) else {
        return false;
    };

    if matches!(name, "exec_command" | "shell") {
        let Some(call_id) = payload.get("call_id").and_then(Value::as_str) else {
            return false;
        };
        let arguments = payload
            .get("arguments")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let mut item = parse_exec_command_arguments(arguments);
        item.attributes
            .insert("call_id".into(), Value::String(call_id.to_string()));
        state.pending_commands.insert(
            pending_command_key(&thread_id, call_id),
            PendingCommandCall {
                thread_id,
                turn_id,
                item,
            },
        );
        return true;
    }

    let arguments = payload.get("arguments").and_then(Value::as_str);
    let item = Item::McpToolCall(McpToolCallItem {
        server: None,
        tool: Some(name.to_string()),
        arguments: arguments.map(parse_json_argument_string),
        attributes: payload_without_keys(payload, &["type", "name", "arguments"])
            .into_iter()
            .collect(),
    });
    state.add_item(&turn_id, item)
}

fn attach_function_call_output(
    payload: &serde_json::Map<String, Value>,
    state: &mut ParseState,
) -> bool {
    let Some(call_id) = payload.get("call_id").and_then(Value::as_str) else {
        return false;
    };
    let pending_key = state
        .thread
        .as_ref()
        .map(|thread| pending_command_key(&thread.thread_id, call_id))
        .unwrap_or_else(|| call_id.to_string());

    let Some(mut pending) = state
        .pending_commands
        .remove(&pending_key)
        .or_else(|| state.pending_commands.remove(call_id))
    else {
        let Some(turn_id) =
            state.resolve_turn_id(payload.get("turn_id").and_then(Value::as_str), None)
        else {
            return false;
        };
        state.unknown_event_lines += 1;
        return state.add_item(
            &turn_id,
            Item::Other(UnknownItem {
                kind: "function_call_output".into(),
                data: payload_without_keys(payload, &["type", "turn_id"]),
            }),
        );
    };

    pending.item.output = payload
        .get("output")
        .and_then(Value::as_str)
        .map(str::to_string);
    state.add_item(&pending.turn_id, Item::CommandExecution(pending.item))
}

fn attach_response_message(
    payload: &serde_json::Map<String, Value>,
    state: &mut ParseState,
) -> bool {
    let Some(turn_id) = state.resolve_turn_id(payload.get("turn_id").and_then(Value::as_str), None)
    else {
        return false;
    };

    let item = crate::model::MessageItem {
        text: extract_response_text(payload.get("content")).or_else(|| {
            payload
                .get("text")
                .and_then(Value::as_str)
                .map(str::to_string)
        }),
        attributes: payload_without_keys(payload, &["type", "turn_id", "content", "text"]),
    };

    match payload.get("role").and_then(Value::as_str) {
        Some("assistant") => state.add_item(&turn_id, Item::AgentMessage(item)),
        _ => state.add_item(&turn_id, Item::UserMessage(item)),
    }
}

fn attach_response_reasoning(
    payload: &serde_json::Map<String, Value>,
    state: &mut ParseState,
) -> bool {
    let Some(turn_id) = state.resolve_turn_id(payload.get("turn_id").and_then(Value::as_str), None)
    else {
        return false;
    };

    state.add_item(
        &turn_id,
        Item::ReasoningSummary(crate::model::ReasoningSummaryItem {
            text: extract_response_text(payload.get("summary"))
                .or_else(|| extract_response_text(payload.get("content")))
                .or_else(|| {
                    payload
                        .get("text")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                }),
            attributes: payload_without_keys(
                payload,
                &[
                    "type",
                    "turn_id",
                    "summary",
                    "content",
                    "text",
                    "encrypted_content",
                ],
            ),
        }),
    )
}

fn parse_exec_command_arguments(arguments: &str) -> CommandExecutionItem {
    let parsed = parse_json_argument_string(arguments);
    let Some(object) = parsed.as_object() else {
        return CommandExecutionItem {
            command: Some(arguments.to_string()),
            exit_code: None,
            cwd: None,
            output: None,
            attributes: BTreeMap::new(),
        };
    };

    CommandExecutionItem {
        command: object
            .get("cmd")
            .or_else(|| object.get("command"))
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| parse_command_array(object.get("argv")))
            .or_else(|| parse_command_array(object.get("args"))),
        exit_code: None,
        cwd: object.get("cwd").and_then(Value::as_str).map(PathBuf::from),
        output: None,
        attributes: object
            .iter()
            .filter(|(key, _)| *key != "cmd" && *key != "command" && *key != "cwd")
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect(),
    }
}

fn parse_json_argument_string(value: &str) -> Value {
    serde_json::from_str(value).unwrap_or_else(|_| Value::String(value.to_string()))
}

fn message_item(text: &str) -> crate::model::MessageItem {
    serde_json::from_value(serde_json::json!({ "text": text }))
        .expect("message item should deserialize")
}

fn attach_legacy_thread_event(value: Value, state: &mut ParseState) -> bool {
    let Ok(raw) = serde_json::from_value::<RawThreadEvent>(value) else {
        return false;
    };
    state.thread = Some(raw.into_summary());
    true
}

fn attach_legacy_turn_event(value: Value, state: &mut ParseState) -> bool {
    let Ok(raw) = serde_json::from_value::<RawTurnEvent>(value) else {
        return false;
    };
    if state.turn_index.contains_key(&raw.turn_id) {
        return false;
    }
    let turn_id = raw.turn_id.clone();
    let next_index = state.turns.len();
    state.turn_index.insert(turn_id.clone(), next_index);
    state.turns.push(raw.into_turn());
    state.current_turn_id = Some(turn_id);
    true
}

fn attach_legacy_item_event(value: Value, state: &mut ParseState) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    let Some(turn_id) = object.get("turn_id").and_then(Value::as_str) else {
        return false;
    };
    let Some(kind) = object.get("kind").and_then(Value::as_str) else {
        return false;
    };

    let mut payload = serde_json::Map::new();
    payload.insert("kind".into(), Value::String(kind.to_string()));
    for (key, value) in object {
        if key == "type" || key == "turn_id" {
            continue;
        }
        payload.insert(key.clone(), value.clone());
    }

    match serde_json::from_value::<Item>(Value::Object(payload)) {
        Ok(item) => state.add_item(turn_id, item),
        Err(_) => false,
    }
}

fn attach_legacy_unknown_event(event_type: &str, value: Value, state: &mut ParseState) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    let Some(turn_id) = object
        .get("turn_id")
        .and_then(Value::as_str)
        .or(state.current_turn_id.as_deref())
        .map(str::to_string)
    else {
        return false;
    };

    state.unknown_event_lines += 1;
    state.add_item(
        &turn_id,
        Item::Other(UnknownItem {
            kind: format!("event:{event_type}"),
            data: object
                .iter()
                .filter(|(key, _)| *key != "type" && *key != "turn_id")
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
        }),
    )
}

fn payload_without_keys(
    payload: &serde_json::Map<String, Value>,
    skipped: &[&str],
) -> BTreeMap<String, Value> {
    payload
        .iter()
        .filter(|(key, _)| !skipped.contains(&key.as_str()))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

fn top_level_timestamp(value: &Value) -> Option<DateTime<Utc>> {
    value
        .get("timestamp")
        .and_then(Value::as_str)
        .and_then(|timestamp| DateTime::parse_from_rfc3339(timestamp).ok())
        .map(|timestamp| timestamp.with_timezone(&Utc))
}

fn source_kind_from_value(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(value.clone()),
        Value::Object(object) => object.keys().next().cloned(),
        _ => None,
    }
}

fn implicit_turn_prefix(path: &Path) -> String {
    let mut hasher = DefaultHasher::new();
    path.hash(&mut hasher);
    format!("implicit:{}:", hasher.finish())
}

fn pending_command_key(thread_id: &str, call_id: &str) -> String {
    format!("{thread_id}\u{0}{call_id}")
}

fn extract_response_text(value: Option<&Value>) -> Option<String> {
    let value = value?;
    match value {
        Value::String(text) if !text.is_empty() => Some(text.clone()),
        Value::Array(parts) => {
            let texts: Vec<_> = parts
                .iter()
                .filter_map(extract_response_part_text)
                .collect();
            if texts.is_empty() {
                None
            } else {
                Some(texts.join("\n\n"))
            }
        }
        Value::Object(_) => extract_response_part_text(value),
        _ => None,
    }
}

fn extract_response_part_text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) if !text.is_empty() => Some(text.clone()),
        Value::Object(object) => object
            .get("text")
            .and_then(Value::as_str)
            .filter(|text| !text.is_empty())
            .map(str::to_string)
            .or_else(|| extract_response_text(object.get("content")))
            .or_else(|| extract_response_text(object.get("summary"))),
        _ => None,
    }
}

fn parse_command_array(value: Option<&Value>) -> Option<String> {
    let values = value?.as_array()?;
    let parts: Vec<_> = values
        .iter()
        .map(Value::as_str)
        .collect::<Option<Vec<_>>>()?
        .into_iter()
        .map(str::to_string)
        .collect();
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" "))
    }
}

#[derive(Debug, Deserialize)]
struct RawEnvelope<T> {
    #[serde(default)]
    timestamp: Option<DateTime<Utc>>,
    payload: T,
}

#[derive(Debug, Deserialize)]
struct RawSessionMeta {
    id: String,
    timestamp: DateTime<Utc>,
    #[serde(default)]
    cwd: Option<PathBuf>,
    #[serde(default)]
    source: Option<Value>,
    #[serde(default)]
    originator: Option<String>,
    #[serde(default)]
    model_provider: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawThreadEvent {
    thread_id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    preview: Option<String>,
    created_at: DateTime<Utc>,
    #[serde(default)]
    updated_at: Option<DateTime<Utc>>,
    #[serde(default)]
    cwd: Option<PathBuf>,
    #[serde(default)]
    source_kind: Option<String>,
    #[serde(default)]
    model_provider: Option<String>,
    #[serde(default)]
    ephemeral: Option<bool>,
    #[serde(default)]
    status: Option<String>,
}

impl RawThreadEvent {
    fn into_summary(self) -> ThreadSummary {
        ThreadSummary {
            thread_id: self.thread_id,
            name: self.name,
            preview: self.preview,
            created_at: self.created_at,
            updated_at: self.updated_at,
            cwd: self.cwd,
            source_kind: self.source_kind,
            model_provider: self.model_provider,
            ephemeral: self.ephemeral,
            status: self.status,
        }
    }
}

#[derive(Debug, Deserialize)]
struct RawTurnEvent {
    turn_id: String,
    status: String,
    #[serde(default)]
    started_at: Option<DateTime<Utc>>,
    #[serde(default)]
    completed_at: Option<DateTime<Utc>>,
}

impl RawTurnEvent {
    fn into_turn(self) -> Turn {
        Turn {
            turn_id: self.turn_id,
            status: self.status,
            started_at: self.started_at,
            completed_at: self.completed_at,
            items: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn fixture(path: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(path)
    }

    #[test]
    fn parses_simple_fixture_into_canonical_models() {
        let parsed = parse_session_log(&fixture(
            "fixtures/local_history/sample_root/sessions/thr_simple.jsonl",
        ))
        .expect("read fixture");
        let detail = parsed.detail.expect("parsed thread");

        assert_eq!(detail.summary.thread_id, "thr_simple");
        assert_eq!(detail.turns.len(), 2);
        assert_eq!(detail.items_count, 4);
        assert_eq!(detail.commands_count, 1);
        assert_eq!(detail.files_changed_count, 0);
        assert_eq!(
            detail.summary.preview.as_deref(),
            Some("Please inspect the parser regression.")
        );
    }

    #[test]
    fn malformed_lines_do_not_crash_parsing() {
        let parsed = parse_session_log(&fixture(
            "fixtures/local_history/sample_root/sessions/thr_malformed.jsonl",
        ))
        .expect("read fixture");

        let detail = parsed.detail.expect("parsed thread");
        assert!(parsed.malformed_lines > 0);
        assert_eq!(detail.summary.thread_id, "thr_malformed");
    }

    #[test]
    fn preserves_unknown_item_and_event_variants() {
        let parsed = parse_session_log(&fixture(
            "fixtures/local_history/sample_root/sessions/thr_malformed.jsonl",
        ))
        .expect("read fixture");

        let detail = parsed.detail.expect("parsed thread");
        let items = &detail.turns[0].items;
        assert!(items.iter().any(|item| matches!(
            item,
            Item::Other(UnknownItem { kind, .. }) if kind == "future_tool_result"
        )));
        assert!(items.iter().any(|item| matches!(
            item,
            Item::Other(UnknownItem { kind, .. }) if kind == "event:custom_event"
        )));
    }

    #[test]
    fn parses_turn_context_without_turn_id_and_shell_calls() {
        let parsed = parse_session_log(&fixture(
            "fixtures/local_history/sample_root/sessions/thr_simple_shard.jsonl",
        ))
        .expect("read fixture");

        let detail = parsed.detail.expect("parsed thread");
        assert_eq!(detail.summary.thread_id, "thr_simple");
        assert_eq!(detail.turns.len(), 1);
        assert_eq!(detail.items_count, 2);
        assert_eq!(detail.commands_count, 1);
        assert_eq!(detail.turns[0].status, "completed");
        assert!(detail.turns[0].turn_id.starts_with("implicit:"));
        assert!(detail.turns[0]
            .items
            .iter()
            .any(|item| matches!(item, Item::CommandExecution(command) if command.command.as_deref() == Some("cargo test -- --help") && command.output.as_deref() == Some("help ok"))));
    }

    #[test]
    fn reports_diagnostics_for_file_without_session_metadata() {
        let parsed = parse_session_log(&fixture(
            "fixtures/local_history/sample_root/sessions/thr_missing_meta.jsonl",
        ))
        .expect("read fixture");

        assert!(parsed.detail.is_none());
        assert!(parsed.malformed_lines > 0);
        assert_eq!(parsed.warnings, vec!["missing valid thread metadata"]);
    }

    #[test]
    fn parses_response_item_messages_and_reasoning() {
        let parsed = parse_session_log(&fixture(
            "fixtures/local_history/response_item_root/sessions/thr_response_item.jsonl",
        ))
        .expect("read fixture");

        let detail = parsed.detail.expect("parsed thread");
        assert_eq!(
            detail.summary.preview.as_deref(),
            Some("Summarize the parser edge cases.")
        );
        assert_eq!(detail.turns.len(), 1);
        assert_eq!(detail.items_count, 4);
        assert!(matches!(
            &detail.turns[0].items[0],
            Item::UserMessage(message)
                if message.text.as_deref() == Some("Repository policy goes here.")
                    && message.attributes.get("role") == Some(&Value::String("developer".into()))
        ));
        assert!(matches!(
            &detail.turns[0].items[1],
            Item::UserMessage(message)
                if message.text.as_deref() == Some("Summarize the parser edge cases.")
                    && message.attributes.get("role") == Some(&Value::String("user".into()))
        ));
        assert!(matches!(
            &detail.turns[0].items[2],
            Item::AgentMessage(message)
                if message.text.as_deref()
                    == Some("I found two parser regressions.")
        ));
        assert!(matches!(
            &detail.turns[0].items[3],
            Item::ReasoningSummary(summary)
                if summary.text.as_deref()
                    == Some("Planning parser follow-up")
        ));
    }
}
