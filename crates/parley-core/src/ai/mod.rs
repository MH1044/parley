//! AI in the core (docs/modules/06-publik-and-providers.md; docs/PUBLIK-INTEGRATION.md §6): the
//! [`AiClient`] trait every feature calls, the [`call`] pipeline, the response [`Cache`], prompt
//! files and their schemas, and amounts in dollars.
//!
//! The core makes no HTTP request of its own: the `parley-publik` crate implements [`AiClient`]
//! for publik API, the learner's own key and a local model server, and the desktop app passes the
//! client in. Every answer is parsed, checked against its JSON Schema and the feature's rules, and
//! stored only when it passes (invariant I2).

mod cache;
mod call;
mod client;
mod langid;
mod lesson;
mod money;
mod prompts;
mod schema;
mod story;
mod validate;

pub use cache::{Cache, CacheEntry, cache_key, cache_key_json, canonical_json, idempotency_key};
pub use call::{Called, ConnectionCheck, RuleFailure, call, check_connection};
pub use client::{
    AiClient, AiError, AiStatus, AudioReply, ChatMessage, ChatReply, ChatRequest, LocalModel,
    MAX_REQUEST_BYTES, NO_TOKEN_MESSAGE, ProviderKind, SpeechRequest, TranscribeRequest,
    TranscriptReply, image_part,
};
pub use langid::{LANGID_THRESHOLD, TextLang, identify, is_language, stem};
pub use lesson::{
    FRESH_LESSON_UNAVAILABLE, LESSON_PROMPT_ID, LessonAiInfo, LessonChecks, LessonCtx, LessonDraft,
    LessonPrompt, LessonResponse, OFFLINE_LESSON, PromptEntry, allowed_lemmas, build_lesson,
    generate_lesson, lesson_checks, lesson_prompt_file, request_lesson,
};
pub use money::format_dollars;
pub use prompts::{Prompt, PromptError, RenderError, ping};
pub use schema::parse_reply;
pub use story::{
    STORY_PROMPT_ID, Story, StoryCtx, StoryPrompt, StoryQuestion, check_story, generate_story,
    story_prompt_file, story_xp,
};
pub use validate::{allowed_from_list, coverage_with_stems};

pub(crate) use story::{has_story, story_schema_valid};
pub(crate) use validate::{AiCheck, check_ai_exercise, stems_of};
