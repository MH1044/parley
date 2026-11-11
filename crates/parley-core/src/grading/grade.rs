//! The offline grader (docs/modules/03-exercise-engine-and-grading.md, Design notes; ADR-0005:
//! exact, accent, typo, cached judgement, and only then the AI judge, for free translations).
//!
//! [`grade`] is a pure function of the exercise, the answer and a [`GradeCtx`]; [`grade_ctx`]
//! gathers the context from the store (stored answers, a cached judgement, the noun of a
//! vocabulary item), and [`accept_learner_answer`] records "I was right" so the same answer is
//! correct next time, offline.

use std::collections::BTreeSet;
use std::fmt;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::normalise::{accent_equal, display_words, normalise, words};
use super::typo::typo_match;
use crate::content::{Forms, Gender, ItemKind, Lang, nfc};
use crate::error::AppError;
use crate::exercise::{Body, Exercise, ExerciseType};
use crate::srs::{self, Outcome, Rating, rating_for};
use crate::store::{AnswerSource, ItemRow, Store, StoreError};

/// How an answer that differs from the expected one only in accents is graded (the
/// `accent_leniency` setting).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AccentLeniency {
    /// Almost, rated Hard.
    #[default]
    Lenient,
    /// Wrong, rated Again.
    Strict,
    /// Correct with a note, rated Good.
    Ignore,
}

impl AccentLeniency {
    pub fn as_str(self) -> &'static str {
        match self {
            AccentLeniency::Lenient => "lenient",
            AccentLeniency::Strict => "strict",
            AccentLeniency::Ignore => "ignore",
        }
    }
}

/// What the grader decided about an answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    Correct,
    /// An accent or article slip, or a judgement of almost.
    Almost,
    /// A slip of one letter in a few long words.
    Typo,
    Wrong,
    /// A free translation the offline grader cannot decide: the AI judge (module 08) or the
    /// learner's "I was right" decides it.
    Unchecked,
}

impl Verdict {
    pub fn as_str(self) -> &'static str {
        match self {
            Verdict::Correct => "correct",
            Verdict::Almost => "almost",
            Verdict::Typo => "typo",
            Verdict::Wrong => "wrong",
            Verdict::Unchecked => "unchecked",
        }
    }

    /// The scheduler's verdict: a typo counts as almost; an unchecked answer reviews nothing.
    pub fn srs(self) -> Option<srs::Verdict> {
        match self {
            Verdict::Correct => Some(srs::Verdict::Correct),
            Verdict::Almost | Verdict::Typo => Some(srs::Verdict::Almost),
            Verdict::Wrong => Some(srs::Verdict::Wrong),
            Verdict::Unchecked => None,
        }
    }
}

impl From<srs::Verdict> for Verdict {
    fn from(verdict: srs::Verdict) -> Self {
        match verdict {
            srs::Verdict::Correct => Verdict::Correct,
            srs::Verdict::Almost => Verdict::Almost,
            srs::Verdict::Wrong => Verdict::Wrong,
        }
    }
}

impl fmt::Display for Verdict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What kind of slip a grade names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tag {
    Accent,
    Article,
    Typo,
}

impl Tag {
    pub fn as_str(self) -> &'static str {
        match self {
            Tag::Accent => "accent",
            Tag::Article => "article",
            Tag::Typo => "typo",
        }
    }

    /// The tag named `text`, when it is one of the three.
    pub(crate) fn parse(text: &str) -> Option<Tag> {
        match text {
            "accent" => Some(Tag::Accent),
            "article" => Some(Tag::Article),
            "typo" => Some(Tag::Typo),
            _ => None,
        }
    }
}

/// Which way an answer goes: into the target language or into English. Serialised as the
/// `accepted_answers.direction` values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    ToTarget,
    ToEn,
}

impl Direction {
    pub fn as_str(self) -> &'static str {
        match self {
            Direction::ToTarget => "to_target",
            Direction::ToEn => "to_en",
        }
    }
}

impl fmt::Display for Direction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A noun with its definite article, as a vocabulary item's target shows it (`el día`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NounInfo {
    /// The article as written in the target (`el`, `l'`).
    pub article: String,
    /// The noun as written (`día`, `Mädchen`).
    pub noun: String,
    pub gender: Gender,
}

/// The definite articles of each language, as the article rule compares them.
fn definite_articles(lang: Lang) -> &'static [&'static str] {
    match lang {
        Lang::Es => &["el", "la", "los", "las"],
        Lang::Fr => &["le", "la", "les", "l'"],
        Lang::De => &["der", "die", "das", "den", "dem", "des"],
    }
}

