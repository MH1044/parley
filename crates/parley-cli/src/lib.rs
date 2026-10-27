//! Parley's command-line tool.
//!
//! `parley` is the way to use the core without the desktop app: later modules add
//! `content validate`, `pack build`, `grade`, `lesson template` and the others
//! (docs/ARCHITECTURE.md §2). Every command prints one JSON object on standard output with
//! `--json`, exits 0 on success, 1 on a content error and 2 on a usage error. The golden tests in
//! `tests/cases/` run this binary.

#![forbid(unsafe_code)]

pub mod fixtures;

use std::io::Write;

/// The program name as the help text and the version line print it.
pub const NAME: &str = "parley";

/// The version shared by every crate and the app.
pub const VERSION: &str = parley_core::api::VERSION;

const HELP: &str = "Usage: parley [--version [--json]] [--help]

Parley's command-line tool. Every command prints one JSON object with --json.

  --version, -V   print the version (with --json: {\"name\":\"parley\",\"version\":\"...\"})
  --help, -h      print this help

Exit codes: 0 success, 1 content error, 2 usage error.
";

/// Runs the tool over `args` (the arguments after the program name), writing to `out` and `err`.
/// Returns the exit code.
pub fn run(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    let mut want_version = false;
    let mut json = false;
    let mut want_help = false;

    for arg in args {
        match arg.as_str() {
            "--version" | "-V" => want_version = true,
            "--json" => json = true,
            "--help" | "-h" => want_help = true,
            other => {
                let _ = writeln!(err, "error: unknown argument {other}. Run parley --help.");
                return 2;
            }
        }
    }

    if want_help || args.is_empty() {
        let _ = out.write_all(HELP.as_bytes());
        return 0;
    }

    if want_version {
        if json {
            let line = serde_json::json!({ "name": NAME, "version": VERSION });
            let _ = writeln!(out, "{line}");
        } else {
            let _ = writeln!(out, "{NAME} {VERSION}");
        }
        return 0;
    }

    // Only --json was given.
    let _ = writeln!(err, "error: --json needs a command. Run parley --help.");
    2
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_capture(args: &[&str]) -> (u8, String, String) {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = run(&args, &mut out, &mut err);
        (
            code,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    }

    #[test]
    fn version_json_is_one_object() {
        let (code, out, err) = run_capture(&["--version", "--json"]);
        assert_eq!(code, 0);
        assert_eq!(
            out,
            format!("{{\"name\":\"parley\",\"version\":\"{VERSION}\"}}\n")
        );
        assert!(err.is_empty());
    }

    #[test]
    fn version_plain_is_name_and_number() {
        let (code, out, _) = run_capture(&["--version"]);
        assert_eq!(code, 0);
        assert_eq!(out, format!("parley {VERSION}\n"));
    }

    #[test]
    fn unknown_argument_is_a_usage_error() {
        let (code, out, err) = run_capture(&["--bogus"]);
        assert_eq!(code, 2);
        assert!(out.is_empty());
        assert_eq!(err, "error: unknown argument --bogus. Run parley --help.\n");
    }

    #[test]
    fn help_and_no_arguments_print_usage() {
        for args in [&["--help"][..], &[][..]] {
            let (code, out, _) = run_capture(args);
            assert_eq!(code, 0);
            assert!(out.starts_with("Usage: parley"));
        }
    }

    #[test]
    fn json_alone_needs_a_command() {
        let (code, _, err) = run_capture(&["--json"]);
        assert_eq!(code, 2);
        assert_eq!(err, "error: --json needs a command. Run parley --help.\n");
    }
}
