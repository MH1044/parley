//! `parley content validate`, `parley content lock` and `parley content scenarios`; `parley
//! content report` is in [`super::report`].

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use parley_core::content::{
    Checklist, Context, Forms, Lang, Report, ReviewFile, ScenarioCourse, Syllabus, check_checklist,
    check_ranks, check_review, display_path, load_checklist, load_review, load_syllabus,
    lock_items, validate, validate_scenario_file,
};

use super::{
    Siblings, join, load_forms, load_siblings, missing, not_found, parse, parse_lang, print_json,
    shown, unknown, usage,
};

pub fn run(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    match args.first().map(String::as_str) {
        Some("validate") => validate_command(&args[1..], out, err),
        Some("lock") => lock_command(&args[1..], out, err),
        Some("scenarios") => scenarios_command(&args[1..], out, err),
        Some("report") => super::report::run(&args[1..], out, err),
        Some(other) => unknown(err, other),
        None => missing(
            err,
            "content validate, content lock, content scenarios or content report",
        ),
    }
}

/// One syllabus to validate: its path as shown, its path on disk, and its folder name when
/// that is a language code.
pub(super) struct Target {
    pub file: String,
    pub path: PathBuf,
    pub folder: Option<String>,
}

impl Target {
    /// `<dir>/<lang>/syllabus.yaml` of a content directory as typed.
    pub fn in_content(dir: &str, lang: Lang) -> Target {
        Target {
            file: join(dir, &format!("{}/syllabus.yaml", lang.as_str())),
            path: Path::new(dir).join(lang.as_str()).join("syllabus.yaml"),
            folder: Some(lang.as_str().to_string()),
        }
    }

    /// A file beside the syllabus.
    fn sibling(&self, name: &str) -> PathBuf {
        self.path
            .parent()
            .map_or_else(|| PathBuf::from(name), |dir| dir.join(name))
    }
}

fn language_folder(path: &Path) -> Option<String> {
    let name = path.parent()?.file_name()?.to_string_lossy().into_owned();
    Lang::ALL
        .iter()
        .any(|lang| lang.as_str() == name)
        .then_some(name)
}

/// A syllabus that loaded, with the files beside it.
pub(super) struct Loaded {
    pub syllabus: Syllabus,
    pub siblings: Siblings,
}