/// The gender a singular definite article shows, for a noun the forms table has no gender for.
fn article_gender(lang: Lang, article: &str) -> Option<Gender> {
    match (lang, article) {
        (Lang::Es, "el") | (Lang::Fr, "le") | (Lang::De, "der") => Some(Gender::M),
        (Lang::Es, "la") | (Lang::Fr, "la") | (Lang::De, "die") => Some(Gender::F),
        (Lang::De, "das") => Some(Gender::N),
        _ => None,
    }
}

/// The noun of `target` when it is a definite article of the language, a space and a noun
/// (French also `l'` and the noun with no space). The gender is the forms table's
/// `gender_of(noun)`, else the article's (es el m, la f; fr le m, la f; de der m, die f, das n);
/// none without a gender.
pub fn noun_info(lang: Lang, target: &str, forms: &Forms) -> Option<NounInfo> {
    let text = nfc(target);
    let text = text.trim();
    let elided = if lang == Lang::Fr {
        let mut chars = text.char_indices();
        match (chars.next(), chars.next()) {
            (Some((_, l)), Some((at, '\'' | '\u{2019}'))) if l == 'l' || l == 'L' => {
                let split = at + text[at..].chars().next().map_or(1, char::len_utf8);
                Some((&text[..split], &text[split..]))
            }
            _ => None,
        }
    } else {
        None
    };
    let (article, noun) = match elided {
        Some((article, noun)) if !noun.is_empty() && !noun.starts_with(char::is_whitespace) => {
            (article, noun)
        }
        _ => {
            let (article, noun) = text.split_once(' ')?;
            let lower = article.to_lowercase();
            if !definite_articles(lang).contains(&lower.as_str()) || lower == "l'" {
                return None;
            }
            (article, noun.trim())
        }
    };
    if noun.is_empty() {
        return None;
    }
    let key = article.to_lowercase().replace('\u{2019}', "'");
    let gender = forms
        .gender_of(noun)
        .or_else(|| article_gender(lang, &key))?;
    Some(NounInfo {
        article: article.to_string(),
        noun: noun.to_string(),
        gender,
    })
}

/// A cached verdict on an answer: from the AI judge (module 08) or the learner's "I was right".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredJudgement {
    pub verdict: srs::Verdict,
    pub corrected: String,
    pub explanation: String,
    pub tags: Vec<String>,
    /// Who judged: a provider name, or `learner`.
    pub provider: String,
}

/// What the grader knows besides the exercise and the answer.
#[derive(Debug, Clone)]
pub struct GradeCtx<'a> {
    pub lang: Lang,
    pub leniency: AccentLeniency,
    pub forms: &'a Forms,
    /// The noun of a vocabulary item, which turns on the article rule.
    pub noun: Option<NounInfo>,
    /// More accepted answers (displays), tried after the exercise's own.
    pub stored: Vec<String>,
    /// A cached judgement of this very answer.
    pub judgement: Option<StoredJudgement>,
}

/// A learner's answer: typed text, the index of a chosen option, or matched pairs of indexes
/// (`[left, right]`, mapped back by the interface to pair indexes).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Answer {
    Text(String),
    Choice(u32),
    Pairs(Vec<[u32; 2]>),
}

/// The grader's result, as the result sheet shows it. The three fields of module 08 are left out
/// of the JSON when empty, so every other grade serialises as it did before.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Grade {
    pub verdict: Verdict,
    pub tags: Vec<Tag>,
    pub message: Option<String>,
    /// The answer the verdict was measured against, as written; none for an exact match.
    pub corrected: Option<String>,
    /// The words the message names, in their expected spelling.
    pub words: Vec<String>,
    /// The FSRS rating the verdict gives; none for an unchecked answer.
    pub rating: Option<Rating>,
    /// The AI judge's reason, on a verdict it gave (also a stored one).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub explanation: Option<String>,
    /// The exercise's first accepted answer, under a judged `wrong`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected: Option<String>,
    /// Why an unchecked answer has no verdict: the AI judge's call failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub judge_error: Option<AppError>,
}

/// The rating of `verdict` for an exercise of `kind`, with no hint and no response time: the
/// runner rates again with both when it reviews the card.
fn rating_of(kind: ExerciseType, verdict: Verdict) -> Option<Rating> {
    verdict.srs().map(|verdict| {
        rating_for(&Outcome {
            verdict,
            used_hint: false,
            exercise_type: kind.as_str().to_string(),
            response_ms: None,
            learner_override: false,
        })
    })
}

