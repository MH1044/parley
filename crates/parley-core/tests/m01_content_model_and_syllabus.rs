//! Module 01 tests of the library (docs/modules/01-content-model-and-syllabus.md, Acceptance
//! tests). Each test names the bullet it covers. Bullets 1, 2, 8 and 13 also run as golden cases
//! under tests/cases/01-content-model-and-syllabus; the binary (exit codes, stderr, files written)
//! is covered by crates/parley-cli/tests/m01_content_model_and_syllabus.rs; bullet 17 is the
//! harness itself. Expected values come from the spec and from the fixture READMEs
//! (tests/fixtures/syllabus, tests/fixtures/tatoeba-mini, tests/fixtures/forms-mini.README.md),
//! never from running the code.

use std::fs;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use parley_core::content::{
    Context, Forms, Gender, IdsLock, Lang, PackError, PackInput, PackReport, Syllabus, TokenKind,
    build_pack, derive_items, load_exclude, load_numbers, lock_items, pack_info, parse_packs_lock,
    parse_syllabus, read_wiktextract, tokenise, validate, verify_pack, write_forms_tsv,
};
use serde_json::{Value, json};

const VALID_ES: &str = "tests/fixtures/syllabus/valid-es.yaml";
const SKILL: &str = "es.a1.u01.s01";
const FORMS_MINI_SHA256: &str = "bdd2092585e56451edb8e59e844bfca7a0659c3cc5cf51c3716430dea3b33919";

/// Example 2 of valid-es.yaml, as the one line it occupies in the file.
const EXAMPLE_2: &str = "          - {id: 2, target: \"Buenos días.\", en: [\"Good morning.\"]}\n";
/// A fifth example that passes every rule beside the four of valid-es.yaml.
const EXAMPLE_5: &str = "          - {id: 5, target: \"Hola, Luis.\", en: [\"Hello, Luis.\"]}\n";

/// The repository root: the first ancestor of this crate that holds the workspace Cargo.toml.
fn repo_root() -> PathBuf {
    let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    loop {
        if let Ok(text) = fs::read_to_string(dir.join("Cargo.toml"))
            && text.contains("[workspace]")
        {
            return dir;
        }
        assert!(dir.pop(), "no workspace Cargo.toml above the crate");
    }
}

fn fixture(path: &str) -> PathBuf {
    repo_root().join(path)
}

fn read(path: &str) -> String {
    fs::read_to_string(fixture(path)).unwrap_or_else(|e| panic!("{path} could not be read: {e}"))
}

fn parse(file: &str, text: &str) -> Syllabus {
    parse_syllabus(file, text).unwrap_or_else(|errors| {
        let messages: Vec<&str> = errors.iter().map(|e| e.0.as_str()).collect();
        panic!("{file} did not parse:\n{}", messages.join("\n"))
    })
}

fn valid_es() -> Syllabus {
    parse(VALID_ES, &read(VALID_ES))
}

/// The hand-written table beside the syllabus fixtures (five rows, see its README).
fn fixture_forms() -> Forms {
    let file = fs::File::open(fixture("tests/fixtures/syllabus/forms.tsv")).unwrap();
    Forms::load_tsv(file).unwrap_or_else(|e| panic!("forms.tsv: {e}"))
}

/// forms-mini's entries for one language, read as `forms extract` and `pack build` read them.
fn mini_forms(lang: Lang) -> Forms {
    let file = fs::File::open(fixture("tests/fixtures/forms-mini.jsonl")).unwrap();
    let extract = read_wiktextract(BufReader::new(file), lang, None)
        .unwrap_or_else(|e| panic!("forms-mini.jsonl: {e}"));
    Forms::from_rows(extract.rows)
}

fn numbers() -> Vec<String> {
    load_numbers(&read("tests/fixtures/syllabus/numbers.yaml"))
        .unwrap_or_else(|e| panic!("numbers.yaml: {e}"))
}

fn exclude() -> Vec<String> {
    load_exclude(&read("tests/fixtures/syllabus/exclude.txt"))
}

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .expect("an array of strings")
        .iter()
        .map(|v| v.as_str().expect("a string").to_string())
        .collect()
}

fn without_line(text: &str, line: &str) -> String {
    assert!(text.contains(line), "the fixture does not contain {line:?}");
    text.replacen(line, "", 1)
}

/// The item ids of valid-es.yaml in byte order, as `content lock` lists them.
fn valid_es_ids() -> Vec<String> {
    [
        "#e:1",
        "#e:2",
        "#e:3",
        "#e:4",
        "#l:adiós",
        "#l:bueno",
        "#l:día",
        "#l:gracias",
        "#l:hola",
        "#l:noche",
        "#l:tal",
    ]
    .iter()
    .map(|suffix| format!("{SKILL}{suffix}"))
    .collect()
}

