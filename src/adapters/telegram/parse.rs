//! Classify a Telegram [`Message`] into one of three outcomes that the polling
//! loop knows how to handle. Pure function — no I/O — so it is exhaustively
//! unit-tested.

use crate::domain::ports::{IncomingMessage, IncomingPayload};

use super::api::Message;

/// What the polling loop should do with a parsed Telegram message.
#[derive(Debug, PartialEq, Eq)]
pub enum Parsed {
    /// A message worth storing.
    Incoming(IncomingMessage),
    /// Type we don't yet support; the bot must reply in chat with this reason.
    Unsupported(&'static str),
}

/// Reply sent to the user when their message kind isn't supported. Stable
/// wording so users learn what works without reading the README.
pub const UNSUPPORTED_BINARY: &str =
    "Поддерживается только текст: пришли подпись (caption) к медиа или сам текст.";
pub const UNSUPPORTED_VOICE: &str = "Голосовые пока не поддержаны — расшифровка появится в слайсе 3.";
pub const UNSUPPORTED_OTHER: &str = "Этот тип сообщения не поддержан; пришли текст.";

pub fn parse(msg: &Message) -> Parsed {
    if let Some(text) = msg.text.as_ref().filter(|s| !s.trim().is_empty()) {
        return Parsed::Incoming(build(msg, IncomingPayload::Text(text.clone())));
    }
    if let Some(caption) = msg.caption.as_ref().filter(|s| !s.trim().is_empty()) {
        return Parsed::Incoming(build(msg, IncomingPayload::Caption(caption.clone())));
    }
    if msg.voice.is_some() || msg.video_note.is_some() {
        return Parsed::Unsupported(UNSUPPORTED_VOICE);
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

fn build(msg: &Message, payload: IncomingPayload) -> IncomingMessage {
    IncomingMessage {
        chat_id: msg.chat.id,
        message_id: msg.message_id,
        user_id: msg.from.as_ref().map(|u| u.id),
        username: msg.from.as_ref().and_then(|u| u.username.clone()),
        date: msg.date,
        payload,
    }
}

#[cfg(test)]
mod tests;