fn graded(
    kind: ExerciseType,
    verdict: Verdict,
    tags: Vec<Tag>,
    message: Option<String>,
    corrected: Option<String>,
    words: Vec<String>,
) -> Grade {
    Grade {
        verdict,
        tags,
        message,
        corrected,
        words,
        rating: rating_of(kind, verdict),
        explanation: None,
        expected: None,
        judge_error: None,
    }
}

fn correct(kind: ExerciseType) -> Grade {
    graded(kind, Verdict::Correct, Vec::new(), None, None, Vec::new())
}

fn wrong(kind: ExerciseType, corrected: Option<String>) -> Grade {
    graded(
        kind,
        Verdict::Wrong,
        Vec::new(),
        None,
        corrected,
        Vec::new(),
    )
}

/// The noun with its first letter in upper case, to open a sentence.
fn sentence_case(noun: &str) -> String {
    let mut chars = noun.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

fn gender_word(gender: Gender) -> &'static str {
    match gender {
        Gender::M => "masculine",
        Gender::F => "feminine",
        Gender::N => "neuter",
    }
}

/// One candidate answer: as written, and its normal form.
struct Candidate {
    display: String,
    norm: String,
}

/// The exercise's answers in order, then the stored ones, without repeats by normal form (an
/// answer that normalises to nothing is left out).
fn candidates(ctx: &GradeCtx, ex: &Exercise) -> Vec<Candidate> {
    let mut out: Vec<Candidate> = Vec::new();
    let own = ex.own_answers();
    for display in own.into_iter().chain(ctx.stored.iter().map(String::as_str)) {
        let norm = normalise(ctx.lang, display);
        if !norm.is_empty() && !out.iter().any(|c| c.norm == norm) {
            out.push(Candidate {
                display: display.to_string(),
                norm,
            });
        }
    }
    out
}

/// The candidate's display words at `positions`.
fn words_at(candidate: &Candidate, positions: &[usize]) -> Vec<String> {
    let display = display_words(&candidate.display);
    positions
        .iter()
        .filter_map(|&i| display.get(i).cloned())
        .collect()
}