fn lock_text(lines: &[(&str, &str)]) -> String {
    lines
        .iter()
        .map(|(id, state)| format!("{id}\t{state}\t1\n"))
        .collect()
}

/// The context of a syllabus at `es/syllabus.yaml` inside a content directory.
fn context<'a>(
    lock: Option<&'a IdsLock>,
    forms: &'a Forms,
    numbers: &'a [String],
    exclude: &'a [String],
) -> Context<'a> {
    Context {
        file: "es/syllabus.yaml",
        folder: Some("es"),
        forms,
        numbers,
        exclude,
        lock,
    }
}

fn build_mini_pack(out: &Path, built_at: &str) -> PackReport {
    let input = PackInput {
        lang: Lang::Es,
        tatoeba_dir: fixture("tests/fixtures/tatoeba-mini"),
        wiktextract: fixture("tests/fixtures/forms-mini.jsonl"),
        syllabus: fixture(VALID_ES),
        exclude: None,
        built_at: built_at.to_string(),
    };
    build_pack(&input, out).unwrap_or_else(|e| panic!("pack build failed: {e}"))
}

fn expected_pack_report() -> Value {
    json!({
        "lang": "es",
        "kept": 22,
        "dropped": {
            "no_translation": 5, "too_short": 1, "too_long": 3,
            "excluded_lemma": 2, "unknown_word": 5, "duplicate": 2
        }
    })
}

// Bullet 1: the example syllabus parses to its four examples and validates with no error and no
// warning against the forms table, number words and exclude list beside it.
#[test]
fn t01_valid_es_parses_and_validates_clean() {
    let s = valid_es();
    let skill = &s.units[0].skills[0];
    assert_eq!(skill.lemmas.len(), 7);
    let ids: Vec<i64> = skill.examples.iter().map(|e| e.id).collect();
    assert_eq!(ids, vec![1, 2, 3, 4]);
    assert_eq!(skill.examples[1].target, "Buenos días.");
    assert_eq!(skill.examples[1].en, vec!["Good morning.".to_string()]);
    assert!(matches!(skill.lemmas[3].gender, Some(Gender::M)));
    assert!(matches!(skill.lemmas[4].gender, Some(Gender::F)));

    let (forms, numbers, exclude) = (fixture_forms(), numbers(), exclude());
    let report = validate(
        &s,
        &Context {
            file: VALID_ES,
            folder: None,
            forms: &forms,
            numbers: &numbers,
            exclude: &exclude,
            lock: None,
        },
    );
    assert!(report.ok(), "errors: {:?}", report.errors);
    assert!(report.errors.is_empty());
    assert!(report.warnings.is_empty());
}

// Bullet 2: every bad-V fixture gives exactly the message its golden case expects (the golden
// expected.json files are the single statement of those messages). V01 comes from the parser;
// V02 needs the folder name; V20 and V21 need the ids.lock beside the syllabus.
#[test]
fn t02_each_bad_fixture_gives_exactly_its_message() {
    let (forms, numbers, exclude) = (fixture_forms(), numbers(), exclude());
    let cases = fixture("tests/cases/01-content-model-and-syllabus");
    for n in 1..=21 {
        let rule = format!("V{n:02}");
        let expected: Value = serde_json::from_str(
            &fs::read_to_string(
                cases
                    .join(format!("validate-bad-{rule}"))
                    .join("expected.json"),
            )
            .unwrap(),
        )
        .unwrap();
        let expected_errors = strings(&expected["errors"]);
        let expected_warnings = strings(&expected["warnings"]);

        let directory = matches!(rule.as_str(), "V02" | "V20" | "V21");
        let file = if directory {
            format!("tests/fixtures/syllabus/bad-{rule}/es/syllabus.yaml")
        } else {
            format!("tests/fixtures/syllabus/bad-{rule}.yaml")
        };
        let text = read(&file);
        let s = match parse_syllabus(&file, &text) {
            Ok(s) => s,
            Err(errors) => {
                let messages: Vec<String> = errors.into_iter().map(|e| e.0).collect();
                assert_eq!(messages, expected_errors, "{rule}");
                assert!(expected_warnings.is_empty(), "{rule}");
                continue;
            }
        };
        assert_ne!(rule, "V01", "bad-V01.yaml must fail to parse");

        let lock_path = fixture(&format!("tests/fixtures/syllabus/bad-{rule}/es/ids.lock"));
        let lock = lock_path.is_file().then(|| {
            IdsLock::parse(&fs::read_to_string(&lock_path).unwrap())
                .unwrap_or_else(|e| panic!("{rule} ids.lock: {e}"))
        });
        let empty = Forms::default();
        let ctx = if directory {
            // No forms table, numbers or exclude list lie beside the directory fixtures; the
            // V20 and V21 cases pass the shared table with --forms.
            Context {
                file: &file,
                folder: Some("es"),
                forms: if rule == "V02" { &empty } else { &forms },
                numbers: &[],
                exclude: &[],
                lock: lock.as_ref(),
            }
        } else {
            Context {
                file: &file,
                folder: None,
                forms: &forms,
                numbers: &numbers,
                exclude: &exclude,
                lock: None,
            }
        };
        let report = validate(&s, &ctx);
        assert_eq!(report.errors, expected_errors, "{rule} errors");
        assert_eq!(report.warnings, expected_warnings, "{rule} warnings");
        assert_eq!(report.ok(), expected["ok"].as_bool().unwrap(), "{rule} ok");
    }
}

