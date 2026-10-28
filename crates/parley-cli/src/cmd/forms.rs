//! `parley forms extract`: a forms table from a wiktextract extract.

use std::fs;
use std::io::{BufReader, Write};
use std::path::Path;

use parley_core::content::{Report, load_syllabus, read_wiktextract, write_forms_tsv};
use sha2::{Digest, Sha256};

use super::{
    failure, is_date, missing, not_found, parse, parse_lang, print_json, shown, today, unknown,
    usage,
};

pub fn run(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    match args.first().map(String::as_str) {
        Some("extract") => extract(&args[1..], out, err),
        Some(other) => unknown(err, other),
        None => missing(err, "forms extract"),
    }
}

fn extract(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    let parsed = match parse(
        args,
        &[
            "--lang",
            "--wiktextract",
            "--syllabus",
            "--lemma",
            "--date",
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
    let Some(wiktextract) = parsed.value("--wiktextract") else {
        return missing(err, "--wiktextract");
    };
    let syllabus = parsed.value("--syllabus");
    let requested = parsed.values_of("--lemma");
    if syllabus.is_none() && requested.is_empty() {
        return missing(err, "--syllabus or --lemma");
    }
    let Some(out_path) = parsed.value("--out") else {
        return missing(err, "--out");
    };
    let date = parsed.value("--date").map_or_else(today, str::to_string);
    if !is_date(&date) {
        return usage(err, "--date must be YYYY-MM-DD. Run parley --help.");
    }
    if !Path::new(wiktextract).is_file() {
        return not_found(err, &shown(wiktextract));
    }

    let lemmas: Vec<String> = match syllabus {
        Some(syllabus) => {
            if !Path::new(syllabus).is_file() {
                return not_found(err, &shown(syllabus));
            }
            match load_syllabus(Path::new(syllabus)) {
                Ok(syllabus) => {
                    let mut lemmas: Vec<String> = Vec::new();
                    for (_, skill) in syllabus.skills() {
                        for lemma in &skill.lemmas {
                            if !lemmas.contains(&lemma.lemma) {
                                lemmas.push(lemma.lemma.clone());
                            }
                        }
                    }
                    lemmas
                }
                Err(errors) => {
                    let report = Report {
                        errors: errors.into_iter().map(|e| e.0).collect(),
                        warnings: Vec::new(),
                    };
                    print_json(out, &report);
                    return 1;
                }
            }
        }
        None => requested.iter().map(|l| l.to_string()).collect(),
    };

    let bytes = match fs::read(wiktextract) {
        Ok(bytes) => bytes,
        Err(e) => return failure(err, &format!("{}: {e}", shown(wiktextract))),
    };
    let sha256: String = Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let source = Path::new(wiktextract)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let extract = match read_wiktextract(BufReader::new(&bytes[..]), lang, Some(&lemmas)) {
        Ok(extract) => extract,
        Err(e) => return failure(err, &e.0),
    };
    let mut table = Vec::new();
    if let Err(e) = write_forms_tsv(&mut table, &source, &sha256, &date, &extract.rows) {
        return failure(err, &format!("{}: {e}", shown(out_path)));
    }
    if let Err(e) = fs::write(out_path, table) {
        return failure(err, &format!("{}: {e}", shown(out_path)));
    }
    print_json(
        out,
        &ExtractReport {
            lemmas: extract.found,
            rows: extract.rows.len(),
            missing: extract.missing,
        },
    );
    0
}

/// What `forms extract` prints: the requested lemmas with an entry, the rows written, and the
/// requested lemmas without an entry in the order requested.
#[derive(serde::Serialize)]
struct ExtractReport {
    lemmas: usize,
    rows: usize,
    missing: Vec<String>,
}
