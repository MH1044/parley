//! `parley content validate` and `parley content lock`.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use parley_core::content::{Context, Forms, Lang, Report, load_syllabus, lock_items, validate};

use super::{
    join, load_forms, load_siblings, missing, not_found, parse, parse_lang, print_json, shown,
    unknown, usage,
};

pub fn run(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    match args.first().map(String::as_str) {
        Some("validate") => validate_command(&args[1..], out, err),
        Some("lock") => lock_command(&args[1..], out, err),
        Some(other) => unknown(err, other),
        None => missing(err, "content validate or content lock"),
    }
}

/// One syllabus to validate: its path as shown, its path on disk, and its folder name when
/// that is a language code.
struct Target {
    file: String,
    path: PathBuf,
    folder: Option<String>,
}

fn language_folder(path: &Path) -> Option<String> {
    let name = path.parent()?.file_name()?.to_string_lossy().into_owned();
    Lang::ALL
        .iter()
        .any(|lang| lang.as_str() == name)
        .then_some(name)
}

/// Validates one file: V01 through `load_syllabus`, then the other rules with the files beside
/// the syllabus (or the given forms table).
fn validate_file(target: &Target, forms: Option<&Forms>) -> Report {
    let mut report = Report::default();
    let syllabus = match load_syllabus(&target.path) {
        Ok(syllabus) => syllabus,
        Err(errors) => {
            report.errors = errors.into_iter().map(|e| e.0).collect();
            return report;
        }
    };
    let siblings = match load_siblings(&target.path, forms.cloned()) {
        Ok(siblings) => siblings,
        Err(e) => {
            report.errors.push(e.0);
            return report;
        }
    };
    let ctx = Context {
        file: &target.file,
        folder: target.folder.as_deref(),
        forms: &siblings.forms,
        numbers: &siblings.numbers,
        exclude: &siblings.exclude,
        lock: siblings.lock.as_ref(),
    };
    report.extend(validate(&syllabus, &ctx));
    report
}

fn validate_command(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    let parsed = match parse(args, &["--forms"], &["--json"]) {
        Ok(parsed) => parsed,
        Err(message) => return usage(err, &message),
    };
    if let Some(extra) = parsed.positional.get(1) {
        return unknown(err, extra);
    }
    let Some(given) = parsed.positional.first() else {
        return missing(err, "<file-or-dir>");
    };
    let path = Path::new(given);
    if !path.exists() {
        return not_found(err, &shown(given));
    }
    let mut targets: Vec<Target> = Vec::new();
    if path.is_dir() {
        for lang in Lang::ALL {
            let syllabus = path.join(lang.as_str()).join("syllabus.yaml");
            if syllabus.is_file() {
                targets.push(Target {
                    file: join(given, &format!("{}/syllabus.yaml", lang.as_str())),
                    path: syllabus,
                    folder: Some(lang.as_str().to_string()),
                });
            }
        }
        if targets.is_empty() {
            return not_found(err, &join(given, "es/syllabus.yaml"));
        }
    } else {
        targets.push(Target {
            file: shown(given),
            path: path.to_path_buf(),
            folder: language_folder(path),
        });
    }

    let mut report = Report::default();
    let forms = match parsed.value("--forms") {
        Some(table) => {
            if !Path::new(table).is_file() {
                return not_found(err, &shown(table));
            }
            match load_forms(Path::new(table)) {
                Ok(forms) => Some(forms),
                Err(e) => {
                    report.errors.push(e.0);
                    None
                }
            }
        }
        None => None,
    };
    if report.ok() {
        for target in &targets {
            report.extend(validate_file(target, forms.as_ref()));
        }
    }

    if parsed.has("--json") {
        print_json(out, &report);
    } else {
        for line in report.errors.iter().chain(report.warnings.iter()) {
            let _ = writeln!(out, "{line}");
        }
        if report.ok() {
            let _ = writeln!(out, "ok");
        }
    }
    if report.ok() { 0 } else { 1 }
}

fn lock_command(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    let parsed = match parse(args, &["--lang", "--content"], &[]) {
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
    let content = parsed.value("--content").unwrap_or("content");
    let dir = Path::new(content).join(lang.as_str());
    let syllabus_path = dir.join("syllabus.yaml");
    let file = join(content, &format!("{}/syllabus.yaml", lang.as_str()));
    if !syllabus_path.is_file() {
        return not_found(err, &file);
    }

    let mut report = Report::default();
    let syllabus = match load_syllabus(&syllabus_path) {
        Ok(syllabus) => syllabus,
        Err(errors) => {
            report.errors = errors.into_iter().map(|e| e.0).collect();
            print_json(out, &report);
            return 1;
        }
    };
    let siblings = match load_siblings(&syllabus_path, None) {
        Ok(siblings) => siblings,
        Err(e) => {
            report.errors.push(e.0);
            print_json(out, &report);
            return 1;
        }
    };
    // V20 and V21 are what the lock is about to fix, so they are left out of this check.
    let ctx = Context {
        file: &file,
        folder: Some(lang.as_str()),
        forms: &siblings.forms,
        numbers: &siblings.numbers,
        exclude: &siblings.exclude,
        lock: None,
    };
    report.extend(validate(&syllabus, &ctx));
    if !report.ok() {
        print_json(out, &report);
        return 1;
    }
    let lock = siblings.lock.unwrap_or_default();
    let (next, changes) = lock_items(&syllabus, &siblings.forms, &lock);
    let lock_path = dir.join("ids.lock");
    if let Err(e) = fs::write(&lock_path, next.to_text()) {
        return super::failure(
            err,
            &format!(
                "{}: {e}",
                join(content, &format!("{}/ids.lock", lang.as_str()))
            ),
        );
    }
    print_json(out, &changes);
    0
}
