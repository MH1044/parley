//! A scripted AI client for the core's unit tests: it answers chat from a list, records every
//! request, and reports the status and provider it is given.

use std::sync::Mutex;

use super::client::{
    AiClient, AiError, AiStatus, AudioReply, ChatReply, ChatRequest, ProviderKind, SpeechRequest,
    TranscribeRequest, TranscriptReply,
};

pub(crate) struct Scripted {
    replies: Mutex<Vec<Result<ChatReply, AiError>>>,
    pub seen: Mutex<Vec<ChatRequest>>,
    pub status: Mutex<AiStatus>,
    pub kind: ProviderKind,
}

impl Scripted {
    /// A publik API client that gives `replies` in order, then fails as offline.
    pub fn new(replies: Vec<Result<ChatReply, AiError>>) -> Scripted {
        Scripted {
            replies: Mutex::new(replies),
            seen: Mutex::new(Vec::new()),
            status: Mutex::new(AiStatus {
                provider: "publik".into(),
                connected: true,
                ..AiStatus::default()
            }),
            kind: ProviderKind::Publik,
        }
    }

    /// A reply with this content and charge.
    pub fn reply(content: &str, charge: Option<i64>) -> Result<ChatReply, AiError> {
        Ok(ChatReply {
            content: content.into(),
            charge_micros: charge,
            request_id: None,
        })
    }

    /// How many requests were made.
    pub fn requests(&self) -> usize {
        self.seen.lock().unwrap().len()
    }

    /// The idempotency key of each request, in order.
    pub fn keys(&self) -> Vec<String> {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .map(|r| r.idempotency_key.clone().unwrap_or_default())
            .collect()
    }
}

impl AiClient for Scripted {
    fn chat(&self, req: ChatRequest) -> Result<ChatReply, AiError> {
        self.seen.lock().unwrap().push(req);
        let mut replies = self.replies.lock().unwrap();
        if replies.is_empty() {
            Err(AiError::Offline)
        } else {
            replies.remove(0)
        }
    }
    fn chat_stream(
        &self,
        req: ChatRequest,
        _on_delta: &mut dyn FnMut(&str),
    ) -> Result<ChatReply, AiError> {
        self.chat(req)
    }
    fn transcribe(&self, _req: TranscribeRequest) -> Result<TranscriptReply, AiError> {
        Err(AiError::Offline)
    }
    fn speech(&self, _req: SpeechRequest) -> Result<AudioReply, AiError> {
        Err(AiError::Offline)
    }
    fn kind(&self) -> ProviderKind {
        self.kind
    }
    fn status(&self) -> AiStatus {
        self.status.lock().unwrap().clone()
    }
    fn chat_model(&self, tier: &str) -> String {
        tier.to_string()
    }
}
