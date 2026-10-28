//! `parley pack build` and `parley pack info`.

use std::io::Write;
use std::path::{Path, PathBuf};

use parley_core::content::{PackError, PackInput, Report, build_pack, pack_info};

use super::{
    failure, is_date, missing, not_found, parse, parse_lang, print_json, today, unknown, usage,
};

pub fn run(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    match args.first().map(String::as_str) {
        Some("build") => build(&args[1..], out, err),
        Some("info") => info(&args[1..], out, err),
        Some(other) => unknown(err, other),
        None => missing(err, "pack build or pack info"),
    }
}

/// Maps a pack error to the exit code and output of the command: a missing input is a usage
/// error, content errors print the validator's report, anything else is one line on stderr.
fn report_error(error: PackError, out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    match error {
        PackError::NotFound(path) => not_found(err, &path),
        PackError::Content(errors) => {
            let report = Report {
                errors: errors.into_iter().map(|e| e.0).collect(),
                warnings: Vec::new(),
            };
            print_json(out, &report);
            1
        }
        other => failure(err, &other.to_string()),
    }
}

fn build(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    let parsed = match parse(
        args,
        &[
            "--lang",
            "--tatoeba-dir",
            "--wiktextract",
            "--syllabus",
            "--exclude",
            "--built-at",
            "--out",
        ],
        &[],
    ) {
        Ok(parsed) => parsed,
        Err(message) => return usage(err, &message),
    };
    if let Some(extra) = parsed.positional.first() {
        return unknown(err, extra);
    }
    let Some(lang) = parsed.value("--lang") else {
        return missing(err, "--lang");
    };
    let lang = match parse_lang(err, lang) {
        Ok(lang) => lang,
        Err(code) => return code,
    };
    let Some(tatoeba_dir) = parsed.value("--tatoeba-dir") else {
        return missing(err, "--tatoeba-dir");
    };
    let Some(wiktextract) = parsed.value("--wiktextract") else {
        return missing(err, "--wiktextract");
    };
    let Some(syllabus) = parsed.value("--syllabus") else {
        return missing(err, "--syllabus");
    };
    let Some(out_path) = parsed.value("--out") else {
        return missing(err, "--out");
    };
    let built_at = parsed
        .value("--built-at")
        .map_or_else(today, str::to_string);
    if !is_date(&built_at) {
        return usage(err, "--built-at must be YYYY-MM-DD. Run parley --help.");
    }
    let input = PackInput {
        lang,
        tatoeba_dir: PathBuf::from(tatoeba_dir),
        wiktextract: PathBuf::from(wiktextract),
        syllabus: PathBuf::from(syllabus),
        exclude: parsed.value("--exclude").map(PathBuf::from),
        built_at,
    };
    match build_pack(&input, Path::new(out_path)) {
        Ok(report) => {
            print_json(out, &report);
            0
        }
        Err(error) => report_error(error, out, err),
    }
}

fn info(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    let parsed = match parse(args, &[], &[]) {
        Ok(parsed) => parsed,
        Err(message) => return usage(err, &message),
    };
    if let Some(extra) = parsed.positional.get(1) {
        return unknown(err, extra);
    }
    let Some(file) = parsed.positional.first() else {
        return missing(err, "<file.sqlite3>");
    };
    match pack_info(Path::new(file)) {
        Ok(info) => {
            print_json(out, &info);
            0
        }
        Err(error) => report_error(error, out, err),
    }
}