/// Grades one answer (rules of the Design notes, the first that applies): exact; article (with
/// a noun); accent; typo; cached judgement; otherwise `unchecked` for the two free translations
/// and `wrong` for every other type.
pub fn grade(ctx: &GradeCtx, ex: &Exercise, answer: &Answer) -> Grade {
    let kind = ex.kind();
    match &ex.body {
        Body::PickMeaning {
            options,
            answer_index,
            ..
        } => {
            let right = options.get(*answer_index as usize);
            let chosen = match answer {
                Answer::Choice(index) => index == answer_index,
                Answer::Text(text) => right.is_some_and(|option| {
                    let given = normalise(ctx.lang, text);
                    !given.is_empty() && given == normalise(ctx.lang, option)
                }),
                Answer::Pairs(_) => false,
            };
            return if chosen {
                correct(kind)
            } else {
                wrong(kind, right.cloned())
            };
        }
        Body::MatchPairs { pairs } => {
            let matched = match answer {
                Answer::Pairs(given) => {
                    let given: BTreeSet<[u32; 2]> = given.iter().copied().collect();
                    let expected: BTreeSet<[u32; 2]> =
                        (0..pairs.len() as u32).map(|i| [i, i]).collect();
                    given == expected
                }
                Answer::Text(_) | Answer::Choice(_) => false,
            };
            return if matched {
                correct(kind)
            } else {
                wrong(kind, None)
            };
        }
        _ => {}
    }

    let candidates = candidates(ctx, ex);
    let first = candidates.first().map(|c| c.display.clone());
    let Answer::Text(text) = answer else {
        return wrong(kind, first);
    };
    let given = normalise(ctx.lang, text);
    if given.is_empty() {
        return wrong(kind, first);
    }

    // 1. Exact.
    if candidates.iter().any(|c| c.norm == given) {
        return correct(kind);
    }

    // 2. Article.
    if let (Some(noun), Some(target)) = (&ctx.noun, &first) {
        let noun_norm = normalise(ctx.lang, &noun.noun);
        if given == noun_norm {
            return graded(
                kind,
                Verdict::Almost,
                vec![Tag::Article],
                Some(format!("Almost: remember the article: {target}")),
                Some(target.clone()),
                vec![target.clone()],
            );
        }
        let own_article = normalise(ctx.lang, &noun.article);
        let other_article = definite_articles(ctx.lang)
            .iter()
            .filter(|article| **article != own_article)
            .any(|article| given == format!("{article} {noun_norm}"));
        if other_article {
            return graded(
                kind,
                Verdict::Wrong,
                vec![Tag::Article],
                Some(format!(
                    "{} is {}: {target}.",
                    sentence_case(&noun.noun),
                    gender_word(noun.gender)
                )),
                Some(target.clone()),
                vec![target.clone()],
            );
        }
    }

    // 3. Accent.
    if let Some(candidate) = candidates
        .iter()
        .find(|c| accent_equal(ctx.lang, &given, &c.norm))
    {
        let given_words = words(&given);
        let positions: Vec<usize> = words(&candidate.norm)
            .iter()
            .enumerate()
            .filter(|(i, word)| given_words.get(*i) != Some(*word))
            .map(|(i, _)| i)
            .collect();
        let named = words_at(candidate, &positions);
        let list = named.join(", ");
        let (verdict, message) = match ctx.leniency {
            AccentLeniency::Lenient => {
                (Verdict::Almost, format!("Almost: mind the accent: {list}"))
            }
            AccentLeniency::Strict => (Verdict::Wrong, format!("Wrong: mind the accent: {list}")),
            AccentLeniency::Ignore => (Verdict::Correct, format!("Note the accent: {list}")),
        };
        return graded(
            kind,
            verdict,
            vec![Tag::Accent],
            Some(message),
            Some(candidate.display.clone()),
            named,
        );
    }

    // 4. Typo.
    let given_words = words(&given);
    for candidate in &candidates {
        if let Some(found) = typo_match(ctx.lang, ctx.forms, &given_words, &words(&candidate.norm))
        {
            let named = words_at(candidate, &found.positions);
            let mut tags = vec![Tag::Typo];
            if found.accent {
                tags.push(Tag::Accent);
            }
            return graded(
                kind,
                Verdict::Typo,
                tags,
                Some(format!("Typo: {}", named.join(", "))),
                Some(candidate.display.clone()),
                named,
            );
        }
    }

    // 5. A cached judgement, with the judge's reason when it gave one.
    if let Some(judgement) = &ctx.judgement {
        let tags: Vec<Tag> = judgement
            .tags
            .iter()
            .filter_map(|t| Tag::parse(t))
            .collect();
        let mut stored = graded(
            kind,
            judgement.verdict.into(),
            tags,
            None,
            Some(judgement.corrected.clone()),
            Vec::new(),
        );
        if !judgement.explanation.is_empty() {
            stored.explanation = Some(judgement.explanation.clone());
        }
        return stored;
    }

    // 6. Nothing matched.
    match kind {
        ExerciseType::TranslateToTarget | ExerciseType::TranslateToEn => Grade {
            verdict: Verdict::Unchecked,
            tags: Vec::new(),
            message: None,
            corrected: None,
            words: Vec::new(),
            rating: None,
            explanation: None,
            expected: None,
            judge_error: None,
        },
        _ => wrong(kind, first),
    }
}

