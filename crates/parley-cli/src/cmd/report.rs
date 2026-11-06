//! `parley content report --lang <l> [--content <dir>] [--pack <file>] --json`: the counts of a
//! course and, with a pack, the shares of its examples the pack attests and of its A1 lemmas the
//! pack ranks within 2,000 (docs/modules/05-course-content-a1-a2.md).

use std::io::Write;
use std::path::Path;

use parley_core::content::{course_report, read_pack_stats};

use super::content::{Target, check_course};
use super::{failure, missing, not_found, parse, parse_lang, print_json, shown, unknown, usage};

pub fn run(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    // `--json` is accepted; the JSON object is the only output.
    let parsed = match parse(args, &["--lang", "--content", "--pack"], &["--json"]) {
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
    let target = Target::in_content(content, lang);
    if !target.path.is_file() {
        return not_found(err, &target.file);
    }
    let pack = parsed.value("--pack");
    if let Some(pack) = pack
        && !Path::new(pack).is_file()
    {
        return not_found(err, &shown(pack));
    }

    // The course must validate (its syllabus, checklist and review file); warnings are allowed.
    let check = check_course(&target, None);
    let Some(loaded) = check.loaded.as_ref().filter(|_| check.report.ok()) else {
        print_json(out, &check.report);
        return 1;
    };
    let stats = match pack.map(|pack| read_pack_stats(Path::new(pack))) {
        Some(Ok(stats)) => Some(stats),
        Some(Err(e)) => return failure(err, &e.to_string()),
        None => None,
    };
    let report = course_report(
        &loaded.syllabus,
        &loaded.siblings.forms,
        check.checklist.as_ref(),
        check.review.as_ref(),
        stats.as_ref(),
    );
    print_json(out, &report);
    0
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
    fn usage_errors_name_the_flag_or_the_file() {
        let (code, out, err) = run_capture(&["--json"]);
        assert_eq!((code, out.as_str()), (2, ""));
        assert_eq!(err, "error: --lang is required. Run parley --help.\n");
        let (code, _, err) = run_capture(&["--lang", "es", "--content", "no/such/dir", "--json"]);
        assert_eq!(code, 2);
        assert_eq!(err, "error: no/such/dir/es/syllabus.yaml not found\n");
        let (code, _, err) = run_capture(&["--lang", "it"]);
        assert_eq!(code, 2);
        assert_eq!(
            err,
            "error: --lang must be es, fr or de. Run parley --help.\n"
        );
    }

    #[test]
    fn a_course_that_does_not_validate_gives_the_validate_report() {
        let dir = tempfile::tempdir().unwrap();
        let es = dir.path().join("es");
        std::fs::create_dir_all(&es).unwrap();
        std::fs::write(es.join("syllabus.yaml"), "schema: parley.syllabus/1\n").unwrap();
        let content = parley_core::content::display_path(dir.path());
        let (code, out, _) = run_capture(&["--lang", "es", "--content", &content, "--json"]);
        assert_eq!(code, 1);
        let report: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(report["ok"], false);
        assert!(
            report["errors"][0]
                .as_str()
                .unwrap()
                .ends_with("es/syllabus.yaml: /: lang is required"),
            "{report}"
        );
        let missing_pack = format!("{content}/none.sqlite3");
        let (code, _, err) = run_capture(&[
            "--lang",
            "es",
            "--content",
            &content,
            "--pack",
            &missing_pack,
        ]);
        assert_eq!(code, 2);
        assert_eq!(err, format!("error: {missing_pack} not found\n"));
    }
}
