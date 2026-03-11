use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    UserMessage(MessageItem),
    AgentMessage(MessageItem),
    CommandExecution(CommandExecutionItem),
    FileChange(FileChangeItem),
    ReasoningSummary(ReasoningSummaryItem),
    WebSearch(WebSearchItem),
    McpToolCall(McpToolCallItem),
    Other(UnknownItem),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct MessageItem {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, flatten)]
    pub attributes: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct CommandExecutionItem {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    #[serde(default, flatten)]
    pub attributes: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct FileChangeItem {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub change_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, flatten)]
    pub attributes: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ReasoningSummaryItem {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, flatten)]
    pub attributes: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct WebSearchItem {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, flatten)]
    pub attributes: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct McpToolCallItem {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arguments: Option<Value>,
    #[serde(default, flatten)]
    pub attributes: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct UnknownItem {
    pub kind: String,
    #[serde(default, flatten)]
    pub data: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct RawItem {
    kind: String,
    #[serde(default, flatten)]
    data: BTreeMap<String, Value>,
}

impl Item {
    pub fn kind(&self) -> &str {
        match self {
            Item::UserMessage(_) => "user_message",
            Item::AgentMessage(_) => "agent_message",
            Item::CommandExecution(_) => "command_execution",
            Item::FileChange(_) => "file_change",
            Item::ReasoningSummary(_) => "reasoning_summary",
            Item::WebSearch(_) => "web_search",
            Item::McpToolCall(_) => "mcp_tool_call",
            Item::Other(other) => &other.kind,
        }
    }
}

impl Serialize for Item {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        RawItem::from(self).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Item {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawItem::deserialize(deserializer)?;
        Item::try_from(raw).map_err(serde::de::Error::custom)
    }
}

impl From<&Item> for RawItem {
    fn from(item: &Item) -> Self {
        match item {
            Item::UserMessage(message) => RawItem {
                kind: "user_message".into(),
                data: to_map(message),
            },
            Item::AgentMessage(message) => RawItem {
                kind: "agent_message".into(),
                data: to_map(message),
            },
            Item::CommandExecution(command) => RawItem {
                kind: "command_execution".into(),
                data: to_map(command),
            },
            Item::FileChange(change) => RawItem {
                kind: "file_change".into(),
                data: to_map(change),
            },
            Item::ReasoningSummary(summary) => RawItem {
                kind: "reasoning_summary".into(),
                data: to_map(summary),
            },
            Item::WebSearch(search) => RawItem {
                kind: "web_search".into(),
                data: to_map(search),
            },
            Item::McpToolCall(call) => RawItem {
                kind: "mcp_tool_call".into(),
                data: to_map(call),
            },
            Item::Other(other) => RawItem {
                kind: other.kind.clone(),
                data: other.data.clone(),
            },
        }
    }
}

impl TryFrom<RawItem> for Item {
    type Error = String;

    fn try_from(raw: RawItem) -> Result<Self, Self::Error> {
        match raw.kind.as_str() {
            "user_message" => Ok(Item::UserMessage(from_map(raw.data)?)),
            "agent_message" => Ok(Item::AgentMessage(from_map(raw.data)?)),
            "command_execution" => Ok(Item::CommandExecution(from_map(raw.data)?)),
            "file_change" => Ok(Item::FileChange(from_map(raw.data)?)),
            "reasoning_summary" => Ok(Item::ReasoningSummary(from_map(raw.data)?)),
            "web_search" => Ok(Item::WebSearch(from_map(raw.data)?)),
            "mcp_tool_call" => Ok(Item::McpToolCall(from_map(raw.data)?)),
            _ => Ok(Item::Other(UnknownItem {
                kind: raw.kind,
                data: raw.data,
            })),
        }
    }
}

fn to_map<T>(value: &T) -> BTreeMap<String, Value>
where
    T: Serialize,
{
    let Value::Object(map) = serde_json::to_value(value).expect("item payload should serialize")
    else {
        unreachable!("item payload must serialize to an object");
    };

    map.into_iter().collect()
}

fn from_map<T>(map: BTreeMap<String, Value>) -> Result<T, String>
where
    T: DeserializeOwned,
{
    serde_json::from_value(Value::Object(map.into_iter().collect()))
        .map_err(|error| format!("invalid item payload: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_and_deserializes_known_item_variants() {
        let item = Item::UserMessage(MessageItem {
            text: Some("please inspect the worktree".into()),
            attributes: BTreeMap::from([("source".into(), Value::String("cli".into()))]),
        });

        let json = serde_json::to_value(&item).expect("serialize");
        assert_eq!(json["kind"], "user_message");
        assert_eq!(json["text"], "please inspect the worktree");
        assert_eq!(json["source"], "cli");

        let decoded: Item = serde_json::from_value(json).expect("deserialize");
        assert_eq!(decoded, item);
        assert_eq!(decoded.kind(), "user_message");
    }

    #[test]
    fn preserves_unknown_item_kinds_and_payloads() {
        let json = serde_json::json!({
            "kind": "future_tool_result",
            "text": "some future payload",
            "nested": { "ok": true }
        });

        let decoded: Item = serde_json::from_value(json.clone()).expect("deserialize");
        let Item::Other(other) = decoded else {
            panic!("expected unknown item");
        };

        assert_eq!(other.kind, "future_tool_result");
        assert_eq!(
            other.data.get("text"),
            Some(&Value::String("some future payload".into()))
        );
        assert_eq!(
            other.data.get("nested"),
            Some(&serde_json::json!({ "ok": true }))
        );

        let encoded = serde_json::to_value(Item::Other(other)).expect("serialize");
        assert_eq!(encoded, json);
    }

    #[test]
    fn deserializes_command_execution_payload() {
        let json = serde_json::json!({
            "kind": "command_execution",
            "command": "cargo test",
            "exit_code": 0,
            "cwd": "/workspace/project",
            "output": "ok"
        });

        let item: Item = serde_json::from_value(json).expect("deserialize");
        assert_eq!(
            item,
            Item::CommandExecution(CommandExecutionItem {
                command: Some("cargo test".into()),
                exit_code: Some(0),
                cwd: Some(PathBuf::from("/workspace/project")),
                output: Some("ok".into()),
                attributes: BTreeMap::new(),
            })
        );
    }
}
