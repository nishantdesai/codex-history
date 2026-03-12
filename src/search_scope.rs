use crate::model::Item;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SearchScope {
    pub include_thinking: bool,
    pub include_tools: bool,
}

impl SearchScope {
    pub fn includes_item(&self, item: &Item) -> bool {
        match item {
            Item::UserMessage(_) | Item::AgentMessage(_) => true,
            Item::ReasoningSummary(_) => self.include_thinking,
            Item::CommandExecution(_) | Item::WebSearch(_) | Item::McpToolCall(_) => {
                self.include_tools
            }
            Item::FileChange(_) | Item::Other(_) => false,
        }
    }

    pub fn includes_search_kind(&self, kind: &str) -> bool {
        match kind {
            "user_message" | "agent_message" => true,
            "reasoning_summary" => self.include_thinking,
            "command_execution" | "web_search" | "mcp_tool_call" => self.include_tools,
            "thread_name" | "thread_preview" | "file_change" => false,
            _ => false,
        }
    }

    pub fn search_kind_sql(&self) -> String {
        let mut values = vec!["'user_message'", "'agent_message'"];
        if self.include_thinking {
            values.push("'reasoning_summary'");
        }
        if self.include_tools {
            values.push("'command_execution'");
            values.push("'web_search'");
            values.push("'mcp_tool_call'");
        }
        values.join(", ")
    }
}
