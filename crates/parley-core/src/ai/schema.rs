//! JSON Schema checks of AI answers (docs/PUBLIK-INTEGRATION.md §6.1, Validation): the JSON of an
//! answer is cut out of the reply text, then checked against the feature's schema before anything
//! else looks at it (invariant I2).

use jsonschema::Validator;
use serde_json::Value;

/// Compiles a JSON Schema given as text. The core resolves no remote references, so compiling
/// never reaches the network.
pub(crate) fn compile(schema_json: &str) -> Result<Validator, String> {
    let schema: Value =
        serde_json::from_str(schema_json).map_err(|e| format!("the schema is not JSON: {e}"))?;
    jsonschema::validator_for(&schema).map_err(|e| format!("the schema does not compile: {e}"))
}

/// The JSON of an answer: from the first `{` to the last `}`, parsed as it is; nothing else is tolerated.
pub fn parse_reply(content: &str) -> Option<Value> {
    let start = content.find('{')?;
    let end = content.rfind('}')?;
    if end < start {
        return None;
    }
    serde_json::from_str(&content[start..=end]).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_reply_is_cut_from_the_first_to_the_last_brace() {
        assert_eq!(parse_reply("{\"ok\": true}"), Some(json!({"ok": true})));
        assert_eq!(
            parse_reply("Sure! ```json\n{\"ok\": true}\n```"),
            Some(json!({"ok": true}))
        );
        assert_eq!(
            parse_reply("{\"a\": {\"b\": 1}} trailing"),
            Some(json!({"a": {"b": 1}}))
        );
        for bad in [
            "",
            "ok",
            "} {",
            "{\"ok\": tru}",
            "{\"a\": 1} and {\"b\": 2}",
        ] {
            assert_eq!(parse_reply(bad), None, "{bad}");
        }
    }

    #[test]
    fn schemas_compile_from_text() {
        let validator = compile(
            r#"{"type":"object","required":["ok"],"properties":{"ok":{"const":true}},"additionalProperties":false}"#,
        )
        .unwrap();
        assert!(validator.is_valid(&json!({"ok": true})));
        assert!(!validator.is_valid(&json!({"ok": false})));
        assert!(!validator.is_valid(&json!({"ok": true, "more": 1})));
        assert!(compile("{").is_err());
        assert!(compile(r#"{"type": 3}"#).is_err());
    }
}
