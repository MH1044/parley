//! The live quality gates (docs/TESTING.md §6) that need the real AI client, which only the app's
//! crates have: `parley-core` makes no HTTP request of its own. Each gate is an ignored test named
//! `live_*`, run by `scripts/gates/run --live <G>` with `PARLEY_LIVE=1`; without that variable it
//! returns at once, so it never runs in the normal suite. Live gates spend money and are the
//! maintainer's to run.
//!
//! - `live_g1` (docs/modules/07-lesson-generation.md, acceptance test 20): 30 fresh-lesson requests
//!   on publik API, the first 10 skills of each course (es, fr, de), each a first lesson composed
//!   for a new learner; at least `[g1] valid_share` of the planned exercises pass the validators
//!   and the 95th percentile of the request time is at most `[g1] p95_s` seconds. Every request and
//!   its answer is written to tests/fixtures/ai/P1/recorded/ for the replay in the normal suite.
//!   About $0.45 (30 × $0.015, docs/PUBLIK-INTEGRATION.md §8).

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parley_core::ai::{
    AiClient, AiError, Cache, LessonCtx, LessonPrompt, build_lesson, request_lesson,
};
use parley_core::content::{Forms, Lang, derive_items, load_numbers, parse_syllabus};
use parley_core::env::{Clock, SplitMix64};
use parley_core::exercise::{Capabilities, ComposeRequest, Origin, compose};
use parley_core::settings::AiSettings;
use parley_core::store::Store;
use parley_publik::PublikEnv;
use parley_publik::keychain::OsKeychain;
use parley_publik::service::{AiService, AiServiceConfig};
use serde_json::json;

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

fn live() -> bool {
    std::env::var("PARLEY_LIVE").is_ok_and(|v| v == "1")
}

struct ZeroClock;

impl Clock for ZeroClock {
    fn now_ms(&self) -> i64 {
        0
    }
    fn utc_offset_minutes(&self) -> i32 {
        0
    }
}

/// `[g1]` of scripts/gates/thresholds.toml: (valid_share, p95_s).
fn thresholds() -> (f64, f64) {
    let text = fs::read_to_string(repo_root().join("scripts/gates/thresholds.toml"))
        .expect("scripts/gates/thresholds.toml");
    let table: toml::Table = text.parse().expect("thresholds.toml parses");
    let g1 = table["g1"].as_table().expect("a [g1] table");
    (
        g1["valid_share"].as_float().expect("[g1] valid_share"),
        g1["p95_s"]
            .as_float()
            .or_else(|| g1["p95_s"].as_integer().map(|n| n as f64))
            .expect("[g1] p95_s"),
    )
}

/// The publik API client, as the desktop app builds it, in a scratch data folder.
fn publik_client(data_dir: PathBuf) -> Result<Arc<dyn AiClient>, AiError> {
    let mut service = AiService::new(AiServiceConfig {
        data_dir,
        publik: PublikEnv::from_process(),
        keychain: Box::new(OsKeychain),
        rng: Box::new(SplitMix64::new(1)),
        backoff: [Duration::from_secs(1), Duration::from_secs(4)],
        app_version: parley_core::api::VERSION.to_string(),
    });
    let ai = AiSettings {
        provider: "publik".into(),
        ..AiSettings::default()
    };
    service.client(&ai)
}

#[test]
#[ignore = "live gate: spends money; run with scripts/gates/run --live G1"]
fn live_g1() {
    if !live() {
        println!("live_g1: PARLEY_LIVE is not 1, so no request is made.");
        return;
    }
    let (valid_share, p95_s) = thresholds();
    let scratch = tempfile::tempdir().expect("a scratch folder");
    let client = publik_client(scratch.path().to_path_buf()).expect("publik API is set up");
    let recorded = repo_root().join("tests/fixtures/ai/P1/recorded");
    fs::create_dir_all(&recorded).expect("the recorded folder");

    let (mut valid, mut planned) = (0usize, 0usize);
    let mut times: Vec<Duration> = Vec::new();
    for lang in [Lang::Es, Lang::Fr, Lang::De] {
        let dir = repo_root().join("content").join(lang.as_str());
        let syllabus = parse_syllabus(
            "syllabus.yaml",
            &fs::read_to_string(dir.join("syllabus.yaml")).unwrap(),
        )
        .expect("the syllabus parses");
        let forms = Forms::load_tsv(&fs::read(dir.join("forms.tsv")).unwrap()[..]).unwrap();
        let numbers = fs::read_to_string(dir.join("numbers.yaml"))
            .map(|text| load_numbers(&text).expect("numbers.yaml loads"))
            .unwrap_or_default();
        let items = derive_items(&syllabus, &forms);
        let skills: Vec<String> = syllabus
            .skills()
            .take(10)
            .map(|(_, skill)| skill.id.clone())
            .collect();
        for skill_id in skills {
            let mut store = Store::open_in_memory().unwrap();
            store.start_course(&syllabus, &forms, 0).unwrap();
            let plan = compose(
                &ComposeRequest {
                    lang,
                    skill_id: skill_id.clone(),
                    lesson_length: 12,
                    new_items_per_lesson: 5,
                    capabilities: Capabilities::default(),
                },
                &store,
                0,
            )
            .unwrap();
            let prompt =
                LessonPrompt::new(&syllabus, &items, &[], &skill_id, 1, &plan).expect("a skill");
            let cache = Cache::in_memory(Box::new(ZeroClock));
            let started = Instant::now();
            let called = request_lesson(&*client, &cache, &prompt);
            times.push(started.elapsed());
            let ctx = LessonCtx {
                prompt: prompt.clone(),
                syllabus: &syllabus,
                forms: &forms,
                items: &items,
                numbers: &numbers,
                sentences: &[],
            };
            let response = match &called {
                Ok(called) => Some(&called.value),
                Err(AiError::Invalid) => None,
                Err(other) => panic!("{skill_id}: {}", other.message()),
            };
            let draft = build_lesson(
                response,
                client.kind(),
                &plan,
                &ctx,
                &mut SplitMix64::new(1),
            );
            valid += draft
                .lesson
                .exercises
                .iter()
                .filter(|ex| ex.origin == Origin::Ai)
                .count();
            planned += plan.entries.len();
            if let Ok(called) = &called {
                let record = json!({
                    "prompt": prompt,
                    "plan": plan,
                    "content": serde_json::to_string(&called.value).unwrap(),
                });
                fs::write(
                    recorded.join(format!("{}-{skill_id}.json", lang.as_str())),
                    serde_json::to_string_pretty(&record).unwrap() + "\n",
                )
                .unwrap();
            }
        }
    }
    times.sort();
    let p95 = times[(times.len() * 95).div_ceil(100) - 1].as_secs_f64();
    let share = valid as f64 / planned.max(1) as f64;
    println!("live_g1: {valid} of {planned} exercises valid ({share:.3}); p95 {p95:.1} s");
    assert!(
        share >= valid_share,
        "valid share {share:.3} < {valid_share}"
    );
    assert!(p95 <= p95_s, "p95 {p95:.1} s > {p95_s} s");
}