// Bullet 3: French elisions split after the apostrophe, aujourd'hui stays whole, a capitalised
// word the table does not know is a Name, and Est-ce que splits on the hyphen.
#[test]
fn t03_tokenise_french() {
    let forms = mini_forms(Lang::Fr);
    let tokens = tokenise(Lang::Fr, "L'homme est aujourd'hui à Paris.", &forms);
    let surfaces: Vec<&str> = tokens.iter().map(|t| t.surface.as_str()).collect();
    assert_eq!(
        surfaces,
        vec!["L'", "homme", "est", "aujourd'hui", "à", "Paris"]
    );
    assert!(matches!(tokens[5].kind, TokenKind::Name), "Paris is a Name");
    for token in &tokens[..5] {
        assert!(
            matches!(token.kind, TokenKind::Word),
            "{} is a Word",
            token.surface
        );
    }
    let mut elision = tokens[0].lemma_keys.clone();
    elision.sort();
    assert_eq!(elision, vec!["la", "le"], "l' gives le and la");
    assert_eq!(tokens[0].norm, "l'");
    assert_eq!(tokens[1].lemma_keys, vec!["homme"]);
    assert_eq!(tokens[2].lemma_keys, vec!["être"]);
    assert_eq!(tokens[5].norm, "paris");

    let tokens = tokenise(Lang::Fr, "Est-ce que tu viens ?", &forms);
    let surfaces: Vec<&str> = tokens.iter().map(|t| t.surface.as_str()).collect();
    assert_eq!(surfaces, vec!["Est", "ce", "que", "tu", "viens"]);
    assert_eq!(tokens[4].lemma_keys, vec!["venir"]);
}

// Bullet 4: al, del and zum stay one token with two lemma keys, every other word's keys come
// from the forms table, Straße stays one token, and digits are a Number.
#[test]
fn t04_tokenise_lemma_keys_numbers_and_sharp_s() {
    let es = mini_forms(Lang::Es);
    let tokens = tokenise(Lang::Es, "Voy al mercado del pueblo.", &es);
    let keys: Vec<Vec<String>> = tokens
        .iter()
        .map(|t| {
            let mut keys = t.lemma_keys.clone();
            keys.sort();
            keys
        })
        .collect();
    assert_eq!(
        keys,
        vec![
            vec!["ir".to_string()],
            vec!["a".to_string(), "el".to_string()],
            vec!["mercado".to_string()],
            vec!["de".to_string(), "el".to_string()],
            vec!["pueblo".to_string()],
        ]
    );
    assert!(
        matches!(tokens[0].kind, TokenKind::Word),
        "Voy at sentence start is a Word"
    );

    let de = mini_forms(Lang::De);
    let tokens = tokenise(Lang::De, "Ich gehe zum Bahnhof.", &de);
    let surfaces: Vec<&str> = tokens.iter().map(|t| t.surface.as_str()).collect();
    assert_eq!(surfaces, vec!["Ich", "gehe", "zum", "Bahnhof"]);
    let mut zum = tokens[2].lemma_keys.clone();
    zum.sort();
    assert_eq!(zum, vec!["dem", "zu"]);
    assert_eq!(tokens[1].lemma_keys, vec!["gehen"]);
    assert_eq!(tokens[3].lemma_keys, vec!["Bahnhof"]);

    let tokens = tokenise(Lang::De, "Die Straße ist lang.", &de);
    let surfaces: Vec<&str> = tokens.iter().map(|t| t.surface.as_str()).collect();
    assert_eq!(surfaces, vec!["Die", "Straße", "ist", "lang"]);
    assert_eq!(tokens[1].norm, "straße");
    assert_eq!(tokens[1].lemma_keys, vec!["Straße"]);

    let tokens = tokenise(Lang::Es, "Tengo 3 gatos.", &es);
    let surfaces: Vec<&str> = tokens.iter().map(|t| t.surface.as_str()).collect();
    assert_eq!(surfaces, vec!["Tengo", "3", "gatos"]);
    assert!(matches!(tokens[1].kind, TokenKind::Number));
    assert_eq!(tokens[0].lemma_keys, vec!["tener"]);
    assert_eq!(tokens[2].lemma_keys, vec!["gato"]);
}

