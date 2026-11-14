//! `parley whisper download`: one local Whisper model of the bundled registry, downloaded and
//! checked (docs/modules/10-speaking-and-pronunciation.md, CLI). It reaches the real network, so
//! it is for people and the CI job `whisper`, never for tests.

use std::io::Write;
use std::path::Path;

use parley_core::env::{HttpRequest, Net};
use parley_core::speech::{ModelEntry, WHISPER_DIR, bundled_models, model_verified};
use parley_publik::http::HttpNet;
use serde::Serialize;

use super::{failure, join, missing, parse, print_json, unknown, usage};

pub fn run(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    match args.first().map(String::as_str) {
        Some("download") => download_command(&args[1..], &HttpNet::new(), out, err),
        Some(other) => unknown(err, other),
        None => missing(err, "whisper download"),
    }
}

#[derive(Serialize)]
struct Downloaded {
    name: String,
    path: String,
    bytes: u64,
}

/// Downloads `entry` into `dir` as `ggml-<name>.bin.part`, renamed to `ggml-<name>.bin` once its
/// length and SHA-256 match; a damaged download is removed. The error is the line to print.
fn download(net: &dyn Net, entry: &ModelEntry, dir: &str) -> Result<String, String> {
    let response = net
        .send(HttpRequest {
            method: "GET".into(),
            url: entry.url.clone(),
            headers: Vec::new(),
            body: Vec::new(),
        })
        .map_err(|_| "You're offline. Download the speech model when you're connected.")?;
    if response.status != 200 {
        return Err("The speech model could not be downloaded. Try again later.".into());
    }
    let file_name = format!("ggml-{}.bin", entry.name);
    let target = Path::new(dir).join(&file_name);
    let part = Path::new(dir).join(format!("{file_name}.part"));
    std::fs::create_dir_all(dir).map_err(|e| format!("{dir}: {e}"))?;
    std::fs::write(&part, &response.body).map_err(|e| format!("{}: {e}", join(dir, &file_name)))?;
    if !model_verified(entry, &response.body) {
        let _ = std::fs::remove_file(&part);
        return Err("The download was damaged. Try again.".into());
    }
    std::fs::rename(&part, &target).map_err(|e| {
        let _ = std::fs::remove_file(&part);
        format!("{}: {e}", join(dir, &file_name))
    })?;
    Ok(join(dir, &file_name))
}

/// `parley whisper download <tiny|base|small> [--dir <dir>]`: prints {"name", "path", "bytes"}.
fn download_command(
    args: &[String],
    net: &dyn Net,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> u8 {
    let parsed = match parse(args, &["--dir"], &["--json"]) {
        Ok(parsed) => parsed,
        Err(message) => return usage(err, &message),
    };
    if let Some(extra) = parsed.positional.get(1) {
        return unknown(err, extra);
    }
    let Some(name) = parsed.positional.first() else {
        return missing(err, "<tiny|base|small>");
    };
    let models = bundled_models();
    let Some(entry) = models.get(name) else {
        let names: Vec<&str> = models.whisper.iter().map(|m| m.name.as_str()).collect();
        return usage(
            err,
            &format!(
                "unknown model {name}; use {}. Run parley --help.",
                names.join(", ")
            ),
        );
    };
    let dir = parsed.value("--dir").unwrap_or(WHISPER_DIR);
    match download(net, entry, dir) {
        Ok(path) => {
            print_json(
                out,
                &Downloaded {
                    name: entry.name.clone(),
                    path,
                    bytes: entry.bytes,
                },
            );
            0
        }
        Err(message) => failure(err, &message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use parley_core::env::{HttpResponse, NetError};

    /// A network that answers every request with one response.
    struct Answer(Result<HttpResponse, NetError>);

    impl Net for Answer {
        fn send(&self, _req: HttpRequest) -> Result<HttpResponse, NetError> {
            self.0.clone()
        }
    }

    fn ok(body: &[u8]) -> Answer {
        Answer(Ok(HttpResponse {
            status: 200,
            headers: Vec::new(),
            body: body.to_vec(),
        }))
    }

    /// The SHA-256 of "abc" (FIPS 180-2, appendix B.1).
    fn entry() -> ModelEntry {
        ModelEntry {
            name: "t".into(),
            url: "https://example.invalid/t.bin".into(),
            bytes: 3,
            sha256: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad".into(),
        }
    }

    #[test]
    fn a_verified_file_takes_its_name_and_a_damaged_one_is_removed() {
        let dir = tempfile::tempdir().unwrap();
        let dir = dir.path().to_str().unwrap().to_string();
        assert_eq!(
            download(&ok(b"abd"), &entry(), &dir),
            Err("The download was damaged. Try again.".into())
        );
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
        let path = download(&ok(b"abc"), &entry(), &dir).unwrap();
        assert!(path.ends_with("/ggml-t.bin"));
        assert_eq!(
            std::fs::read(Path::new(&dir).join("ggml-t.bin")).unwrap(),
            b"abc"
        );
        assert_eq!(
            download(&Answer(Err(NetError::Offline)), &entry(), &dir),
            Err("You're offline. Download the speech model when you're connected.".into())
        );
        let missing = Answer(Ok(HttpResponse {
            status: 404,
            headers: Vec::new(),
            body: Vec::new(),
        }));
        assert_eq!(
            download(&missing, &entry(), &dir),
            Err("The speech model could not be downloaded. Try again later.".into())
        );
    }

    #[test]
    fn usage_errors_send_nothing() {
        let mut out = Vec::new();
        let mut err = Vec::new();
        let never = Answer(Err(NetError::Disabled));
        let args = |list: &[&str]| -> Vec<String> { list.iter().map(|s| s.to_string()).collect() };
        assert_eq!(download_command(&args(&[]), &never, &mut out, &mut err), 2);
        assert_eq!(
            download_command(&args(&["large"]), &never, &mut out, &mut err),
            2
        );
        let text = String::from_utf8(err).unwrap();
        assert!(text.contains("error: <tiny|base|small> is required. Run parley --help."));
        assert!(
            text.contains("error: unknown model large; use tiny, base, small. Run parley --help.")
        );
        assert!(out.is_empty());
    }
}
