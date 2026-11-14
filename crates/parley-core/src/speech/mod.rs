//! Speaking checks (docs/modules/10-speaking-and-pronunciation.md): the words of a sentence and of
//! what was heard ([`speech_words`]), their alignment ([`align`], Wagner and Fischer 1974), the
//! score with its verdict, rating and grade ([`score`]), and the local Whisper models
//! ([`ModelRegistry`]). Nothing here records or transcribes: the take arrives as text, from the
//! provider or from local Whisper, and is scored the same way either way.

pub mod align;
pub mod models;
pub mod score;

pub use align::{Edit, align};
pub use models::{
    DEFAULT_WHISPER_MODEL, ModelEntry, ModelRegistry, WHISPER_DIR, bundled_models, model_file,
    model_verified, parse_model_registry,
};
pub use score::{
    SpeakScore, WordResult, WordStatus, score, speak_grade, speak_rating, speak_verdict,
    speech_words, word_key,
};

pub(crate) use models::model_file_name;
pub(crate) use score::without_brackets;
