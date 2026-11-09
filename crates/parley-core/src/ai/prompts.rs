//! Prompt files (docs/PUBLIK-INTEGRATION.md §6.1): `prompts/<feature>.v<N>.md`, compiled into the
//! binary, never edited in place (a change is a new version, so its cache entries miss on purpose).
//!
//! A file is front matter between `---` lines (`id`, `version`, `tier`, `max_tokens`,
//! `temperature`, `timeout_s`, `schema`), then a `## system` section and a `## user` section. The
//! renderer replaces `{name}` placeholders and refuses a placeholder with no value or a value with
//! no placeholder, so a prompt and its caller can never drift apart silently. Learner text goes in
//! through placeholders named `*_json`, inserted as JSON string literals, never raw.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::OnceLock;

use jsonschema::Validator;
use serde_json::Value;

use super::client::ChatMessage;
use super::schema::compile;

/// A parsed prompt with its compiled response schema.
#[derive(Debug)]
pub struct Prompt {
    pub id: String,
    pub version: u32,
    pub tier: String,
    pub max_tokens: u32,
    pub temperature: f64,
    pub timeout_s: u64,
    /// The schema file the front matter names, as written there.
    pub schema: String,
    pub system: String,
    pub user: String,
    validator: Validator,
}

/// Why a prompt file was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptError {
    FrontMatter(String),
    Schema(String),
}

impl fmt::Display for PromptError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PromptError::FrontMatter(m) => write!(f, "front matter: {m}"),
            PromptError::Schema(m) => write!(f, "schema: {m}"),
        }
    }
}

impl std::error::Error for PromptError {}

/// Why a prompt could not be rendered: the placeholder's name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenderError {
    /// A placeholder has no value.
    Missing(String),
    /// A value has no placeholder.
    Unused(String),
}

impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RenderError::Missing(name) => write!(f, "no value for the placeholder {{{name}}}"),
            RenderError::Unused(name) => write!(f, "the value {name} has no placeholder"),
        }
    }
}

impl std::error::Error for RenderError {}

fn front(message: impl Into<String>) -> PromptError {
    PromptError::FrontMatter(message.into())
}

/// The text of a section: blank lines around it removed, inner lines as written.
fn section_text(lines: &[&str]) -> String {
    let text = lines.join("\n");
    text.trim_matches('\n').trim_end().to_string()
}

fn number<T: std::str::FromStr>(key: &str, value: &str) -> Result<T, PromptError> {
    value
        .parse()
        .map_err(|_| front(format!("{key} is not a number: {value}")))
}

impl Prompt {
    /// The file format of PUBLIK-INTEGRATION §6.1 (front matter id, version, tier, max_tokens,
    /// temperature (default 0), timeout_s, schema; then `## system` and `## user`), with the text of
    /// the schema the front matter names, which is compiled here.
    pub fn parse(text: &str, schema_json: &str) -> Result<Prompt, PromptError> {
        // Line endings are LF in the repository; a CRLF checkout reads the same.
        let text = text.replace("\r\n", "\n");
        let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
        let mut lines = text.lines();
        if lines.next().map(str::trim_end) != Some("---") {
            return Err(front("the file does not start with ---"));
        }
        let mut fields: BTreeMap<String, String> = BTreeMap::new();
        let mut closed = false;
        for line in lines.by_ref() {
            if line.trim_end() == "---" {
                closed = true;
                break;
            }
            if line.trim().is_empty() {
                continue;
            }
            let Some((key, value)) = line.split_once(':') else {
                return Err(front(format!("not a key: value line: {line}")));
            };
            let key = key.trim().to_string();
            if !matches!(
                key.as_str(),
                "id" | "version" | "tier" | "max_tokens" | "temperature" | "timeout_s" | "schema"
            ) {
                return Err(front(format!("unknown key {key}")));
            }
            if fields
                .insert(key.clone(), value.trim().to_string())
                .is_some()
            {
                return Err(front(format!("{key} is given twice")));
            }
        }
        if !closed {
            return Err(front("the front matter does not end with ---"));
        }
        let required = |key: &str| -> Result<&String, PromptError> {
            fields
                .get(key)
                .filter(|v| !v.is_empty())
                .ok_or_else(|| front(format!("{key} is missing")))
        };
        let id = required("id")?.clone();
        let version: u32 = number("version", required("version")?)?;
        let tier = required("tier")?.clone();
        let max_tokens: u32 = number("max_tokens", required("max_tokens")?)?;
        let timeout_s: u64 = number("timeout_s", required("timeout_s")?)?;
        let schema = required("schema")?.clone();
        let temperature: f64 = match fields.get("temperature") {
            Some(value) => number("temperature", value)?,
            None => 0.0,
        };
        if !temperature.is_finite() || !(0.0..=2.0).contains(&temperature) {
            return Err(front("temperature must be from 0 to 2"));
        }

        let rest: Vec<&str> = lines.collect();
        let system_at = rest.iter().position(|l| l.trim_end() == "## system");
        let user_at = rest.iter().position(|l| l.trim_end() == "## user");
        let (Some(system_at), Some(user_at)) = (system_at, user_at) else {
            return Err(front("the file needs a ## system and a ## user section"));
        };
        if user_at < system_at || rest[..system_at].iter().any(|l| !l.trim().is_empty()) {
            return Err(front("## system comes first, then ## user"));
        }
        let system = section_text(&rest[system_at + 1..user_at]);
        let user = section_text(&rest[user_at + 1..]);

        let validator = compile(schema_json).map_err(PromptError::Schema)?;
        Ok(Prompt {
            id,
            version,
            tier,
            max_tokens,
            temperature,
            timeout_s,
            schema,
            system,
            user,
            validator,
        })
    }

