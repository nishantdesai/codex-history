use std::env;
use std::sync::OnceLock;

use regex::{Captures, Regex};
use serde::Serialize;
use serde_json::Value;

const REDACTED: &str = "[REDACTED]";

pub fn redact_human_text(text: &str) -> String {
    sanitize_home_paths(&redact_secrets(text))
}

pub fn redact_error_text(text: &str) -> String {
    redact_human_text(text)
}

pub fn to_redacted_json_string<T>(value: &T, pretty: bool) -> Result<String, String>
where
    T: Serialize + ?Sized,
{
    let text = if pretty {
        serde_json::to_string_pretty(value)
            .map_err(|error| format!("failed to serialize JSON output: {error}"))?
    } else {
        serde_json::to_string(value)
            .map_err(|error| format!("failed to serialize output: {error}"))?
    };

    Ok(redact_serialized_json(&text))
}

pub fn redact_json_value(mut value: Value) -> Value {
    redact_json_value_inner(None, &mut value);
    value
}

fn redact_json_value_inner(key: Option<&str>, value: &mut Value) {
    match value {
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
        Value::String(text) => {
            if key.is_some_and(is_secret_key) {
                *text = REDACTED.to_string();
            } else {
                *text = redact_secrets(text);
            }
        }
        Value::Array(values) => {
            for value in values {
                redact_json_value_inner(None, value);
            }
        }
        Value::Object(map) => {
            for (key, value) in map {
                redact_json_value_inner(Some(key), value);
            }
        }
    }
}

fn redact_secrets(text: &str) -> String {
    let mut redacted = text.to_string();

    redacted = replace_all(jwt_regex(), &redacted, REDACTED);
    redacted = bearer_regex()
        .replace_all(&redacted, |caps: &Captures| {
            format!("{}{}", &caps[1], REDACTED)
        })
        .into_owned();
    redacted = replace_all(
        secret_assignment_quoted_regex(),
        &redacted,
        "$1$2[REDACTED]$2",
    );
    redacted = replace_all(secret_assignment_regex(), &redacted, "$1[REDACTED]");
    replace_all(common_token_regex(), &redacted, REDACTED)
}

fn redact_serialized_json(text: &str) -> String {
    let redacted = replace_all(json_secret_field_regex(), text, "$1[REDACTED]$3");
    redact_secrets(&redacted)
}

fn sanitize_home_paths(text: &str) -> String {
    let mut sanitized = text.to_string();

    if let Some(home) = env::var_os("HOME") {
        let home = home.to_string_lossy();
        if !home.is_empty() {
            sanitized = sanitized.replace(home.as_ref(), "~");
        }
    }

    generic_home_regex()
        .replace_all(&sanitized, |caps: &Captures| {
            let prefix = caps
                .name("prefix")
                .map(|value| value.as_str())
                .unwrap_or("");
            let suffix = caps
                .name("suffix")
                .map(|value| value.as_str())
                .unwrap_or("");
            format!("{prefix}~{suffix}")
        })
        .into_owned()
}

fn is_secret_key(key: &str) -> bool {
    let normalized = key
        .trim()
        .trim_matches(|ch| ch == '"' || ch == '\'')
        .to_ascii_lowercase()
        .replace('-', "_");

    matches!(
        normalized.as_str(),
        "api_key"
            | "apikey"
            | "access_token"
            | "auth_token"
            | "refresh_token"
            | "session_token"
            | "token"
            | "secret"
            | "password"
            | "passwd"
            | "cookie"
            | "authorization"
            | "bearer"
    )
}

fn replace_all(pattern: &Regex, text: &str, replacement: &str) -> String {
    pattern.replace_all(text, replacement).into_owned()
}

fn jwt_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r"\b[A-Za-z0-9_-]{16,}\.[A-Za-z0-9_-]{16,}\.[A-Za-z0-9_-]{16,}\b")
            .expect("jwt regex")
    })
}

fn bearer_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r"(?i)\b(Bearer\s+)([A-Za-z0-9._~+/\-=]{12,})").expect("bearer regex")
    })
}

fn secret_assignment_quoted_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(
            r#"(?i)(\b(?:x-api-key|api[_-]?key|access[_-]?token|auth[_-]?token|refresh[_-]?token|session[_-]?token|token|secret|password|passwd|cookie)\b\s*[:=]\s*)(["'])([^"'\r\n]{4,})["']"#,
        )
        .expect("quoted secret assignment regex")
    })
}

