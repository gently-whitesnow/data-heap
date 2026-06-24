//! Voice flow: Telegram `getFile` → download bytes → transcribe.
//!
//! The bytes never touch disk: `reqwest::bytes()` already yields a
//! `Bytes` buffer that `multipart::Part::stream` consumes without copying.

use bytes::Bytes;
use reqwest::Client;
use secrecy::{ExposeSecret, SecretString};
use serde_json::json;

use crate::domain::error::{Error, Result};
use crate::domain::ports::Transcription;

use super::api::{FileInfo, Response};

/// Telegram caps `getFile` downloads at 20 MB. Reject larger payloads early so
/// we don't waste time downloading something a hosted provider will refuse.
const MAX_VOICE_BYTES: usize = 20 * 1024 * 1024;

pub async fn transcribe_voice(
    http: &Client,
    base_url: &str,
    token: &SecretString,
    file_id: &str,
    mime_type: Option<&str>,
    transcription: &dyn Transcription,
) -> Result<String> {
    let info = get_file(http, base_url, token, file_id).await?;
    if let Some(size) = info.file_size {
        if size as usize > MAX_VOICE_BYTES {
            return Err(Error::Transcription(format!(
                "voice file is {size} bytes; Telegram getFile cap is {MAX_VOICE_BYTES}"
            )));
        }
    }
    let bytes = download(http, base_url, token, &info.file_path).await?;
    if bytes.len() > MAX_VOICE_BYTES {
        return Err(Error::Transcription(format!(
            "voice file is {} bytes; cap is {MAX_VOICE_BYTES}",
            bytes.len()
        )));
    }

    let filename = derive_filename(&info.file_path, mime_type);
    transcription.transcribe(bytes, &filename).await
}

async fn get_file(
    http: &Client,
    base_url: &str,
    token: &SecretString,
    file_id: &str,
) -> Result<FileInfo> {
    let url = format!("{base_url}/bot{}/getFile", token.expose_secret());
    let resp: Response<FileInfo> = http
        .post(&url)
        .json(&json!({ "file_id": file_id }))
        .send()
        .await?
        .json()
        .await?;
    if !resp.ok {
        return Err(Error::Telegram(format!(
            "getFile: {}",
            resp.description.unwrap_or_else(|| "unknown error".into())
        )));
    }
    resp.result
        .ok_or_else(|| Error::Telegram("getFile returned no result".into()))
}

async fn download(
    http: &Client,
    base_url: &str,
    token: &SecretString,
    file_path: &str,
) -> Result<Bytes> {
    let url = format!("{base_url}/file/bot{}/{file_path}", token.expose_secret());
    let resp = http.get(&url).send().await?;
    let status = resp.status();
    if !status.is_success() {
        return Err(Error::Telegram(format!(
            "file download returned {status}"
        )));
    }
    Ok(resp.bytes().await?)
}

fn derive_filename(file_path: &str, mime_type: Option<&str>) -> String {
    // Telegram's `file_path` is something like `voice/file_42.oga` — keep the
    // basename so providers can detect the format from the extension.
    let basename = file_path
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or("voice");
    if basename.contains('.') {
        return basename.to_string();
    }
    let ext = match mime_type {
        Some("audio/ogg") => "oga",
        Some("audio/mpeg") => "mp3",
        Some("audio/mp4") => "m4a",
        Some("audio/wav" | "audio/x-wav") => "wav",
        Some("audio/webm") => "webm",
        _ => "ogg",
    };
    format!("{basename}.{ext}")
}

#[cfg(test)]
mod tests {
    use super::derive_filename;

    #[test]
    fn keeps_telegram_basename_with_extension() {
        assert_eq!(derive_filename("voice/file_42.oga", None), "file_42.oga");
    }

    #[test]
    fn falls_back_to_mime_extension() {
        assert_eq!(
            derive_filename("voice/file_42", Some("audio/ogg")),
            "file_42.oga"
        );
        assert_eq!(
            derive_filename("voice/file_42", Some("audio/mpeg")),
            "file_42.mp3"
        );
    }

    #[test]
    fn unknown_mime_defaults_to_ogg() {
        assert_eq!(derive_filename("voice/file_42", None), "file_42.ogg");
    }
}