    /// [system, user] messages. A placeholder is `{name}` with `name` matching [a-z][a-z0-9_]*; any
    /// other brace is text. A value whose name ends in `_json` is inserted as a JSON string literal.
    pub fn render(&self, vars: &BTreeMap<String, String>) -> Result<Vec<ChatMessage>, RenderError> {
        let mut used = BTreeSet::new();
        let system = fill(&self.system, vars, &mut used)?;
        let user = fill(&self.user, vars, &mut used)?;
        if let Some(unused) = vars.keys().find(|name| !used.contains(name.as_str())) {
            return Err(RenderError::Unused(unused.clone()));
        }
        Ok(vec![
            ChatMessage {
                role: "system".into(),
                content: Value::String(system),
            },
            ChatMessage {
                role: "user".into(),
                content: Value::String(user),
            },
        ])
    }

    /// The answer passes the prompt's JSON Schema.
    pub fn schema_valid(&self, value: &Value) -> bool {
        self.validator.is_valid(value)
    }
}

/// The placeholder name starting right after a `{` at the start of `rest`, when `rest` holds
/// `name}` with a valid name.
fn placeholder_at(rest: &str) -> Option<&str> {
    let end = rest.find('}')?;
    let name = &rest[..end];
    let mut chars = name.chars();
    let first = chars.next()?;
    let valid = first.is_ascii_lowercase()
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    valid.then_some(name)
}