/// The syllabus rules for one file: V01 through `load_syllabus`, then V02 to V21 with the files
/// beside the syllabus (or the given forms table), then V22 when `review.yaml` is beside it (a
/// course folder). The syllabus comes back when it loaded.
fn syllabus_rules(target: &Target, forms: Option<&Forms>) -> (Report, Option<Loaded>) {
    let mut report = Report::default();
    let syllabus = match load_syllabus(&target.path) {
        Ok(syllabus) => syllabus,
        Err(errors) => {
            report.errors = errors.into_iter().map(|e| e.0).collect();
            return (report, None);
        }
    };
    let siblings = match load_siblings(&target.path, forms.cloned()) {
        Ok(siblings) => siblings,
        Err(e) => {
            report.errors.push(e.0);
            return (report, None);
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
    if target.sibling("review.yaml").is_file() {
        report.extend(check_ranks(&syllabus));
    }
    (report, Some(Loaded { syllabus, siblings }))
}

/// Everything checked for one language of a content directory, and what was loaded.
pub(super) struct CourseCheck {
    pub report: Report,
    pub loaded: Option<Loaded>,
    pub checklist: Option<Checklist>,
    pub review: Option<ReviewFile>,
}

/// One language folder: the syllabus rules (V01 to V22), then CL01 to CL04 when
/// `checklist.yaml` is beside the syllabus, then RV01 to RV04 when `review.yaml` is. A syllabus
/// that does not load leaves only the rules that need no syllabus (CL01, CL02, RV01, RV02).
pub(super) fn check_course(target: &Target, forms: Option<&Forms>) -> CourseCheck {
    let (mut report, loaded) = syllabus_rules(target, forms);
    let syllabus = loaded.as_ref().map(|loaded| &loaded.syllabus);

    let mut checklist = None;
    let path = target.sibling("checklist.yaml");
    if path.is_file() {
        match load_checklist(&path) {
            Ok(parsed) => {
                report.extend(check_checklist(&display_path(&path), &parsed, syllabus));
                checklist = Some(parsed);
            }
            Err(errors) => report.errors.extend(errors.into_iter().map(|e| e.0)),
        }
    }

    let mut review = None;
    let path = target.sibling("review.yaml");
    if path.is_file() {
        match load_review(&path) {
            Ok(parsed) => {
                report.extend(check_review(&display_path(&path), &parsed, syllabus));
                review = Some(parsed);
            }
            Err(errors) => report.errors.extend(errors.into_iter().map(|e| e.0)),
        }
    }

    CourseCheck {
        report,
        loaded,
        checklist,
        review,
    }
}

/// The `*.yaml` files of a directory, in byte order of file name.
fn scenario_files(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut names: Vec<String> = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.ends_with(".yaml") && entry.path().is_file() {
            names.push(name);
        }
    }
    names.sort_unstable();
    Ok(names.into_iter().map(|name| dir.join(name)).collect())
}

/// SC01 to SC06 for each file in turn, against the courses that loaded.
fn check_scenarios(files: &[PathBuf], courses: &BTreeMap<Lang, Loaded>) -> Report {
    let courses: BTreeMap<Lang, ScenarioCourse<'_>> = courses
        .iter()
        .map(|(lang, loaded)| {
            (
                *lang,
                ScenarioCourse {
                    syllabus: &loaded.syllabus,
                    forms: &loaded.siblings.forms,
                    numbers: &loaded.siblings.numbers,
                },
            )
        })
        .collect();
    let mut report = Report::default();
    for file in files {
        report.extend(validate_scenario_file(file, &courses));
    }
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
    let directory = path.is_dir();
    let mut targets: Vec<(Lang, Target)> = Vec::new();
    if directory {
        for lang in Lang::ALL {
            let target = Target::in_content(given, lang);
            if target.path.is_file() {
                targets.push((lang, target));
            }
        }
        if targets.is_empty() {
            return not_found(err, &join(given, "es/syllabus.yaml"));
        }
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
        if directory {
            // Each language: the syllabus, its checklist and its review file; then the scenarios
            // against every syllabus that loaded.
            let mut loaded: BTreeMap<Lang, Loaded> = BTreeMap::new();
            for (lang, target) in &targets {
                let check = check_course(target, forms.as_ref());
                report.extend(check.report);
                if let Some(course) = check.loaded {
                    loaded.insert(*lang, course);
                }
            }
            let scenarios = path.join("scenarios");
            if scenarios.is_dir() {
                match scenario_files(&scenarios) {
                    Ok(files) => report.extend(check_scenarios(&files, &loaded)),
                    Err(e) => report
                        .errors
                        .push(format!("{}: {e}", join(given, "scenarios"))),
                }
            }
        } else {
            let target = Target {
                file: shown(given),
                path: path.to_path_buf(),
                folder: language_folder(path),
            };
            report.extend(syllabus_rules(&target, forms.as_ref()).0);
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
    // V20 and V21 are what the lock is about to fix, so they are left out of this check; ranks
    // (V22) are filled in after the ids are recorded, so they do not hold the lock up either.
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

/// `parley content scenarios <file-or-dir> [--content <dir>]`: SC01 to SC06 for one scenario
/// file, or every `*.yaml` of a directory in byte order of file name, against the syllabi of the
/// content directory (default `content`).
fn scenarios_command(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    let parsed = match parse(args, &["--content"], &["--json"]) {
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
    let content = parsed.value("--content").unwrap_or("content");
    if !Path::new(content).is_dir() {
        return not_found(err, &shown(content));
    }

    // A syllabus that is missing, or does not load with the files beside it, has no skills.
    let mut courses: BTreeMap<Lang, Loaded> = BTreeMap::new();
    for lang in Lang::ALL {
        let target = Target::in_content(content, lang);
        if !target.path.is_file() {
            continue;
        }
        if let Ok(syllabus) = load_syllabus(&target.path)
            && let Ok(siblings) = load_siblings(&target.path, None)
        {
            courses.insert(lang, Loaded { syllabus, siblings });
        }
    }

    let files = if path.is_dir() {
        match scenario_files(path) {
            Ok(files) => files,
            Err(e) => return super::failure(err, &format!("{}: {e}", shown(given))),
        }
    } else {
        vec![path.to_path_buf()]
    };
    let report = check_scenarios(&files, &courses);
    print_json(out, &report);
    if report.ok() { 0 } else { 1 }
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

    /// A one-skill Spanish course in `<dir>/content/es/`: the module 01 example syllabus with its
    /// forms, and, when asked, a checklist and a review file.
    fn course(dir: &Path) -> String {
        let es = dir.join("content").join("es");
        fs::create_dir_all(&es).unwrap();
        fs::write(es.join("syllabus.yaml"), SYLLABUS).unwrap();
        fs::write(es.join("forms.tsv"), FORMS).unwrap();
        display_path(&dir.join("content"))
    }

    const SYLLABUS: &str = "\
schema: parley.syllabus/1
lang: es
name: Spanish for English speakers
variety: Spain Spanish
version: 1
function_words: [el, la, de, y, que, qué]
names: [Ana]
units:
  - id: es.a1.u01
    title: Hello
    cefr: A1
    skills:
      - id: es.a1.u01.s01
        kind: function
        title: Greetings
        cefr: A1
        can_do: I can say hello and goodbye.
        requires: []
        lemmas:
          - {lemma: hola, pos: intj, en: [hello], rank: 4}
          - {lemma: bueno, pos: adj, en: [good], rank: 3500}
          - {lemma: día, pos: noun, gender: m, en: [day], rank: null}
          - {lemma: buenas noches, pos: expr, en: [good night], rank: null}
          - {lemma: tal, pos: adv, en: [such], rank: 9}
        examples:
          - {id: 1, target: \"Hola, ¿qué tal?\", en: [\"Hi, how are you?\"]}
          - {id: 2, target: \"Buenos días.\", en: [\"Good morning.\"]}
          - {id: 3, target: \"Buenas noches, Ana.\", en: [\"Good night, Ana.\"]}
          - {id: 4, target: \"Hola, Ana.\", en: [\"Hi, Ana.\"]}
";

    const FORMS: &str = "# source: hand-written; sha256: -; date: 2026-11-05
buenos\tbueno\tadj\t\tmasculine plural
días\tdía\tnoun\tm\tplural
";

    const REVIEW: &str = "schema: parley.review/1\nlang: es\nunits:\n  - {unit: es.a1.u01, status: unreviewed, date: \"2026-11-05\"}\n";

    const SCENARIO: &str = "\
schema: parley.scenario/1
id: cafe
cefr: A1
title_en: At a café
partner_role_en: the waiter
learner_role_en: a customer
turn_cap: 12
goals:
  - {id: greet, text_en: Greet the waiter}
  - {id: order_drink, text_en: Order a drink}
  - {id: pay_leave, text_en: Pay and say goodbye}
languages:
  es:
    setting_en: a small café in Madrid
    opening: \"¡Buenos días, Ana!\"
    opening_en: \"Good morning, Ana!\"
    skills: [es.a1.u01.s01]
";

    #[test]
    fn v22_runs_only_beside_a_review_file() {
        let dir = tempfile::tempdir().unwrap();
        let content = course(dir.path());
        let (code, out, _) = run_capture(&["validate", &content, "--json"]);
        assert_eq!(
            (code, out.as_str()),
            (0, "{\"ok\":true,\"errors\":[],\"warnings\":[]}\n")
        );

        fs::write(dir.path().join("content/es/review.yaml"), REVIEW).unwrap();
        let (code, out, _) = run_capture(&["validate", &content, "--json"]);
        assert_eq!(code, 1);
        let report: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(
            report,
            serde_json::json!({"ok": false,
                "errors": ["es.a1.u01.s01: día needs a rank"],
                "warnings": ["warning: bueno has rank 3500; check that it belongs at A1"]})
        );
        let file = format!("{content}/es/syllabus.yaml");
        let (code, out, _) = run_capture(&["validate", &file]);
        assert_eq!(code, 1);
        assert_eq!(
            out,
            "es.a1.u01.s01: día needs a rank\nwarning: bueno has rank 3500; check that it belongs at A1\n"
        );
    }

    #[test]
    fn a_directory_checks_the_checklist_review_and_scenarios_in_order() {
        let dir = tempfile::tempdir().unwrap();
        let content = course(dir.path());
        let es = dir.path().join("content/es");
        fs::write(
            es.join("checklist.yaml"),
            "schema: parley.checklist/1\nlang: es\nentries:\n  - {id: a1.f.greet, cefr: A2, kind: function, text_en: Greet people, skills: [es.a1.u01.s01, es.a1.u02.s01]}\n",
        )
        .unwrap();
        fs::write(
            es.join("review.yaml"),
            "schema: parley.review/1\nlang: es\nunits: []\n",
        )
        .unwrap();
        let syllabus = SYLLABUS
            .replace(
                "rank: null}\n          - {lemma: buenas",
                "rank: 7}\n          - {lemma: buenas",
            )
            .replace("rank: 3500", "rank: 8");
        fs::write(es.join("syllabus.yaml"), syllabus).unwrap();
        let scenarios = dir.path().join("content/scenarios");
        fs::create_dir_all(&scenarios).unwrap();
        fs::write(
            scenarios.join("shop.yaml"),
            SCENARIO.replace("turn_cap: 12", "turn_cap: 30"),
        )
        .unwrap();
        fs::write(
            scenarios.join("cafe.yaml"),
            SCENARIO.replace("Buenos días", "Buenas tardes"),
        )
        .unwrap();
        fs::write(scenarios.join("notes.txt"), "not a scenario").unwrap();
        let (code, out, _) = run_capture(&["validate", &content, "--json"]);
        assert_eq!(code, 1);
        let report: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(
            report["errors"],
            serde_json::json!([
                format!(
                    "{content}/es/checklist.yaml: a1.f.greet: skill es.a1.u02.s01 does not exist"
                ),
                format!(
                    "{content}/es/checklist.yaml: a1.f.greet: skill es.a1.u01.s01 is A1, but the entry is A2"
                ),
                format!("{content}/es/review.yaml: unit es.a1.u01 is missing"),
                "cafe: es: opening uses \"tardes\", which the listed skills do not teach",
                format!("{content}/scenarios/shop.yaml: id must match the file name"),
                "cafe: turn_cap 30 is outside 6 to 20",
            ])
        );
    }

    #[test]
    fn scenarios_are_checked_against_the_content_directory() {
        let dir = tempfile::tempdir().unwrap();
        let content = course(dir.path());
        let scenarios = dir.path().join("scenarios");
        fs::create_dir_all(&scenarios).unwrap();
        fs::write(scenarios.join("cafe.yaml"), SCENARIO).unwrap();
        let file = display_path(&scenarios.join("cafe.yaml"));
        let (code, out, err) = run_capture(&["scenarios", &file, "--content", &content]);
        assert_eq!(
            (code, out.as_str(), err.as_str()),
            (0, "{\"ok\":true,\"errors\":[],\"warnings\":[]}\n", "")
        );
        fs::write(
            scenarios.join("bad.yaml"),
            SCENARIO.replace("[es.a1.u01.s01]", "[es.a1.u03.s01]"),
        )
        .unwrap();
        let folder = display_path(&scenarios);
        let (code, out, _) = run_capture(&["scenarios", &folder, "--content", &content]);
        assert_eq!(code, 1);
        let report: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(
            report["errors"],
            serde_json::json!([
                format!("{folder}/bad.yaml: id must match the file name"),
                "cafe: es: skill es.a1.u03.s01 does not exist",
            ])
        );
        assert_eq!(report["warnings"], serde_json::json!([]));
    }

    #[test]
    fn scenarios_usage_errors() {
        let (code, out, err) = run_capture(&["scenarios", "no/such/cafe.yaml"]);
        assert_eq!((code, out.as_str()), (2, ""));
        assert_eq!(err, "error: no/such/cafe.yaml not found\n");
        let (code, _, err) = run_capture(&["scenarios"]);
        assert_eq!(code, 2);
        assert_eq!(
            err,
            "error: <file-or-dir> is required. Run parley --help.\n"
        );
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("cafe.yaml");
        fs::write(&file, SCENARIO).unwrap();
        let (code, _, err) = run_capture(&[
            "scenarios",
            &display_path(&file),
            "--content",
            "no/such/content",
        ]);
        assert_eq!(code, 2);
        assert_eq!(err, "error: no/such/content not found\n");
    }
}
