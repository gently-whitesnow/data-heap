//! Save-confirmation reply and inline "delete" callback handling.
//!
//! After the daemon persists an item, [`send_confirmation`] echoes the saved
//! text back with a single inline button whose `callback_data` carries the
//! item id. When that button is pressed Telegram delivers a `callback_query`,
//! [`handle_delete_callback`] tombstones the item and edits the original
//! confirmation message so the button disappears.

use reqwest::Client;
use secrecy::{ExposeSecret, SecretString};
use serde_json::{json, Value};

use crate::domain::error::{Error, Result};
use crate::domain::item::ItemId;
use crate::domain::ports::Storage;

use super::api::{CallbackQuery, Response};

/// Preview cap for text/link/caption echoes. Pasting a wall of text just to
/// confirm a save buries the delete button and clutters the chat — truncate
/// long items to a leading preview. Voice items bypass this (see
/// [`send_confirmation`]) because the chat echo is the only transcript the user
/// ever sees, so a cut there reads as lost data.
const PREVIEW_CHAR_CAP: usize = 500;

/// Hard per-message char limit imposed by Telegram. Full voice transcripts are
/// split into chunks no larger than this before sending.
const TELEGRAM_MSG_CHAR_CAP: usize = 4096;

/// Prefix for the delete-item callback. Keeps `callback_data` self-describing
/// in case other actions are added later (e.g. `kind:delete:<id>` style).
const DELETE_CALLBACK_PREFIX: &str = "delete:";

/// Button label and confirmation banners. Constants so wording stays uniform
/// across the reply flow and the post-delete edit.
const DELETE_BUTTON_LABEL: &str = "🗑 удалить";
const DELETED_NOTICE: &str = "🗑 удалено";
const TOAST_DELETED: &str = "Удалено";
const TOAST_ALREADY_GONE: &str = "Уже удалено";

pub(super) fn method_url(base_url: &str, token: &SecretString, method: &str) -> String {
    format!("{base_url}/bot{}/{method}", token.expose_secret())
}

