//! The local Whisper models (docs/modules/10-speaking-and-pronunciation.md): `models/registry.toml`,
//! where each downloaded file lives, and the check that makes a download a model.
//!
//! Model weights are never committed (CONTRIBUTING.md): the registry pins each file's URL, size
//! and SHA-256, the app downloads one only when the learner asks, and a file takes its own name
//! only once its length and SHA-256 match, so a file under its name is always whole and verified.

use std::sync::OnceLock;

use sha2::{Digest, Sha256};

/// The folder of the models, under the data directory.
pub const WHISPER_DIR: &str = "models/whisper";
/// The model used until the learner picks another.
pub const DEFAULT_WHISPER_MODEL: &str = "base";

/// The registry's schema tag.
const MODELS_SCHEMA: &str = "parley.models/1";

/// One model file: its name, where it comes from, and its size and SHA-256.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelEntry {
    pub name: String,
    pub url: String,
    pub bytes: u64,
    pub sha256: String,
}

/// The models of `models/registry.toml`, ascending by size (tiny, base, small).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelRegistry {
    pub whisper: Vec<ModelEntry>,
}

impl ModelRegistry {
    /// The entry named `name`.
    pub fn get(&self, name: &str) -> Option<&ModelEntry> {
        self.whisper.iter().find(|entry| entry.name == name)
    }
}

/// A model name: one or more of `a-z` and `0-9`.
fn is_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
}