/// The key of a cached judgement: SHA-256 of the UTF-8 bytes of `lang`, U+001F, the direction,
/// U+001F, the NFC source, U+001F and the answer's normal form, in lower-case hex. Module 08
/// keys the AI judge's verdicts the same way.
pub fn judgement_key(lang: Lang, direction: Direction, source: &str, answer_norm: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(lang.as_str().as_bytes());
    hasher.update("\u{1F}".as_bytes());
    hasher.update(direction.as_str().as_bytes());
    hasher.update("\u{1F}".as_bytes());
    hasher.update(nfc(source).as_bytes());
    hasher.update("\u{1F}".as_bytes());
    hasher.update(answer_norm.as_bytes());
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Whether the exercise asks for the item itself: a `translate_to_target` whose `prompt_en` is
/// one of the item's English texts, or a `translate_to_en`, `listen_type` or `speak` whose
/// prompt is the item's target (NFC compared). Only then do the item's stored answers apply.
pub(crate) fn prompt_is_items_own(item: &ItemRow, ex: &Exercise) -> bool {
    match &ex.body {
        Body::TranslateToTarget { prompt_en, .. } => {
            let prompt = nfc(prompt_en);
            item.en.iter().any(|en| nfc(en) == prompt)
        }
        Body::TranslateToEn {
            prompt_target: text,
            ..
        }
        | Body::ListenType { text, .. }
        | Body::Speak { text, .. } => nfc(text) == nfc(&item.target),
        Body::PickMeaning { .. } | Body::FillGap { .. } | Body::MatchPairs { .. } => false,
    }
}

/// The grading context for `ex` from the store: the item's stored answers in the exercise's
/// direction when the prompt is the item's own, the cached judgement of this answer to this
/// prompt, and the noun of a vocabulary item's `translate_to_target`, `listen_type` or `speak`
/// when its noun is the item's lemma.
pub fn grade_ctx<'a>(
    store: &Store,
    forms: &'a Forms,
    leniency: AccentLeniency,
    ex: &Exercise,
    answer: &Answer,
) -> Result<GradeCtx<'a>, StoreError> {
    let item = store.item(&ex.item_id)?;
    let lang = item.lang;
    let mut ctx = GradeCtx {
        lang,
        leniency,
        forms,
        noun: None,
        stored: Vec::new(),
        judgement: None,
    };
    if let Some(direction) = ex.direction() {
        if prompt_is_items_own(&item, ex) {
            ctx.stored = store
                .accepted_answers(&item.id, direction)?
                .into_iter()
                .map(|answer| answer.display)
                .collect();
        }
        if let (Answer::Text(text), Some(source)) = (answer, ex.prompt()) {
            let given = normalise(lang, text);
            if !given.is_empty() {
                ctx.judgement = store.judgement(&judgement_key(lang, direction, source, &given))?;
            }
        }
    }
    let noun_target = match &ex.body {
        Body::TranslateToTarget { target, .. } => Some(target),
        Body::ListenType { text, .. } | Body::Speak { text, .. } => Some(text),
        _ => None,
    };
    if item.kind == ItemKind::Vocab
        && let Some(target) = noun_target
    {
        ctx.noun = noun_info(lang, target, forms).filter(|info| {
            let noun = nfc(&info.noun);
            item.lemmas.iter().any(|lemma| nfc(lemma) == noun)
        });
    }
    Ok(ctx)
}