// Bullet 5: forms lookups on forms-mini and on the hand-written table, and the loader's one
// error message.
#[test]
fn t05_forms_lookups() {
    let es = mini_forms(Lang::Es);
    assert_eq!(es.lemmas_of("buenas"), vec!["bueno"]);
    assert_eq!(es.lemmas_of("fue"), vec!["ir", "ser"]);
    assert!(matches!(es.gender_of("mesa"), Some(Gender::F)));
    assert!(matches!(es.gender_of("día"), Some(Gender::M)));
    assert!(
        es.gender_of("bueno").is_none(),
        "an adjective has no gender"
    );
    assert_eq!(
        es.rows().len(),
        84,
        "every Spanish headword and inflected form"
    );

    let de = mini_forms(Lang::De);
    assert!(matches!(de.gender_of("Tisch"), Some(Gender::M)));
    assert!(matches!(de.gender_of("Straße"), Some(Gender::F)));
    assert_eq!(de.lemmas_of("stehe"), vec!["stehen"]);
    assert_eq!(de.rows().len(), 14);

    let table = fixture_forms();
    assert_eq!(table.rows().len(), 5);
    assert_eq!(table.lemmas_of("días"), vec!["día"]);
    assert_eq!(table.lemmas_of("noches"), vec!["noche"]);
    assert!(matches!(table.gender_of("noche"), Some(Gender::F)));
    assert!(table.lemmas_of("tardes").is_empty());
    assert!(Forms::default().lemmas_of("buenas").is_empty());

    let bad = "# source: hand-written; sha256: -; date: 2026-10-27\nbuenas\tbueno\tadj\n";
    let err = Forms::load_tsv(bad.as_bytes()).expect_err("four columns are refused");
    assert_eq!(err.0, "forms.tsv: line 2: expected 5 tab-separated columns");
}

const DE_SYLLABUS: &str = r#"schema: parley.syllabus/1
lang: de
name: German for English speakers
variety: Standard German; Sie and du both taught
version: 1
function_words: [der, die, das, ein, eine, und, oder, aber, ich, du, er, sie, es, wir, ihr, um,
  in, an, zu, von, mit, nicht, ist, ja, nein, bitte, danke]
names: [Anna, Max, Berlin]
units:
  - id: de.a1.u01
    title: Morning
    cefr: A1
    skills:
      - id: de.a1.u01.s01
        kind: grammar
        title: Separable verbs
        cefr: A1
        can_do: I can say when I get up.
        requires: []
        lemmas:
          - {lemma: aufstehen, pos: verb, en: [to get up], rank: null, separable: auf}
          - {lemma: gehen, pos: verb, en: [to go], rank: null}
          - {lemma: Bahnhof, pos: noun, gender: m, en: [station], rank: null}
          - {lemma: Tisch, pos: noun, gender: m, en: [table], rank: null}
        examples:
          - {id: 1, target: "Ich stehe um sieben auf.", en: ["I get up at seven."]}
          - {id: 2, target: "Ich gehe mit Anna.", en: ["I am going with Anna."]}
          - {id: 3, target: "Der Bahnhof ist in Berlin.", en: ["The station is in Berlin."]}
          - {id: 4, target: "Der Tisch ist nicht in Berlin.", en: ["The table is not in Berlin."]}
"#;

// Bullet 6: a lemma marked `separable: auf` allows its base verb (stehe, a form of stehen in
// forms-mini) and the particle; without that lemma the base verb is untaught. `sieben` is a
// number word, `um` and `ich` are function words.
#[test]
fn t06_separable_verb_allows_its_base_and_particle() {
    let forms = mini_forms(Lang::De);
    let numbers: Vec<String> = ["eins", "zwei", "drei", "vier", "fünf", "sechs", "sieben"]
        .iter()
        .map(|w| w.to_string())
        .collect();
    let ctx = Context {
        file: "de/syllabus.yaml",
        folder: Some("de"),
        forms: &forms,
        numbers: &numbers,
        exclude: &[],
        lock: None,
    };

    let s = parse("de/syllabus.yaml", DE_SYLLABUS);
    let report = validate(&s, &ctx);
    assert!(report.ok(), "errors: {:?}", report.errors);
    assert!(report.warnings.is_empty());

    let without = without_line(
        DE_SYLLABUS,
        "          - {lemma: aufstehen, pos: verb, en: [to get up], rank: null, separable: auf}\n",
    );
    let s = parse("de/syllabus.yaml", &without);
    let report = validate(&s, &ctx);
    assert!(!report.ok());
    assert_eq!(
        report.errors[0],
        "de.a1.u01.s01: example 1 uses \"stehe\", which no earlier skill teaches"
    );
    assert!(
        !report
            .errors
            .iter()
            .any(|e| e.contains("\"sieben\"") || e.contains("\"um\"")),
        "number and function words are never reported: {:?}",
        report.errors
    );
}