fn secret_assignment_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(
            r"(?i)(\b(?:x-api-key|api[_-]?key|access[_-]?token|auth[_-]?token|refresh[_-]?token|session[_-]?token|token|secret|password|passwd|cookie)\b\s*[:=]\s*)([A-Za-z0-9._~+/\-=]{8,})",
        )
        .expect("secret assignment regex")
    })
}

fn common_token_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(
            r"\b(?:gh[pousr]_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{20,}|sk-[A-Za-z0-9_-]{16,}|rk-[A-Za-z0-9_-]{16,}|glpat-[A-Za-z0-9_-]{16,}|xox[baprs]-[A-Za-z0-9-]{10,}|AIza[0-9A-Za-z_-]{20,}|AKIA[0-9A-Z]{16})\b",
        )
        .expect("common token regex")
    })
}

fn generic_home_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(
            r#"(?P<prefix>^|[\s`"'(])/(?:(?:Users)|(?:home))/(?P<user>[^/\s`"'()]+)(?P<suffix>(?:/[^\s`"'()]+)*)"#,
        )
        .expect("generic home path regex")
    })
}

fn json_secret_field_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(
            r#"("(?i:api[_-]?key|apikey|access[_-]?token|auth[_-]?token|refresh[_-]?token|session[_-]?token|token|secret|password|passwd|cookie|authorization|bearer)"\s*:\s*")((?:\\.|[^"\\])*)(")"#,
        )
        .expect("json secret field regex")
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn human_redaction_masks_common_secret_patterns() {
        let text = concat!(
            "Authorization: Bearer sk-live_1234567890abcdefghijklmnop ",
            "api_key=plainsecret123456 ",
            "jwt=eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.",
            "eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkNvZGV4In0.",
            "signaturepayload1234567890 ",
            "token ghp_123456789012345678901234567890123456"
        );

        let redacted = redact_human_text(text);

        assert!(!redacted.contains("sk-live_1234567890abcdefghijklmnop"));
        assert!(!redacted.contains("plainsecret123456"));
        assert!(!redacted.contains("eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9"));
        assert!(!redacted.contains("ghp_123456789012345678901234567890123456"));
        assert!(redacted.contains("Bearer [REDACTED]"));
        assert!(redacted.contains("api_key=[REDACTED]"));
        assert!(redacted.matches(REDACTED).count() >= 4);
    }

    #[test]
    fn human_redaction_sanitizes_home_style_paths() {
        let text = "cwd: /Users/alice/project file: /home/bob/worktree";
        let redacted = redact_human_text(text);
        assert_eq!(redacted, "cwd: ~/project file: ~/worktree");
    }

    #[test]
    fn json_redaction_preserves_structure() {
        let value = json!({
            "thread_id": "thr_secret",
            "token": "plainsecret123456",
            "nested": {
                "text": "Authorization: Bearer sk-live_1234567890abcdefghijklmnop",
                "jwt": "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkNvZGV4In0.signaturepayload1234567890"
            },
            "items": [
                "ghp_123456789012345678901234567890123456",
                {"api_key": "quotedsecret123456"}
            ]
        });

        let redacted = redact_json_value(value);

        assert_eq!(redacted["thread_id"], "thr_secret");
        assert_eq!(redacted["token"], REDACTED);
        assert_eq!(
            redacted["nested"]["text"],
            "Authorization: Bearer [REDACTED]"
        );
        assert_eq!(redacted["nested"]["jwt"], REDACTED);
        assert_eq!(redacted["items"][0], REDACTED);
        assert_eq!(redacted["items"][1]["api_key"], REDACTED);
    }

    #[test]
    fn redacted_json_string_is_valid_json() {
        let value = json!({
            "text": "api_key=plainsecret123456",
            "cwd": "/Users/alice/project"
        });

        let rendered = to_redacted_json_string(&value, true).expect("json renders");
        let parsed: Value = serde_json::from_str(&rendered).expect("valid json");

        assert_eq!(parsed["text"], "api_key=[REDACTED]");
        assert_eq!(parsed["cwd"], "/Users/alice/project");
    }
}