/// "I was right": the learner overrules an unchecked grade. For a text answer to an exercise
/// with a direction, the answer (NFC, trimmed) becomes a `learner` accepted answer of the item
/// when the prompt is the item's own, and otherwise a `learner` judgement of this answer to this
/// prompt (verdict correct). Either way the same answer is correct next time, offline. The grade
/// is correct, rated Good.
pub fn accept_learner_answer(
    store: &mut Store,
    ex: &Exercise,
    answer: &Answer,
    now_ms: i64,
) -> Result<Grade, StoreError> {
    let accepted = Grade {
        verdict: Verdict::Correct,
        tags: Vec::new(),
        message: None,
        corrected: None,
        words: Vec::new(),
        rating: Some(Rating::Good),
        explanation: None,
        expected: None,
        judge_error: None,
    };
    let (Answer::Text(text), Some(direction)) = (answer, ex.direction()) else {
        return Ok(accepted);
    };
    let item = store.item(&ex.item_id)?;
    let display = nfc(text).trim().to_string();
    let given = normalise(item.lang, &display);
    if given.is_empty() {
        return Ok(accepted);
    }
    if prompt_is_items_own(&item, ex) {
        store.add_accepted_answer(&item.id, direction, &display, AnswerSource::Learner, now_ms)?;
    } else if let Some(source) = ex.prompt() {
        let judgement = StoredJudgement {
            verdict: srs::Verdict::Correct,
            corrected: display.clone(),
            explanation: String::new(),
            tags: Vec::new(),
            provider: "learner".to_string(),
        };
        store.put_judgement(
            &judgement_key(item.lang, direction, source, &given),
            &judgement,
            now_ms,
        )?;
    }
    Ok(accepted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::FormRow;
    use crate::exercise::Origin;
    use crate::srs::Facet;

    fn row(form: &str, lemma: &str, pos: &str, gender: Option<Gender>) -> FormRow {
        FormRow {
            form: form.into(),
            lemma: lemma.into(),
            pos: pos.into(),
            gender,
            tags: String::new(),
        }
    }

    fn forms() -> Forms {
        Forms::from_rows(vec![
            row("pero", "pero", "conj", None),
            row("día", "día", "noun", Some(Gender::M)),
            row("quiero", "querer", "verb", None),
            row("agua", "agua", "noun", Some(Gender::F)),
        ])
    }

    fn exercise(body: Body) -> Exercise {
        Exercise {
            id: "e1".into(),
            item_id: "i".into(),
            facet: Facet::Productive,
            origin: Origin::Template,
            body,
        }
    }

    fn to_target(expected: &[&str]) -> Exercise {
        exercise(Body::TranslateToTarget {
            prompt_en: "x".into(),
            target: expected[0].into(),
            accepted: expected.iter().map(|s| s.to_string()).collect(),
            tiles: None,
        })
    }

    fn listen(expected: &str) -> Exercise {
        exercise(Body::ListenType {
            text: expected.into(),
            accepted: vec![expected.into()],
        })
    }

    fn ctx(forms: &Forms, lang: Lang, leniency: AccentLeniency) -> GradeCtx<'_> {
        GradeCtx {
            lang,
            leniency,
            forms,
            noun: None,
            stored: Vec::new(),
            judgement: None,
        }
    }

    fn text(s: &str) -> Answer {
        Answer::Text(s.into())
    }

    #[test]
    fn exact_matches_ignore_case_and_punctuation() {
        let forms = forms();
        let c = ctx(&forms, Lang::Es, AccentLeniency::Lenient);
        let g = grade(&c, &to_target(&["Hola, ¿qué tal?"]), &text("hola qué tal"));
        assert_eq!(g.verdict, Verdict::Correct);
        assert_eq!((g.tags.len(), g.message, g.corrected), (0, None, None));
        assert_eq!(g.rating, Some(Rating::Good));
        let g = grade(&c, &to_target(&["Hola, ¿qué tal?"]), &text("Hola que tal"));
        assert_eq!(g.verdict, Verdict::Almost);
        assert_eq!(g.tags, [Tag::Accent]);
        assert_eq!(g.message.as_deref(), Some("Almost: mind the accent: qué"));
        assert_eq!(g.words, ["qué"]);
        assert_eq!(g.corrected.as_deref(), Some("Hola, ¿qué tal?"));
    }

    #[test]
    fn accent_leniency_sets_the_verdict_and_rating() {
        let forms = forms();
        let ex = listen("Buenos días.");
        let answer = text("Buenos dias");
        let lenient = grade(
            &ctx(&forms, Lang::Es, AccentLeniency::Lenient),
            &ex,
            &answer,
        );
        assert_eq!(
            (lenient.verdict, lenient.rating),
            (Verdict::Almost, Some(Rating::Hard))
        );
        let strict = grade(&ctx(&forms, Lang::Es, AccentLeniency::Strict), &ex, &answer);
        assert_eq!(
            (strict.verdict, strict.rating),
            (Verdict::Wrong, Some(Rating::Again))
        );
        assert_eq!(
            strict.message.as_deref(),
            Some("Wrong: mind the accent: días")
        );
        let ignore = grade(&ctx(&forms, Lang::Es, AccentLeniency::Ignore), &ex, &answer);
        assert_eq!(
            (ignore.verdict, ignore.rating),
            (Verdict::Correct, Some(Rating::Good))
        );
        assert_eq!(ignore.message.as_deref(), Some("Note the accent: días"));
        assert_eq!(ignore.tags, [Tag::Accent]);
    }

    #[test]
    fn german_umlauts_and_sharp_s_have_their_spellings() {
        let forms = forms();
        let c = ctx(&forms, Lang::De, AccentLeniency::Lenient);
        for answer in ["das Maedchen", "das Madchen"] {
            let g = grade(&c, &listen("Das Mädchen"), &text(answer));
            assert_eq!(g.verdict, Verdict::Almost, "{answer}");
            assert_eq!(g.words, ["Mädchen"]);
        }
        let g = grade(&c, &listen("die Straße"), &text("die Strasse"));
        assert_eq!(g.verdict, Verdict::Almost);
        let es = ctx(&forms, Lang::Es, AccentLeniency::Lenient);
        assert_eq!(
            grade(&es, &listen("Straße"), &text("strasse")).verdict,
            Verdict::Wrong
        );
    }

    #[test]
    fn the_article_rule_needs_a_noun() {
        let forms = forms();
        let mut c = ctx(&forms, Lang::Es, AccentLeniency::Lenient);
        c.noun = noun_info(Lang::Es, "el día", &forms);
        let ex = to_target(&["el día"]);
        let g = grade(&c, &ex, &text("día"));
        assert_eq!(g.verdict, Verdict::Almost);
        assert_eq!(g.tags, [Tag::Article]);
        assert_eq!(
            g.message.as_deref(),
            Some("Almost: remember the article: el día")
        );
        assert_eq!(g.rating, Some(Rating::Hard));
        let g = grade(&c, &ex, &text("la día"));
        assert_eq!(g.verdict, Verdict::Wrong);
        assert_eq!(g.message.as_deref(), Some("Día is masculine: el día."));
        assert_eq!(g.corrected.as_deref(), Some("el día"));
        c.noun = noun_info(Lang::Es, "el agua", &forms);
        let g = grade(&c, &to_target(&["el agua"]), &text("la agua"));
        assert_eq!(g.message.as_deref(), Some("Agua is feminine: el agua."));
        let plain = ctx(&forms, Lang::Es, AccentLeniency::Lenient);
        assert_eq!(
            grade(&plain, &listen("el día"), &text("día")).verdict,
            Verdict::Wrong
        );
    }

    #[test]
    fn noun_info_reads_the_article_and_gender() {
        let forms = forms();
        let info = noun_info(Lang::Es, "el agua", &forms).unwrap();
        assert_eq!(
            (info.article.as_str(), info.noun.as_str(), info.gender),
            ("el", "agua", Gender::F)
        );
        let info = noun_info(Lang::Es, "la noche", &Forms::default()).unwrap();
        assert_eq!(info.gender, Gender::F);
        let info = noun_info(Lang::De, "das Haus", &Forms::default()).unwrap();
        assert_eq!(info.gender, Gender::N);
        assert_eq!(
            noun_info(Lang::Fr, "l'homme", &Forms::default()),
            None,
            "l' shows no gender"
        );
        let homme = Forms::from_rows(vec![row("homme", "homme", "noun", Some(Gender::M))]);
        let info = noun_info(Lang::Fr, "l\u{2019}homme", &homme).unwrap();
        assert_eq!(
            (info.article.as_str(), info.noun.as_str()),
            ("l\u{2019}", "homme")
        );
        assert_eq!(noun_info(Lang::Es, "hola", &forms), None);
        assert_eq!(noun_info(Lang::Es, "un perro", &forms), None);
        assert_eq!(noun_info(Lang::Es, "los perros", &forms), None, "no gender");
    }

    #[test]
    fn typos_are_forgiven_in_long_words_only() {
        let forms = forms();
        let c = ctx(&forms, Lang::Es, AccentLeniency::Lenient);
        let sentence = listen("Quiero un café con leche.");
        let g = grade(&c, &sentence, &text("Quiero un café con lecje"));
        assert_eq!(g.verdict, Verdict::Typo);
        assert_eq!(g.tags, [Tag::Typo]);
        assert_eq!(g.message.as_deref(), Some("Typo: leche"));
        assert_eq!(g.rating, Some(Rating::Hard));
        assert_eq!(
            grade(&c, &listen("el perro"), &text("el pero")).verdict,
            Verdict::Wrong
        );
        let fr = ctx(&forms, Lang::Fr, AccentLeniency::Lenient);
        assert_eq!(
            grade(&fr, &listen("le livre"), &text("la livre")).verdict,
            Verdict::Wrong
        );
        let g = grade(&c, &sentence, &text("quiero un cafe con lecje"));
        assert_eq!(g.tags, [Tag::Typo, Tag::Accent]);
        assert_eq!(g.words, ["café", "leche"]);
    }

    #[test]
    fn free_translations_are_unchecked_and_other_types_wrong() {
        let forms = forms();
        let c = ctx(&forms, Lang::Es, AccentLeniency::Lenient);
        let g = grade(&c, &to_target(&["Buenos días."]), &text("Buenas tardes"));
        assert_eq!(
            (g.verdict, g.rating, g.corrected),
            (Verdict::Unchecked, None, None)
        );
        let gap = exercise(Body::FillGap {
            sentence: "Buenos ___.".into(),
            answer: "días".into(),
            full_sentence: "Buenos días.".into(),
            hint_en: "day".into(),
        });
        let g = grade(&c, &gap, &text("noches"));
        assert_eq!((g.verdict, g.rating), (Verdict::Wrong, Some(Rating::Again)));
        assert_eq!(g.corrected.as_deref(), Some("días"));
        let g = grade(&c, &gap, &text(" ¿? "));
        assert_eq!((g.verdict, g.message), (Verdict::Wrong, None));
    }

    #[test]
    fn stored_answers_and_judgements_are_tried_in_order() {
        let forms = forms();
        let mut c = ctx(&forms, Lang::Es, AccentLeniency::Lenient);
        c.stored = vec!["Buen día.".into()];
        assert_eq!(
            grade(&c, &to_target(&["Buenos días."]), &text("buen día")).verdict,
            Verdict::Correct
        );
        c.judgement = Some(StoredJudgement {
            verdict: srs::Verdict::Almost,
            corrected: "Buenas tardes.".into(),
            explanation: String::new(),
            tags: vec!["typo".into(), "grammar".into()],
            provider: "publik".into(),
        });
        let g = grade(&c, &to_target(&["Buenos días."]), &text("buenas tarde"));
        assert_eq!(g.verdict, Verdict::Almost);
        assert_eq!(g.corrected.as_deref(), Some("Buenas tardes."));
        assert_eq!(g.explanation, None, "an empty explanation is left out");
        assert_eq!((g.tags, g.message), (vec![Tag::Typo], None));
        if let Some(judgement) = c.judgement.as_mut() {
            judgement.explanation = "Tarde is singular here.".into();
        }
        let g = grade(&c, &to_target(&["Buenos días."]), &text("buenas tarde"));
        assert_eq!(g.explanation.as_deref(), Some("Tarde is singular here."));
        assert_eq!(g.expected, None);
    }

    #[test]
    fn choices_and_pairs_are_right_or_wrong() {
        let forms = forms();
        let c = ctx(&forms, Lang::Es, AccentLeniency::Lenient);
        let pick = exercise(Body::PickMeaning {
            prompt_target: "el día".into(),
            options: vec!["night".into(), "day".into(), "hello".into(), "good".into()],
            answer_index: 1,
        });
        assert_eq!(
            grade(&c, &pick, &Answer::Choice(1)).verdict,
            Verdict::Correct
        );
        assert_eq!(grade(&c, &pick, &text("Day")).verdict, Verdict::Correct);
        let g = grade(&c, &pick, &Answer::Choice(0));
        assert_eq!(
            (g.verdict, g.corrected.as_deref()),
            (Verdict::Wrong, Some("day"))
        );
        let pairs = exercise(Body::MatchPairs {
            pairs: (0..4).map(|i| [format!("t{i}"), format!("e{i}")]).collect(),
        });
        let right = Answer::Pairs(vec![[2, 2], [0, 0], [3, 3], [1, 1]]);
        assert_eq!(grade(&c, &pairs, &right).verdict, Verdict::Correct);
        let crossed = Answer::Pairs(vec![[0, 1], [1, 0], [2, 2], [3, 3]]);
        assert_eq!(grade(&c, &pairs, &crossed).verdict, Verdict::Wrong);
        assert_eq!(
            grade(&c, &pairs, &Answer::Pairs(vec![[0, 0]])).verdict,
            Verdict::Wrong
        );
    }

    #[test]
    fn serde_shapes_follow_the_spec() {
        assert_eq!(
            serde_json::to_string(&text("hola")).unwrap(),
            r#"{"text":"hola"}"#
        );
        assert_eq!(
            serde_json::to_string(&Answer::Choice(2)).unwrap(),
            r#"{"choice":2}"#
        );
        assert_eq!(
            serde_json::to_string(&Answer::Pairs(vec![[0, 1]])).unwrap(),
            r#"{"pairs":[[0,1]]}"#
        );
        assert_eq!(
            serde_json::to_string(&Direction::ToEn).unwrap(),
            "\"to_en\""
        );
        assert_eq!(
            serde_json::to_string(&AccentLeniency::Ignore).unwrap(),
            "\"ignore\""
        );
        assert_eq!(AccentLeniency::default(), AccentLeniency::Lenient);
        let g = Grade {
            verdict: Verdict::Unchecked,
            tags: vec![Tag::Article],
            message: None,
            corrected: None,
            words: vec![],
            rating: None,
            explanation: None,
            expected: None,
            judge_error: None,
        };
        assert_eq!(
            serde_json::to_string(&g).unwrap(),
            r#"{"verdict":"unchecked","tags":["article"],"message":null,"corrected":null,"words":[],"rating":null}"#
        );
        assert_eq!(Verdict::Typo.srs(), Some(srs::Verdict::Almost));
        assert_eq!(Verdict::Unchecked.srs(), None);
    }

    #[test]
    fn judgement_keys_hash_the_four_parts() {
        // SHA-256 of "es", U+001F, "to_en", U+001F, "Hola", U+001F, "hello", computed with
        // sha256sum.
        assert_eq!(
            judgement_key(Lang::Es, Direction::ToEn, "Hola", "hello"),
            "27b0dbedda726fa4a3964db678428f54cb23269dafaf63d4a9c34f84e8ad91b5"
        );
        assert_eq!(
            judgement_key(Lang::Es, Direction::ToEn, "e\u{301}l", "he"),
            judgement_key(Lang::Es, Direction::ToEn, "\u{e9}l", "he"),
            "the source is compared in NFC"
        );
    }
}