// Bullet 7: derive_items on the example syllabus gives exactly expected-items.json: seven
// vocabulary items in lemma order (nouns with their article), then four phrase items.
#[test]
fn t07_derive_items_matches_expected_items() {
    let items = derive_items(&valid_es(), &fixture_forms());
    let actual = serde_json::to_value(&items).unwrap();
    let expected: Value =
        serde_json::from_str(&read("tests/fixtures/syllabus/expected-items.json")).unwrap();
    assert_eq!(actual, expected);

    let ids: Vec<&str> = actual
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["id"].as_str().unwrap())
        .collect();
    assert_eq!(
        ids,
        vec![
            "es.a1.u01.s01#l:hola",
            "es.a1.u01.s01#l:adiós",
            "es.a1.u01.s01#l:bueno",
            "es.a1.u01.s01#l:día",
            "es.a1.u01.s01#l:noche",
            "es.a1.u01.s01#l:tal",
            "es.a1.u01.s01#l:gracias",
            "es.a1.u01.s01#e:1",
            "es.a1.u01.s01#e:2",
            "es.a1.u01.s01#e:3",
            "es.a1.u01.s01#e:4",
        ]
    );
    assert_eq!(actual[3]["target"], "el día");
    assert_eq!(actual[4]["target"], "la noche");
    assert_eq!(actual[7]["kind"], "phrase");
    assert_eq!(
        actual[7]["accepted"]["to_target"],
        json!(["Hola, ¿qué tal?"])
    );
}