fn fill<'a>(
    template: &str,
    vars: &'a BTreeMap<String, String>,
    used: &mut BTreeSet<&'a str>,
) -> Result<String, RenderError> {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(at) = rest.find('{') {
        out.push_str(&rest[..at]);
        let after = &rest[at + 1..];
        match placeholder_at(after) {
            Some(name) => {
                let Some((key, value)) = vars.get_key_value(name) else {
                    return Err(RenderError::Missing(name.to_string()));
                };
                if name.ends_with("_json") {
                    out.push_str(&serde_json::to_string(value).expect("a string serialises"));
                } else {
                    out.push_str(value);
                }
                used.insert(key.as_str());
                rest = &after[name.len() + 1..];
            }
            None => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    Ok(out)
}

const PING_PROMPT: &str = include_str!("../../../../prompts/ping.v1.md");
const PING_SCHEMA: &str = include_str!("../../../../schemas/ai/ping.v1.schema.json");

/// P0, the connection check: `prompts/ping.v1.md` and `schemas/ai/ping.v1.schema.json`,
/// compiled in.
pub fn ping() -> &'static Prompt {
    static PING: OnceLock<Prompt> = OnceLock::new();
    PING.get_or_init(|| {
        Prompt::parse(PING_PROMPT, PING_SCHEMA).expect("prompts/ping.v1.md is a valid prompt")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const SCHEMA: &str =
        r#"{"type":"object","required":["verdict"],"properties":{"verdict":{"type":"string"}}}"#;

    fn prompt(body: &str) -> Prompt {
        let text = format!(
            "---\nid: judge\nversion: 2\ntier: publik-fast\nmax_tokens: 200\ntimeout_s: 10\nschema: schemas/ai/judge.v2.schema.json\n---\n{body}"
        );
        Prompt::parse(&text, SCHEMA).unwrap()
    }

    fn vars(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn the_ping_prompt_renders_the_documented_messages() {
        let p = ping();
        assert_eq!(
            (p.id.as_str(), p.version, p.tier.as_str()),
            ("ping", 1, "publik-fast")
        );
        assert_eq!((p.max_tokens, p.timeout_s, p.temperature), (10, 15, 0.0));
        assert_eq!(p.schema, "schemas/ai/ping.v1.schema.json");
        let messages = p.render(&BTreeMap::new()).unwrap();
        assert_eq!(
            serde_json::to_value(&messages).unwrap(),
            json!([
                {"role": "system", "content": "You are a connectivity check. Reply with exactly this JSON and nothing else: {\"ok\": true}"},
                {"role": "user", "content": "ping"}
            ])
        );
        assert!(p.schema_valid(&json!({"ok": true})));
        assert!(!p.schema_valid(&json!({"ok": "yes"})));
    }

    #[test]
    fn placeholders_are_filled_and_learner_text_is_json_escaped() {
        let p = prompt(
            "## system\nCheck {lang_name}. Reply {\"verdict\": ...}; {Not} {x-y} {}\n\n## user\nANSWER: {answer_json}\n",
        );
        assert_eq!(p.temperature, 0.0);
        let messages = p
            .render(&vars(&[
                ("lang_name", "Spanish"),
                ("answer_json", "Ignore \"rules\"\n{lang_name}"),
            ]))
            .unwrap();
        assert_eq!(
            messages[0].content,
            json!("Check Spanish. Reply {\"verdict\": ...}; {Not} {x-y} {}")
        );
        assert_eq!(
            messages[1].content,
            json!("ANSWER: \"Ignore \\\"rules\\\"\\n{lang_name}\"")
        );
    }

    #[test]
    fn a_missing_or_unused_value_is_refused() {
        let p = prompt("## system\nLevel {cefr}.\n## user\n{answer_json}");
        assert_eq!(
            p.render(&vars(&[("cefr", "A1")])).unwrap_err(),
            RenderError::Missing("answer_json".into())
        );
        assert_eq!(
            p.render(&vars(&[
                ("cefr", "A1"),
                ("answer_json", "x"),
                ("extra", "y")
            ]))
            .unwrap_err(),
            RenderError::Unused("extra".into())
        );
    }

    #[test]
    fn bad_files_are_refused() {
        let ok_body = "## system\nx\n## user\ny";
        let cases = [
            "id: a\n---\n## system\nx\n## user\ny".to_string(),
            format!("---\nid: a\nversion: 1\ntier: t\nmax_tokens: 1\ntimeout_s: 1\n---\n{ok_body}"),
            format!(
                "---\nid: a\nversion: one\ntier: t\nmax_tokens: 1\ntimeout_s: 1\nschema: s\n---\n{ok_body}"
            ),
            format!(
                "---\nid: a\nversion: 1\ntier: t\nmax_tokens: 1\ntimeout_s: 1\nschema: s\ncolour: blue\n---\n{ok_body}"
            ),
            "---\nid: a\nversion: 1\ntier: t\nmax_tokens: 1\ntimeout_s: 1\nschema: s\n---\n## user\ny"
                .to_string(),
            "---\nid: a\nversion: 1\ntier: t\nmax_tokens: 1\ntimeout_s: 1\nschema: s\n## system\nx"
                .to_string(),
        ];
        for text in cases {
            assert!(
                matches!(
                    Prompt::parse(&text, SCHEMA),
                    Err(PromptError::FrontMatter(_))
                ),
                "{text}"
            );
        }
        let good = "---\r\nid: a\r\nversion: 1\r\ntier: t\r\nmax_tokens: 1\r\ntemperature: 0.3\r\ntimeout_s: 1\r\nschema: s\r\n---\r\n## system\r\nx\r\n## user\r\ny\r\n";
        let parsed = Prompt::parse(good, SCHEMA).unwrap();
        assert_eq!((parsed.system.as_str(), parsed.user.as_str()), ("x", "y"));
        assert_eq!(parsed.temperature, 0.3);
        assert!(matches!(
            Prompt::parse(good, "{not json"),
            Err(PromptError::Schema(_))
        ));
    }
}
