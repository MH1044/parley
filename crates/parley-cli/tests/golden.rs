//! The golden harness (docs/TESTING.md §1): every directory `tests/cases/NN-slug/<case>/` is one
//! test that runs the `parley` binary and compares its standard output, parsed as JSON, with the
//! case's `expected.json`.
//!
//! A case directory holds:
//! - `args.txt`: one command-line argument per line. The token `{input}` is replaced by the path
//!   of the case's `input.json`, for commands that read a file; `{tmp}` by a fresh temporary
//!   directory (forward slashes), for commands that write a file.
//! - `stdin.txt` (optional): written to the command's standard input.
//! - `exit.txt` (optional): the expected exit code, default 0, so a case that must exit 1 by
//!   design (a syllabus with errors) can be a golden case.
//! - `expected.json`: the expected output. Integers and strings compare exactly; floats compare
//!   within the tolerance of `tolerance.json` (optional, `{"path.to.field": 1e-4}`, array indexes
//!   as numbers), default 1e-9.
//!
//! The command runs from the repository root, so paths in `args.txt` are relative to it. Setting
//! `PARLEY_BLESS=NN-slug` rewrites the expected files of that one module instead of comparing,
//! for a new case or a change the spec and commit body describe; the diff is then read line by
//! line, because a golden file is the specification, not a cache.
//!
//! Run one module's cases with `cargo test -p parley-cli --test golden -- NN-slug`.

use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use libtest_mimic::{Arguments, Failed, Trial};
use serde_json::Value;

fn main() {
    let args = Arguments::from_args();
    let root = repo_root();
    let cases = root.join("tests").join("cases");

    let mut trials = Vec::new();
    for module_dir in sorted_dirs(&cases) {
        let module = file_name(&module_dir);
        for case_dir in sorted_dirs(&module_dir) {
            let name = format!("{module}/{}", file_name(&case_dir));
            let root = root.clone();
            let module = module.clone();
            trials.push(Trial::test(name, move || {
                run_case(&root, &module, &case_dir)
            }));
        }
    }

    libtest_mimic::run(&args, trials).exit();
}

/// The repository root: the first ancestor of this crate that holds the workspace Cargo.toml.
fn repo_root() -> PathBuf {
    let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    loop {
        let manifest = dir.join("Cargo.toml");
        if let Ok(text) = fs::read_to_string(&manifest)
            && text.contains("[workspace]")
        {
            return dir;
        }
        if !dir.pop() {
            panic!(
                "no workspace Cargo.toml above {}",
                env!("CARGO_MANIFEST_DIR")
            );
        }
    }
}

fn sorted_dirs(dir: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = match fs::read_dir(dir) {
        Ok(entries) => entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect(),
        Err(_) => Vec::new(),
    };
    dirs.sort();
    dirs
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn run_case(root: &Path, module: &str, case: &Path) -> Result<(), Failed> {
    let args_text = fs::read_to_string(case.join("args.txt"))
        .map_err(|e| format!("args.txt could not be read: {e}"))?;
    let input_path = case.join("input.json").to_string_lossy().into_owned();
    // Lives until the end of the case, so a file the command writes can be read back.
    let tmp = tempfile::tempdir().map_err(|e| format!("no temporary directory: {e}"))?;
    let tmp_path = tmp.path().to_string_lossy().replace('\\', "/");
    let args: Vec<String> = args_text
        .lines()
        .map(|line| line.trim_end_matches('\r'))
        .filter(|line| !line.is_empty())
        .map(|line| {
            line.replace("{input}", &input_path)
                .replace("{tmp}", &tmp_path)
        })
        .collect();
    let expected_exit: i32 = match fs::read_to_string(case.join("exit.txt")) {
        Ok(text) => text
            .trim()
            .parse()
            .map_err(|e| format!("exit.txt is not an integer: {e}"))?,
        Err(_) => 0,
    };

    let mut child = Command::new(env!("CARGO_BIN_EXE_parley"))
        .args(&args)
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("could not start the parley binary: {e}"))?;

    let stdin_bytes = fs::read(case.join("stdin.txt")).unwrap_or_default();
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(&stdin_bytes)
            .map_err(|e| format!("could not write stdin.txt to the command: {e}"))?;
    }

    let output = child
        .wait_with_output()
        .map_err(|e| format!("the parley binary did not finish: {e}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    if output.status.code() != Some(expected_exit) {
        return Err(format!(
            "parley {} exited with {:?}, expected {expected_exit}\nstdout:\n{stdout}\nstderr:\n{stderr}",
            args.join(" "),
            output.status.code()
        )
        .into());
    }

    let actual: Value = serde_json::from_str(&stdout)
        .map_err(|e| format!("stdout is not one JSON object ({e}):\n{stdout}"))?;

    let expected_path = case.join("expected.json");
    if std::env::var("PARLEY_BLESS")
        .map(|m| m == module)
        .unwrap_or(false)
    {
        let mut text = serde_json::to_string_pretty(&actual).expect("JSON serialises");
        text.push('\n');
        fs::write(&expected_path, text).map_err(|e| format!("could not bless: {e}"))?;
        return Ok(());
    }

    let expected_text = fs::read_to_string(&expected_path)
        .map_err(|e| format!("expected.json could not be read: {e}"))?;
    let expected: Value = serde_json::from_str(&expected_text)
        .map_err(|e| format!("expected.json is not JSON: {e}"))?;
    let tolerances = read_tolerances(case)?;

    let mut diffs = Vec::new();
    compare("", &expected, &actual, &tolerances, &mut diffs);
    if diffs.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "{} difference(s) against expected.json:\n{}",
            diffs.len(),
            diffs.join("\n")
        )
        .into())
    }
}