// Bullet 8: the pack builder keeps 22 of the 40 tatoeba-mini sentences for the reasons the
// fixture README assigns, and sentence 1001 is kept with English 2001 by fixture_a.
#[test]
fn t08_pack_build_report_and_rows() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("es.sqlite3");
    let report = build_mini_pack(&out, "2026-10-01");
    assert_eq!(
        serde_json::to_value(&report).unwrap(),
        expected_pack_report()
    );

    let conn = rusqlite::Connection::open(&out).unwrap();
    let count = |sql: &str| conn.query_row(sql, [], |r| r.get::<_, i64>(0)).unwrap();
    assert_eq!(count("SELECT count(*) FROM sentences"), 22);
    assert_eq!(count("SELECT count(*) FROM translations"), 22);
    assert_eq!(count("SELECT count(*) FROM forms"), 84);
    assert_eq!(count("SELECT count(*) FROM freq"), 24);
    assert_eq!(count("SELECT min(id) FROM sentences"), 1001);
    assert_eq!(count("SELECT max(id) FROM sentences"), 1022);

    let (text, author, tokens, max_rank) = conn
        .query_row(
            "SELECT text, author, tokens, max_rank FROM sentences WHERE id = 1001",
            [],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, i64>(3)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(text, "Hola, soy Ana.");
    assert_eq!(author, "fixture_a");
    assert_eq!(tokens, 3);
    assert_eq!(max_rank, 24, "hola is the 24th ranked lemma");

    let (en_id, en_text, en_author) = conn
        .query_row(
            "SELECT en_id, en_text, en_author FROM translations WHERE sentence_id = 1001",
            [],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(
        (en_id, en_text.as_str(), en_author.as_str()),
        (2001, "Hello, I am Ana.", "fixture_b")
    );

    // Tatoeba's \N marker for a sentence without an owner is stored as empty text.
    let author: String = conn
        .query_row("SELECT author FROM sentences WHERE id = 1010", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(author, "");
    let en_author: String = conn
        .query_row(
            "SELECT en_author FROM translations WHERE sentence_id = 1010",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(en_author, "");

    let mut stmt = conn
        .prepare("SELECT lemma FROM sentence_lemmas WHERE sentence_id = 1002 ORDER BY lemma")
        .unwrap();
    let lemmas: Vec<String> = stmt
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    assert_eq!(lemmas, vec!["a", "de", "el", "ir", "mercado", "pueblo"]);

    let (rank, count) = conn
        .query_row(
            "SELECT rank, count FROM freq WHERE lemma = 'ser'",
            [],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, f64>(1)?)),
        )
        .unwrap();
    assert_eq!(rank, 1);
    assert!(
        (count - 7.0).abs() < 1e-9,
        "ser is in 7 kept sentences, got {count}"
    );
    let rank: i64 = conn
        .query_row("SELECT rank FROM freq WHERE lemma = 'tener'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(rank, 2);
}

// Bullet 9: two builds from the same inputs on different days share content_sha256; pack_info
// carries every key of the Interfaces format with the fixture's counts.
#[test]
fn t09_pack_info_is_stable_across_builds() {
    let dir = tempfile::tempdir().unwrap();
    let first = dir.path().join("es-2026-10-01.sqlite3");
    let second = dir.path().join("es-2026-10-02.sqlite3");
    build_mini_pack(&first, "2026-10-01");
    build_mini_pack(&second, "2026-10-02");
    let info_a = pack_info(&first).unwrap_or_else(|e| panic!("pack info: {e}"));
    let info_b = pack_info(&second).unwrap_or_else(|e| panic!("pack info: {e}"));
    let a = serde_json::to_value(&info_a).unwrap();
    let b = serde_json::to_value(&info_b).unwrap();
    assert_eq!(a["content_sha256"], b["content_sha256"]);
    assert_eq!(a["built_at"], "2026-10-01");
    assert_eq!(b["built_at"], "2026-10-02");

    for key in [
        "schema",
        "lang",
        "built_at",
        "tool_version",
        "licence",
        "sentences",
        "translations",
        "forms",
        "ranked_lemmas",
        "bytes",
        "sha256",
        "content_sha256",
        "source_sha256",
    ] {
        assert!(a.get(key).is_some(), "pack info lacks {key}: {a}");
    }
    assert_eq!(a["schema"], "parley.pack/1");
    assert_eq!(a["lang"], "es");
    assert_eq!(a["tool_version"], "0.1.0");
    assert_eq!(a["licence"], "CC BY 2.0 FR");
    assert_eq!(a["sentences"], 22);
    assert_eq!(a["translations"], 22);
    assert_eq!(a["forms"], 84);
    assert_eq!(a["ranked_lemmas"], 24);
    assert_eq!(
        a["bytes"].as_u64().unwrap(),
        fs::metadata(&first).unwrap().len()
    );
    let is_hex = |v: &Value| {
        let s = v.as_str().unwrap_or_default();
        s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit())
    };
    assert!(is_hex(&a["sha256"]));
    assert!(is_hex(&a["content_sha256"]));

    let sources = a["source_sha256"]
        .as_object()
        .expect("source_sha256 is an object");
    for name in [
        "spa_sentences_detailed.tsv",
        "eng_sentences_detailed.tsv",
        "spa-eng_links.tsv",
        "forms-mini.jsonl",
        "valid-es.yaml",
    ] {
        assert!(
            sources.contains_key(name),
            "source_sha256 lacks {name}: {sources:?}"
        );
        assert!(is_hex(&sources[name]), "{name}");
    }
    assert_eq!(sources["forms-mini.jsonl"], FORMS_MINI_SHA256);
    for key in sources.keys() {
        assert!(
            key == "exclude.txt"
                || key.ends_with(".tsv")
                || key == "forms-mini.jsonl"
                || key == "valid-es.yaml",
            "unexpected source {key}"
        );
    }
    assert_ne!(
        sources["spa_sentences_detailed.tsv"],
        sources["eng_sentences_detailed.tsv"]
    );
    assert_eq!(a["source_sha256"], b["source_sha256"]);
}

// Bullet 10: verify_pack accepts the file it was given the hash of and refuses a copy with one
// changed byte with the learner-facing message.
#[test]
fn t10_verify_pack_detects_one_changed_byte() {
    let dir = tempfile::tempdir().unwrap();
    let original = dir.path().join("es.sqlite3");
    build_mini_pack(&original, "2026-10-01");
    let info =
        serde_json::to_value(pack_info(&original).unwrap_or_else(|e| panic!("{e}"))).unwrap();
    let sha256 = info["sha256"].as_str().unwrap();
    assert!(verify_pack(&original, sha256).is_ok());

    let copy = dir.path().join("damaged.sqlite3");
    let mut bytes = fs::read(&original).unwrap();
    let middle = bytes.len() / 2;
    bytes[middle] ^= 0x01;
    fs::write(&copy, bytes).unwrap();
    let err = verify_pack(&copy, sha256).expect_err("a changed byte is detected");
    assert!(matches!(err, PackError::ChecksumMismatch));
    assert_eq!(err.to_string(), "The download was damaged. Try again.");
}

// Bullet 12: packs.lock.toml with a table missing a key names the key; the schema-only file of
// this module parses.
#[test]
fn t12_packs_lock_missing_sha256() {
    let text = "schema = \"parley.packs/1\"\n\n[es]\nversion = \"2026-10-01\"\n\
                url = \"https://github.com/MH1044/parley/releases/download/v0.1.0/es-2026-10-01.sqlite3\"\n\
                bytes = 123456\n\
                content_sha256 = \"0000000000000000000000000000000000000000000000000000000000000000\"\n";
    let err = parse_packs_lock(text).expect_err("a missing sha256 is an error");
    assert_eq!(err.0, "packs.lock.toml: [es] needs sha256");
    assert_eq!(err.to_string(), "packs.lock.toml: [es] needs sha256");

    assert!(parse_packs_lock("schema = \"parley.packs/1\"\n").is_ok());
    let committed = read("content/packs.lock.toml");
    assert!(
        parse_packs_lock(&committed).is_ok(),
        "content/packs.lock.toml parses"
    );
}

// Bullet 13 (the library side of forms extract): the three requested lemmas give the 15 rows of
// expected-forms.tsv, byte for byte once written with the fixture's SHA-256; an unknown lemma is
// reported missing in the order requested.
#[test]
fn t13_wiktextract_rows_match_expected_forms_tsv() {
    let open =
        || BufReader::new(fs::File::open(fixture("tests/fixtures/forms-mini.jsonl")).unwrap());
    let wanted: Vec<String> = ["bueno", "noche", "ir"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let extract =
        read_wiktextract(open(), Lang::Es, Some(&wanted[..])).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(extract.found, 3);
    assert!(extract.missing.is_empty());
    assert_eq!(extract.rows.len(), 15);
    let mut written = Vec::new();
    write_forms_tsv(
        &mut written,
        "forms-mini.jsonl",
        FORMS_MINI_SHA256,
        "2026-10-01",
        &extract.rows,
    )
    .unwrap();
    let expected = fs::read(fixture("tests/fixtures/syllabus/expected-forms.tsv")).unwrap();
    assert_eq!(
        String::from_utf8(written).unwrap(),
        String::from_utf8(expected).unwrap()
    );

    let wanted: Vec<String> = ["bueno", "zzz"].iter().map(|s| s.to_string()).collect();
    let extract =
        read_wiktextract(open(), Lang::Es, Some(&wanted[..])).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(extract.found, 1);
    assert_eq!(extract.missing, vec!["zzz"]);
    assert_eq!(extract.rows.len(), 4);

    let whole = read_wiktextract(open(), Lang::Es, None).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(whole.rows.len(), 84);
    let loaded = Forms::load_tsv(
        fs::File::open(fixture("tests/fixtures/syllabus/expected-forms.tsv")).unwrap(),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(loaded.rows().len(), 15);
    assert_eq!(loaded.lemmas_of("fue"), vec!["ir"]);
}

// Bullet 15 (the library side): deleting example 2 leaves #e:1, #e:3 and #e:4 untouched; the
// lock records every id, retires the deleted one, refuses its reuse (V20) and asks for a lock
// run for a new id (V21).
#[test]
fn t15_stable_ids_and_lock() {
    let (forms, numbers, exclude) = (fixture_forms(), numbers(), exclude());
    let full = read(VALID_ES);

    let all_items = serde_json::to_value(derive_items(&valid_es(), &forms)).unwrap();
    let reduced = parse(VALID_ES, &without_line(&full, EXAMPLE_2));
    let items = serde_json::to_value(derive_items(&reduced, &forms)).unwrap();
    let example_items: Vec<&Value> = items
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| item["kind"] == "phrase")
        .collect();
    let ids: Vec<&str> = example_items
        .iter()
        .map(|i| i["id"].as_str().unwrap())
        .collect();
    assert_eq!(
        ids,
        vec![
            "es.a1.u01.s01#e:1",
            "es.a1.u01.s01#e:3",
            "es.a1.u01.s01#e:4"
        ]
    );
    for item in &example_items {
        let same = all_items
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["id"] == item["id"])
            .expect("the item existed before the deletion");
        assert_eq!(*item, same, "an untouched example keeps its item");
    }

    // The lock flow needs four examples at every step (V10), so a fifth example is added first.
    let five = format!("{full}{EXAMPLE_5}");
    let s5 = parse(VALID_ES, &five);
    let (lock0, report) = lock_items(&s5, &forms, &IdsLock::default());
    let mut expected_ids = valid_es_ids();
    expected_ids.insert(4, format!("{SKILL}#e:5"));
    assert_eq!(report.added, expected_ids);
    assert!(report.retired.is_empty());
    let active: Vec<(&str, &str)> = expected_ids
        .iter()
        .map(|id| (id.as_str(), "active"))
        .collect();
    assert_eq!(lock0.to_text(), lock_text(&active));
    assert!(lock0.is_active("es.a1.u01.s01#e:2"));
    assert!(!lock0.is_retired("es.a1.u01.s01#e:2"));
    assert!(!lock0.is_active("es.a1.u01.s01#e:6"));
    let reparsed = IdsLock::parse(&lock0.to_text()).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(reparsed.to_text(), lock0.to_text());

    let report = validate(&s5, &context(Some(&lock0), &forms, &numbers, &exclude));
    assert!(report.ok(), "{:?}", report.errors);

    let s4 = parse(VALID_ES, &without_line(&five, EXAMPLE_2));
    let report = validate(&s4, &context(Some(&lock0), &forms, &numbers, &exclude));
    assert_eq!(
        report.errors,
        vec![format!(
            "{SKILL}#e:2 is in ids.lock but not in the syllabus; run parley content lock --lang es to retire it"
        )]
    );
    let (lock1, report) = lock_items(&s4, &forms, &lock0);
    assert!(report.added.is_empty());
    assert_eq!(report.retired, vec![format!("{SKILL}#e:2")]);
    assert!(lock1.is_retired("es.a1.u01.s01#e:2"));
    assert!(!lock1.is_active("es.a1.u01.s01#e:2"));
    let mut lines = active.clone();
    lines[1] = ("es.a1.u01.s01#e:2", "retired");
    assert_eq!(lock1.to_text(), lock_text(&lines));
    let report = validate(&s4, &context(Some(&lock1), &forms, &numbers, &exclude));
    assert!(report.ok(), "{:?}", report.errors);

    let reused = format!(
        "{}          - {{id: 2, target: \"Buenas noches, Luis.\", en: [\"Good night, Luis.\"]}}\n",
        without_line(&five, EXAMPLE_2)
    );
    let s = parse(VALID_ES, &reused);
    let report = validate(&s, &context(Some(&lock1), &forms, &numbers, &exclude));
    assert_eq!(
        report.errors,
        vec![format!(
            "{SKILL}#e:2 was retired; give the example a new id"
        )]
    );

    let fresh = reused.replace(
        "{id: 2, target: \"Buenas noches, Luis.\"",
        "{id: 6, target: \"Buenas noches, Luis.\"",
    );
    let s6 = parse(VALID_ES, &fresh);
    let report = validate(&s6, &context(Some(&lock1), &forms, &numbers, &exclude));
    assert_eq!(
        report.errors,
        vec![format!(
            "{SKILL}#e:6 is not in ids.lock; run parley content lock --lang es"
        )]
    );
    let (lock2, report) = lock_items(&s6, &forms, &lock1);
    assert_eq!(report.added, vec![format!("{SKILL}#e:6")]);
    assert!(report.retired.is_empty());
    assert!(
        lock2.is_retired("es.a1.u01.s01#e:2"),
        "a retired line is never deleted"
    );
    let report = validate(&s6, &context(Some(&lock2), &forms, &numbers, &exclude));
    assert!(report.ok(), "{:?}", report.errors);
    let mut lines = lines.clone();
    lines.insert(5, ("es.a1.u01.s01#e:6", "active"));
    assert_eq!(lock2.to_text(), lock_text(&lines));
}

// Interfaces: numbers.yaml yields the number word set (every word of every entry) and refuses
// a list that is not 0 to 100; exclude.txt skips comments and blank lines.
#[test]
fn numbers_and_exclude_lists() {
    let words = numbers();
    for word in ["cero", "uno", "tres", "treinta", "y", "cien", "dieciséis"] {
        assert!(words.iter().any(|w| w == word), "{word} is a number word");
    }
    assert!(
        !words.iter().any(|w| w == "treinta y uno"),
        "entries are split into words"
    );

    let text = read("tests/fixtures/syllabus/numbers.yaml");
    let short = text.trim_end_matches("  - cien\n");
    assert_ne!(
        short.len(),
        text.len(),
        "the fixture ends with the entry for 100"
    );
    let err = load_numbers(short).expect_err("100 entries are refused");
    assert_eq!(
        err.0,
        "numbers.yaml: words has 100 entries; 0 to 100 need 101"
    );

    assert_eq!(exclude(), vec!["fumar", "matar", "odiar"]);
    assert_eq!(
        load_exclude("# comment\n\nfumar\n  \nmatar\n"),
        vec!["fumar", "matar"]
    );
}
