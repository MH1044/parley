//! `parley freq check --lang <l> --pack <file> [--content <dir>]`: every single-word lemma's
//! `rank` against the pack's frequency table (docs/modules/05-course-content-a1-a2.md). It
//! reports; the author edits the ranks.

use std::io::Write;
use std::path::Path;

use parley_core::content::{Report, freq_check, load_syllabus, read_pack_stats};

use super::content::Target;
use super::{failure, missing, not_found, parse, parse_lang, print_json, shown, unknown, usage};

pub fn run(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    match args.first().map(String::as_str) {
        Some("check") => check(&args[1..], out, err),
        Some(other) => unknown(err, other),
        None => missing(err, "freq check"),
    }
}

fn check(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    let parsed = match parse(args, &["--lang", "--pack", "--content"], &["--json"]) {
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
    let Some(pack) = parsed.value("--pack") else {
        return missing(err, "--pack");
    };
    let content = parsed.value("--content").unwrap_or("content");
    if !Path::new(pack).is_file() {
        return not_found(err, &shown(pack));
    }
    let target = Target::in_content(content, lang);
    if !target.path.is_file() {
        return not_found(err, &target.file);
    }

    // Only the syllabus has to load (V01): the ranks are what this command helps to fill in.
    let syllabus = match load_syllabus(&target.path) {
        Ok(syllabus) => syllabus,
        Err(errors) => {
            let report = Report {
                errors: errors.into_iter().map(|e| e.0).collect(),
                warnings: Vec::new(),
            };
            print_json(out, &report);
            return 1;
        }
    };
    let stats = match read_pack_stats(Path::new(pack)) {
        Ok(stats) => stats,
        Err(e) => return failure(err, &e.to_string()),
    };
    let result = freq_check(&syllabus, &stats.freq);
    print_json(out, &result);
    if result.ok() { 0 } else { 1 }
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
        let (code, out, err) = run_capture(&[]);
        assert_eq!((code, out.as_str()), (2, ""));
        assert_eq!(err, "error: freq check is required. Run parley --help.\n");
        let (code, _, err) = run_capture(&["check", "--lang", "es"]);
        assert_eq!(code, 2);
        assert_eq!(err, "error: --pack is required. Run parley --help.\n");
        let (code, _, err) =
            run_capture(&["check", "--lang", "es", "--pack", "no/such/es.sqlite3"]);
        assert_eq!(code, 2);
        assert_eq!(err, "error: no/such/es.sqlite3 not found\n");
    }

    #[test]
    fn ranks_are_compared_with_the_pack() {
        let dir = tempfile::tempdir().unwrap();
        let es = dir.path().join("es");
        std::fs::create_dir_all(&es).unwrap();
        std::fs::write(
            es.join("syllabus.yaml"),
            "schema: parley.syllabus/1\nlang: es\nname: n\nvariety: v\nversion: 1\nfunction_words: []\nnames: []\nunits:\n  - id: es.a1.u01\n    title: t\n    cefr: A1\n    skills:\n      - id: es.a1.u01.s01\n        kind: vocab\n        title: t\n        cefr: A1\n        can_do: I can.\n        requires: []\n        lemmas:\n          - {lemma: hola, pos: intj, en: [hi], rank: 1}\n          - {lemma: adiós, pos: intj, en: [bye], rank: 1}\n          - {lemma: por favor, pos: expr, en: [please], rank: null}\n          - {lemma: zzz, pos: intj, en: [z], rank: null}\n        examples: []\n",
        )
        .unwrap();
        let pack = dir.path().join("es.sqlite3");
        let conn = rusqlite::Connection::open(&pack).unwrap();
        conn.execute_batch(
            "CREATE TABLE sentences (id INTEGER PRIMARY KEY, text TEXT, norm TEXT, tokens INTEGER,
               max_rank INTEGER, author TEXT);
             CREATE TABLE freq (lemma TEXT PRIMARY KEY, rank INTEGER, count REAL);
             INSERT INTO freq VALUES ('hola', 1, 9.0);
             INSERT INTO freq VALUES ('adiós', 2, 4.0);",
        )
        .unwrap();
        drop(conn);
        let content = parley_core::content::display_path(dir.path());
        let pack = parley_core::content::display_path(&pack);
        let (code, out, _) = run_capture(&[
            "check",
            "--lang",
            "es",
            "--pack",
            &pack,
            "--content",
            &content,
        ]);
        assert_eq!(code, 1);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&out).unwrap(),
            serde_json::json!({"lang": "es", "lemmas": 3,
                "differs": [{"skill": "es.a1.u01.s01", "lemma": "adiós", "rank": 1, "pack_rank": 2}],
                "missing": [{"skill": "es.a1.u01.s01", "lemma": "zzz"}]})
        );
    }
}