fn read_tolerances(case: &Path) -> Result<HashMap<String, f64>, Failed> {
    let path = case.join("tolerance.json");
    if !path.exists() {
        return Ok(HashMap::new());
    }
    let text =
        fs::read_to_string(&path).map_err(|e| format!("tolerance.json could not be read: {e}"))?;
    let value: Value =
        serde_json::from_str(&text).map_err(|e| format!("tolerance.json is not JSON: {e}"))?;
    let object = value
        .as_object()
        .ok_or("tolerance.json must be an object of path to tolerance")?;
    let mut tolerances = HashMap::new();
    for (path, tolerance) in object {
        let tolerance = tolerance
            .as_f64()
            .ok_or_else(|| format!("tolerance.json: {path} is not a number"))?;
        tolerances.insert(path.clone(), tolerance);
    }
    Ok(tolerances)
}

fn join_path(path: &str, key: &str) -> String {
    if path.is_empty() {
        key.to_string()
    } else {
        format!("{path}.{key}")
    }
}

/// Structural comparison. Floats (a float on either side) compare within the path's tolerance;
/// everything else compares exactly. Every difference is recorded with its path.
fn compare(
    path: &str,
    expected: &Value,
    actual: &Value,
    tolerances: &HashMap<String, f64>,
    diffs: &mut Vec<String>,
) {
    match (expected, actual) {
        (Value::Object(e), Value::Object(a)) => {
            for (key, expected_value) in e {
                match a.get(key) {
                    Some(actual_value) => compare(
                        &join_path(path, key),
                        expected_value,
                        actual_value,
                        tolerances,
                        diffs,
                    ),
                    None => diffs.push(format!("{}: missing", join_path(path, key))),
                }
            }
            for key in a.keys() {
                if !e.contains_key(key) {
                    diffs.push(format!("{}: not expected", join_path(path, key)));
                }
            }
        }
        (Value::Array(e), Value::Array(a)) => {
            if e.len() != a.len() {
                diffs.push(format!(
                    "{}: expected {} element(s), got {}",
                    if path.is_empty() { "(root)" } else { path },
                    e.len(),
                    a.len()
                ));
            }
            for (i, (expected_value, actual_value)) in e.iter().zip(a).enumerate() {
                compare(
                    &join_path(path, &i.to_string()),
                    expected_value,
                    actual_value,
                    tolerances,
                    diffs,
                );
            }
        }
        (Value::Number(e), Value::Number(a)) if e.is_f64() || a.is_f64() => {
            let tolerance = tolerances.get(path).copied().unwrap_or(1e-9);
            let (ef, af) = (
                e.as_f64().unwrap_or(f64::NAN),
                a.as_f64().unwrap_or(f64::NAN),
            );
            let difference = (ef - af).abs();
            if difference.is_nan() || difference > tolerance {
                diffs.push(format!(
                    "{path}: expected {ef}, got {af} (tolerance {tolerance})"
                ));
            }
        }
        _ => {
            if expected != actual {
                diffs.push(format!(
                    "{}: expected {expected}, got {actual}",
                    if path.is_empty() { "(root)" } else { path }
                ));
            }
        }
    }
}
