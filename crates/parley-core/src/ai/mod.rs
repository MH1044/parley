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
mod money;
mod prompts;
mod schema;

pub use cache::{Cache, CacheEntry, cache_key, cache_key_json, canonical_json, idempotency_key};
pub use call::{Called, ConnectionCheck, RuleFailure, call, check_connection};
pub use client::{
    AiClient, AiError, AiStatus, AudioReply, ChatMessage, ChatReply, ChatRequest, LocalModel,
    MAX_REQUEST_BYTES, NO_TOKEN_MESSAGE, ProviderKind, SpeechRequest, TranscribeRequest,
    TranscriptReply, image_part,
};
pub use money::format_dollars;
pub use prompts::{Prompt, PromptError, RenderError, ping};
pub use schema::parse_reply;
