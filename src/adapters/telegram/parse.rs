//! Classify a Telegram [`Message`] into one of four outcomes that the polling
//! loop knows how to handle. Pure function — no I/O — so it is exhaustively
//! unit-tested.

use crate::domain::ports::{IncomingMessage, IncomingPayload};

use super::api::Message;

/// What the polling loop should do with a parsed Telegram message.
#[derive(Debug, PartialEq, Eq)]
pub enum Parsed {
    /// A text-bearing message worth storing as-is (text, link, caption).
    Incoming(IncomingMessage),
    /// A voice message: the adapter must download it, transcribe, and emit an
    /// `IncomingPayload::Voice(transcript)` afterwards. The skeleton carries
    /// every field of the eventual [`IncomingMessage`] except the payload.
    Voice {
        file_id: String,
        mime_type: Option<String>,
        skeleton: MessageSkeleton,
    },
    /// Type we don't yet support; the bot must reply in chat with this reason.
    Unsupported(&'static str),
}

/// Common Telegram-side fields shared by all parsed variants — extracted up
/// front so the adapter can build the final [`IncomingMessage`] after async
/// work without re-reading the original `Message`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageSkeleton {
    pub chat_id: i64,
    pub message_id: i64,
    pub user_id: Option<i64>,
    pub username: Option<String>,
    pub date: i64,
}

impl MessageSkeleton {
    pub fn into_incoming(self, payload: IncomingPayload) -> IncomingMessage {
        IncomingMessage {
            chat_id: self.chat_id,
            message_id: self.message_id,
            user_id: self.user_id,
            username: self.username,
            date: self.date,
            payload,
        }
    }
}

/// Reply sent to the user when their message kind isn't supported. Stable
/// wording so users learn what works without reading the README.
pub const UNSUPPORTED_BINARY: &str =
    "Поддерживается только текст: пришли подпись (caption) к медиа или сам текст.";
pub const UNSUPPORTED_VIDEO_NOTE: &str =
    "Видео-кружки пока не поддержаны: пришли голосовое или текст.";
pub const UNSUPPORTED_OTHER: &str = "Этот тип сообщения не поддержан; пришли текст.";

pub fn parse(msg: &Message) -> Parsed {
    if let Some(text) = msg.text.as_ref().filter(|s| !s.trim().is_empty()) {
        return Parsed::Incoming(skeleton(msg).into_incoming(IncomingPayload::Text(text.clone())));
    }
    if let Some(caption) = msg.caption.as_ref().filter(|s| !s.trim().is_empty()) {
        return Parsed::Incoming(
            skeleton(msg).into_incoming(IncomingPayload::Caption(caption.clone())),
        );
    }
    if let Some(voice) = msg.voice.as_ref() {
        return Parsed::Voice {
            file_id: voice.file_id.clone(),
            mime_type: voice.mime_type.clone(),
            skeleton: skeleton(msg),
        };
    }
    if msg.video_note.is_some() {
        return Parsed::Unsupported(UNSUPPORTED_VIDEO_NOTE);
    }
    if msg.photo.is_some()
        || msg.video.is_some()
        || msg.document.is_some()
        || msg.audio.is_some()
        || msg.animation.is_some()
        || msg.sticker.is_some()
    {
        return Parsed::Unsupported(UNSUPPORTED_BINARY);
    }
    Parsed::Unsupported(UNSUPPORTED_OTHER)
}

fn skeleton(msg: &Message) -> MessageSkeleton {
    MessageSkeleton {
        chat_id: msg.chat.id,
        message_id: msg.message_id,
        user_id: msg.from.as_ref().map(|u| u.id),
        username: msg.from.as_ref().and_then(|u| u.username.clone()),
        date: msg.date,
    }
}

#[cfg(test)]
mod tests;
