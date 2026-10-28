//! Parley's command-line tool.
//!
//! `parley` is the way to use the core without the desktop app: the content commands of module
//! 01 (`content validate`, `content lock`, `forms extract`, `pack build`, `pack info`) and, from
//! later modules, `grade`, `lesson template` and the others (docs/ARCHITECTURE.md §2). Every
//! command prints one JSON object on standard output, exits 0 on success, 1 on a content error
//! and 2 on a usage error. The golden tests in `tests/cases/` run this binary.

#![forbid(unsafe_code)]

pub mod cmd;
pub mod fixtures;

use std::io::Write;

/// The program name as the help text and the version line print it.
pub const NAME: &str = "parley";

/// The version shared by every crate and the app.
pub const VERSION: &str = parley_core::api::VERSION;

const HELP: &str = "Usage: parley <command> [options]
       parley --version [--json]
       parley --help

Parley's command-line tool. Every command prints one JSON object on standard output.

  content validate <file-or-dir> [--forms <forms.tsv>] [--json]
      Check a syllabus, or every <dir>/<lang>/syllabus.yaml (es, fr, de). The forms table is
      --forms, else forms.tsv beside the syllabus; numbers.yaml, exclude.txt and ids.lock are
      read from beside it when present. Without --json: one line per error and warning, then
      ok. Exit 1 on any error; warnings are allowed.
  content lock --lang <es|fr|de> [--content <dir>]
      Record every item id of <dir>/<lang>/syllabus.yaml in <dir>/<lang>/ids.lock (default
      dir: content), retiring the ids that left; prints {\"added\":[...],\"retired\":[...]}.
  forms extract --lang <l> --wiktextract <file.jsonl> (--syllabus <file> | --lemma <lemma>...)
                [--date <YYYY-MM-DD>] --out <forms.tsv>
      Write a forms table for the syllabus's lemmas (or the given ones) from a wiktextract
      extract; prints {\"lemmas\":n,\"rows\":n,\"missing\":[...]}. --date defaults to today.
  pack build --lang <l> --tatoeba-dir <dir> --wiktextract <file.jsonl> --syllabus <file>
             [--exclude <file>] [--built-at <YYYY-MM-DD>] --out <file.sqlite3>
      Build a sentence pack from decompressed Tatoeba exports; prints the kept count and a
      count per drop reason. --built-at defaults to today.
  pack info <file.sqlite3>
      Describe a pack: its meta rows, row counts, size and hashes.
  --version, -V   print the version (with --json: {\"name\":\"parley\",\"version\":\"...\"})
  --help, -h      print this help

Exit codes: 0 success, 1 content error, 2 usage error.
";

/// Runs the tool over `args` (the arguments after the program name), writing to `out` and `err`.
/// Returns the exit code.
pub fn run(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    match args.first().map(String::as_str) {
        Some("content") => return cmd::content::run(&args[1..], out, err),
        Some("forms") => return cmd::forms::run(&args[1..], out, err),
        Some("pack") => return cmd::pack::run(&args[1..], out, err),
        _ => {}
    }

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
            assert!(out.contains("content validate"));
            assert!(out.contains("pack info"));
        }
    }

    #[test]
    fn json_alone_needs_a_command() {
        let (code, _, err) = run_capture(&["--json"]);
        assert_eq!(code, 2);
        assert_eq!(err, "error: --json needs a command. Run parley --help.\n");
    }

    #[test]
    fn required_flags_and_missing_files_are_usage_errors() {
        let (code, out, err) = run_capture(&[
            "pack",
            "build",
            "--lang",
            "es",
            "--tatoeba-dir",
            "x",
            "--wiktextract",
            "y",
            "--syllabus",
            "z",
        ]);
        assert_eq!(code, 2);
        assert!(out.is_empty());
        assert_eq!(err, "error: --out is required. Run parley --help.\n");

        let (code, out, err) = run_capture(&[
            "pack",
            "build",
            "--lang",
            "es",
            "--tatoeba-dir",
            "no\\such\\dir",
            "--wiktextract",
            "y",
            "--syllabus",
            "z",
            "--out",
            "o",
        ]);
        assert_eq!(code, 2);
        assert!(out.is_empty());
        assert_eq!(
            err,
            "error: no/such/dir/spa_sentences_detailed.tsv not found\n"
        );

        let (code, _, err) = run_capture(&["content", "validate", "no/such/file.yaml"]);
        assert_eq!(code, 2);
        assert_eq!(err, "error: no/such/file.yaml not found\n");

        let (code, _, err) = run_capture(&["content", "validate"]);
        assert_eq!(code, 2);
        assert_eq!(
            err,
            "error: <file-or-dir> is required. Run parley --help.\n"
        );

        let (code, _, err) = run_capture(&["content", "lock", "--lang", "en"]);
        assert_eq!(code, 2);
        assert_eq!(
            err,
            "error: --lang must be es, fr or de. Run parley --help.\n"
        );

        let (code, _, err) = run_capture(&["pack", "info"]);
        assert_eq!(code, 2);
        assert_eq!(
            err,
            "error: <file.sqlite3> is required. Run parley --help.\n"
        );

        let (code, _, err) =
            run_capture(&["forms", "extract", "--lang", "es", "--wiktextract", "w"]);
        assert_eq!(code, 2);
        assert_eq!(
            err,
            "error: --syllabus or --lemma is required. Run parley --help.\n"
        );

        let (code, _, err) = run_capture(&["content", "frobnicate"]);
        assert_eq!(code, 2);
        assert_eq!(
            err,
            "error: unknown argument frobnicate. Run parley --help.\n"
        );
    }

    #[test]
    fn a_command_group_without_its_subcommand_names_the_choices() {
        for (args, choices) in [
            (&["content"][..], "content validate or content lock"),
            (&["forms"][..], "forms extract"),
            (&["pack"][..], "pack build or pack info"),
        ] {
            let (code, out, err) = run_capture(args);
            assert_eq!(code, 2, "{args:?}");
            assert!(out.is_empty());
            assert_eq!(
                err,
                format!("error: {choices} is required. Run parley --help.\n")
            );
        }
    }

    #[test]
    fn date_flags_must_be_iso_dates() {
        let (code, out, err) = run_capture(&[
            "forms",
            "extract",
            "--lang",
            "es",
            "--wiktextract",
            "x.jsonl",
            "--lemma",
            "bueno",
            "--date",
            "yesterday",
            "--out",
            "x.tsv",
        ]);
        assert_eq!(code, 2);
        assert!(out.is_empty());
        assert_eq!(
            err,
            "error: --date must be YYYY-MM-DD. Run parley --help.\n"
        );

        let (code, out, err) = run_capture(&[
            "pack",
            "build",
            "--lang",
            "es",
            "--tatoeba-dir",
            "t",
            "--wiktextract",
            "x.jsonl",
            "--syllabus",
            "s.yaml",
            "--built-at",
            "01-10-2026",
            "--out",
            "x.sqlite3",
        ]);
        assert_eq!(code, 2);
        assert!(out.is_empty());
        assert_eq!(
            err,
            "error: --built-at must be YYYY-MM-DD. Run parley --help.\n"
        );
    }
}