/// 64 lowercase hexadecimal digits.
fn is_sha256(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// One `[whisper.<name>]` table: exactly `url` (https://), `bytes` (> 0) and `sha256`.
fn entry(name: &str, value: &toml::Value) -> Result<ModelEntry, String> {
    if !is_name(name) {
        return Err(format!("model name {name} must be a-z and 0-9 only"));
    }
    let table = value
        .as_table()
        .ok_or_else(|| format!("[whisper.{name}] must be a table"))?;
    if let Some(extra) = table
        .keys()
        .find(|key| !["url", "bytes", "sha256"].contains(&key.as_str()))
    {
        return Err(format!(
            "unknown key {extra} in [whisper.{name}]; use url, bytes and sha256"
        ));
    }
    let url = table
        .get("url")
        .and_then(toml::Value::as_str)
        .filter(|url| url.starts_with("https://") && url.len() > "https://".len())
        .ok_or_else(|| format!("[whisper.{name}] url must be an https:// URL"))?;
    let bytes = table
        .get("bytes")
        .and_then(toml::Value::as_integer)
        .filter(|bytes| *bytes > 0)
        .ok_or_else(|| format!("[whisper.{name}] bytes must be a whole number above 0"))?;
    let sha256 = table
        .get("sha256")
        .and_then(toml::Value::as_str)
        .filter(|sha| is_sha256(sha))
        .ok_or_else(|| format!("[whisper.{name}] sha256 must be 64 lowercase hex digits"))?;
    Ok(ModelEntry {
        name: name.to_string(),
        url: url.to_string(),
        bytes: bytes as u64,
        sha256: sha256.to_string(),
    })
}

/// Reads `models/registry.toml`: `schema = "parley.models/1"` and tables `[whisper.<name>]` with
/// exactly `url` (https://), `bytes` (> 0) and `sha256` (64 lowercase hex); names `[a-z0-9]+`.
/// Anything else is refused, so a typo is caught when the file is edited. The entries come back
/// ascending by size.
pub fn parse_model_registry(text: &str) -> Result<ModelRegistry, String> {
    let table: toml::Table = text.parse().map_err(|e| format!("not TOML: {e}"))?;
    let mut schema = None;
    let mut whisper = Vec::new();
    for (key, value) in &table {
        match key.as_str() {
            "schema" => schema = value.as_str(),
            "whisper" => {
                let models = value
                    .as_table()
                    .ok_or_else(|| "whisper must be a table of models".to_string())?;
                for (name, model) in models {
                    whisper.push(entry(name, model)?);
                }
            }
            other => return Err(format!("unknown key {other}; use schema and whisper")),
        }
    }
    if schema != Some(MODELS_SCHEMA) {
        return Err(format!("schema must be {MODELS_SCHEMA}"));
    }
    whisper.sort_by(|a, b| a.bytes.cmp(&b.bytes).then_with(|| a.name.cmp(&b.name)));
    Ok(ModelRegistry { whisper })
}

const BUNDLED_MODELS: &str = include_str!("../../../../models/registry.toml");

/// The bundled `models/registry.toml`, parsed once (a unit test proves it parses).
pub fn bundled_models() -> ModelRegistry {
    static PARSED: OnceLock<ModelRegistry> = OnceLock::new();
    PARSED
        .get_or_init(|| {
            parse_model_registry(BUNDLED_MODELS)
                .expect("models/registry.toml is a valid model registry")
        })
        .clone()
}

/// The file of a model, relative to the data directory: `models/whisper/ggml-<name>.bin`.
pub fn model_file(name: &str) -> String {
    format!("{WHISPER_DIR}/{}", model_file_name(name))
}

/// `ggml-<name>.bin`, the name `Fs::list` gives for a downloaded model.
pub(crate) fn model_file_name(name: &str) -> String {
    format!("ggml-{name}.bin")
}

/// The length equals `bytes` and the SHA-256 equals `sha256`.
pub fn model_verified(entry: &ModelEntry, bytes: &[u8]) -> bool {
    if bytes.len() as u64 != entry.bytes {
        return false;
    }
    let digest: String = Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    digest.eq_ignore_ascii_case(&entry.sha256)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHA: &str = "60ed5bc3dd14eea856493d334349b405782ddcaf0028d4b5df4088345fba2efe";

    fn registry(model: &str) -> String {
        format!("schema = \"parley.models/1\"\n\n[whisper.base]\n{model}\n")
    }

    #[test]
    fn the_bundled_registry_lists_three_models_by_size() {
        let models = bundled_models();
        let names: Vec<&str> = models.whisper.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["tiny", "base", "small"]);
        let base = models.get("base").unwrap();
        assert_eq!(base.bytes, 147_951_465);
        assert_eq!(base.sha256, SHA);
        assert!(base.url.ends_with("/ggml-base.bin"));
        assert_eq!(models.get(DEFAULT_WHISPER_MODEL), Some(base));
        assert_eq!(models.get("large"), None);
        assert_eq!(model_file("base"), "models/whisper/ggml-base.bin");
    }

    #[test]
    fn anything_but_the_three_keys_is_refused() {
        let good =
            format!("url = \"https://example.invalid/m.bin\"\nbytes = 3\nsha256 = \"{SHA}\"");
        assert!(parse_model_registry(&registry(&good)).is_ok());
        for bad in [
            good.replace("https://", "http://"),
            good.replace(SHA, &SHA[1..]),
            good.replace(SHA, &SHA.to_uppercase()),
            good.replace("bytes = 3", "bytes = 0"),
            good.replace("bytes = 3", "bytes = \"3\""),
            format!("{good}\nmirror = \"x\""),
            good.replace("url = \"https://example.invalid/m.bin\"\n", ""),
        ] {
            assert!(parse_model_registry(&registry(&bad)).is_err(), "{bad}");
        }
        assert!(parse_model_registry(&registry(&good).replace("models/1", "models/2")).is_err());
        assert!(parse_model_registry(&format!("{}\nextra = 1\n", registry(&good))).is_err());
        assert!(
            parse_model_registry(&registry(&good).replace("whisper.base", "whisper.Base")).is_err()
        );
        assert!(parse_model_registry("schema = ").is_err());
    }

    #[test]
    fn a_file_is_verified_by_length_and_sha256() {
        // SHA-256 of "abc" (FIPS 180-2, appendix B.1).
        let entry = ModelEntry {
            name: "t".into(),
            url: "https://example.invalid/t.bin".into(),
            bytes: 3,
            sha256: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad".into(),
        };
        assert!(model_verified(&entry, b"abc"));
        assert!(!model_verified(&entry, b"abd"));
        assert!(!model_verified(&entry, b"abcd"));
    }
}