/// How much of the saved text to echo back into the chat.
pub enum Echo<'a> {
    /// Truncated leading preview — text, links, captions.
    Preview(&'a str),
    /// Full text, split across messages — voice transcripts, where the chat
    /// echo is the only transcript the user sees and a cut reads as lost data.
    Full(&'a str),
}

/// Send the post-save echo with the delete button. Failures are logged by the
/// caller; this returns an error so the caller can decide whether to surface
/// it (Telegram rejecting a reply is non-fatal for the polling loop).
///
/// An [`Echo::Full`] payload is split across as many messages as Telegram's
/// per-message limit requires, with the delete button on the last one. An
/// [`Echo::Preview`] is sent as a single truncated message.
pub async fn send_confirmation(
    http: &Client,
    base_url: &str,
    token: &SecretString,
    chat_id: i64,
    reply_to_message_id: i64,
    echo: Echo<'_>,
    item_id: ItemId,
) -> Result<()> {
    let saved_text = match echo {
        Echo::Preview(text) => {
            return send_message(
                http,
                base_url,
                token,
                chat_id,
                &preview(text),
                Some(reply_to_message_id),
                Some(delete_keyboard(item_id)),
            )
            .await;
        }
        Echo::Full(text) => text,
    };

    let chunks = chunk_message(saved_text);
    let last = chunks.len() - 1;
    for (i, chunk) in chunks.iter().enumerate() {
        // Reply-link only the first message; hang the delete button on the last.
        let reply_to = (i == 0).then_some(reply_to_message_id);
        let markup = (i == last).then(|| delete_keyboard(item_id));
        send_message(http, base_url, token, chat_id, chunk, reply_to, markup).await?;
    }
    Ok(())
}

/// Send one chat message, optionally as a reply and optionally with an inline
/// keyboard. Shared by the single-preview and chunked-voice paths.
async fn send_message(
    http: &Client,
    base_url: &str,
    token: &SecretString,
    chat_id: i64,
    text: &str,
    reply_to_message_id: Option<i64>,
    reply_markup: Option<Value>,
) -> Result<()> {
    let mut body = json!({ "chat_id": chat_id, "text": text });
    if let Some(reply_to) = reply_to_message_id {
        body["reply_parameters"] = json!({ "message_id": reply_to });
    }
    if let Some(markup) = reply_markup {
        body["reply_markup"] = markup;
    }
    let resp: Response<Value> = http
        .post(method_url(base_url, token, "sendMessage"))
        .json(&body)
        .send()
        .await?
        .json()
        .await?;
    if !resp.ok {
        return Err(Error::Telegram(format!(
            "sendMessage: {}",
            resp.description.unwrap_or_else(|| "unknown error".into())
        )));
    }
    Ok(())
}

/// Process a callback_query payload: parse the action, soft-delete the item,
/// edit the confirmation message to clear the button, and answer the query so
/// Telegram stops showing the spinner on the user's button.
pub async fn handle_delete_callback(
    http: &Client,
    base_url: &str,
    token: &SecretString,
    storage: &dyn Storage,
    query: &CallbackQuery,
) -> Result<()> {
    let Some(item_id) = query.data.as_deref().and_then(parse_delete_callback) else {
        // Unknown action — still ack so the spinner stops, then no-op.
        answer_callback(http, base_url, token, &query.id, None).await?;
        return Ok(());
    };

    let already_gone = storage.get_item(item_id).await?.is_none();
    storage.delete_item(item_id).await?;

    if let Some(msg) = query.message.as_ref() {
        if let Err(e) =
            edit_to_deleted_notice(http, base_url, token, msg.chat.id, msg.message_id).await
        {
            tracing::warn!(error = %e, "edit confirmation message failed");
        }
    }

    let toast = if already_gone {
        TOAST_ALREADY_GONE
    } else {
        TOAST_DELETED
    };
    answer_callback(http, base_url, token, &query.id, Some(toast)).await?;
    Ok(())
}

fn delete_keyboard(item_id: ItemId) -> Value {
    json!({
        "inline_keyboard": [[{
            "text": DELETE_BUTTON_LABEL,
            "callback_data": format!("{DELETE_CALLBACK_PREFIX}{}", item_id.0),
        }]]
    })
}

fn parse_delete_callback(data: &str) -> Option<ItemId> {
    let raw = data.strip_prefix(DELETE_CALLBACK_PREFIX)?;
    raw.parse::<i64>().ok().map(ItemId)
}

fn preview(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= PREVIEW_CHAR_CAP {
        return trimmed.to_string();
    }
    let head: String = trimmed.chars().take(PREVIEW_CHAR_CAP).collect();
    format!("{head}…")
}

/// Split text into Telegram-sized chunks, counting by chars (not bytes) so a
/// multibyte boundary is never split. Always yields at least one chunk so the
/// caller can unconditionally place the delete button on the last one.
fn chunk_message(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.trim().chars().collect();
    if chars.is_empty() {
        return vec![String::new()];
    }
    chars
        .chunks(TELEGRAM_MSG_CHAR_CAP)
        .map(|c| c.iter().collect())
        .collect()
}

async fn edit_to_deleted_notice(
    http: &Client,
    base_url: &str,
    token: &SecretString,
    chat_id: i64,
    message_id: i64,
) -> Result<()> {
    let body = json!({
        "chat_id": chat_id,
        "message_id": message_id,
        "text": DELETED_NOTICE,
    });
    let resp: Response<Value> = http
        .post(method_url(base_url, token, "editMessageText"))
        .json(&body)
        .send()
        .await?
        .json()
        .await?;
    if !resp.ok {
        // "message is not modified" is harmless: same edit issued twice.
        let desc = resp.description.unwrap_or_default();
        if !desc.contains("message is not modified") {
            return Err(Error::Telegram(format!("editMessageText: {desc}")));
        }
    }
    Ok(())
}

async fn answer_callback(
    http: &Client,
    base_url: &str,
    token: &SecretString,
    callback_query_id: &str,
    text: Option<&str>,
) -> Result<()> {
    let mut body = json!({ "callback_query_id": callback_query_id });
    if let Some(t) = text {
        body["text"] = Value::String(t.into());
    }
    let resp: Response<Value> = http
        .post(method_url(base_url, token, "answerCallbackQuery"))
        .json(&body)
        .send()
        .await?
        .json()
        .await?;
    if !resp.ok {
        tracing::warn!(
            error = %resp.description.unwrap_or_default(),
            "telegram answerCallbackQuery rejected"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_delete_callback() {
        assert_eq!(parse_delete_callback("delete:42"), Some(ItemId(42)));
    }

    #[test]
    fn rejects_non_delete_callback() {
        assert!(parse_delete_callback("other:42").is_none());
        assert!(parse_delete_callback("delete:abc").is_none());
        assert!(parse_delete_callback("").is_none());
    }

    #[test]
    fn preview_keeps_short_text_verbatim() {
        assert_eq!(preview("hello"), "hello");
        assert_eq!(preview("  trimmed  "), "trimmed");
    }

    #[test]
    fn preview_truncates_long_text_with_ellipsis() {
        let long: String = "а".repeat(PREVIEW_CHAR_CAP + 50);
        let p = preview(&long);
        assert_eq!(p.chars().count(), PREVIEW_CHAR_CAP + 1);
        assert!(p.ends_with('…'));
    }

    #[test]
    fn preview_respects_char_boundary_for_multibyte() {
        let mixed: String = "💡".repeat(PREVIEW_CHAR_CAP + 10);
        let p = preview(&mixed);
        assert!(p.ends_with('…'));
        assert_eq!(p.chars().count(), PREVIEW_CHAR_CAP + 1);
    }

    #[test]
    fn chunk_keeps_short_text_as_single_message() {
        let chunks = chunk_message("  short voice note  ");
        assert_eq!(chunks, vec!["short voice note".to_string()]);
    }

    #[test]
    fn chunk_splits_long_text_at_telegram_limit() {
        let long: String = "а".repeat(TELEGRAM_MSG_CHAR_CAP * 2 + 17);
        let chunks = chunk_message(&long);
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].chars().count(), TELEGRAM_MSG_CHAR_CAP);
        assert_eq!(chunks[1].chars().count(), TELEGRAM_MSG_CHAR_CAP);
        assert_eq!(chunks[2].chars().count(), 17);
        let rejoined: String = chunks.concat();
        assert_eq!(rejoined.chars().count(), long.chars().count());
    }

    #[test]
    fn chunk_counts_chars_not_bytes() {
        let emoji: String = "💡".repeat(TELEGRAM_MSG_CHAR_CAP + 5);
        let chunks = chunk_message(&emoji);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].chars().count(), TELEGRAM_MSG_CHAR_CAP);
        assert_eq!(chunks[1].chars().count(), 5);
    }

    #[test]
    fn chunk_empty_yields_one_empty_message() {
        assert_eq!(chunk_message("   "), vec![String::new()]);
    }
}
