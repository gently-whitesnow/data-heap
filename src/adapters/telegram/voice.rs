//! Voice flow: Telegram `getFile` → download bytes → write to a temp file →
//! transcribe → temp file is deleted on drop (success and error paths alike).
//!
//! The temp file exists for two reasons: it gives the OS a single resource
//! handle to clean up if the process is killed mid-transcription, and it makes
//! the "downloaded then deleted" guarantee in the spec a concrete artifact
//! rather than only an in-memory promise. The bytes are still passed to the
//! [`Transcription`] port directly to avoid forcing every adapter through
//! disk I/O.

use std::io::Write;

use reqwest::Client;
use serde_json::json;
use tempfile::NamedTempFile;

use crate::domain::error::{Error, Result};
use crate::domain::ports::Transcription;

use super::api::{FileInfo, Response};

/// Telegram caps `getFile` downloads at 20 MB. Reject larger payloads early so
/// we don't waste time downloading something a hosted provider will refuse.
const MAX_VOICE_BYTES: usize = 20 * 1024 * 1024;

pub async fn transcribe_voice(
    http: &Client,
    base_url: &str,
    token: &str,
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

    // Materialize the download on disk under a tempfile guard. The handle
    // drops at the end of this function — both on Ok and on the `?` early
    // returns from `transcribe`.
    let tempfile =
        NamedTempFile::new().map_err(|e| Error::Transcription(format!("temp file: {e}")))?;
    tempfile
        .as_file()
        .write_all(&bytes)
        .map_err(|e| Error::Transcription(format!("write temp file: {e}")))?;

    let filename = derive_filename(&info.file_path, mime_type);
    let transcript = transcription.transcribe(&bytes, &filename).await?;
    // `tempfile` drops here, removing the file — explicit close also works
    // but RAII is enough.
    Ok(transcript)
}

async fn get_file(http: &Client, base_url: &str, token: &str, file_id: &str) -> Result<FileInfo> {
    let url = format!("{base_url}/bot{token}/getFile");
    let resp: Response<FileInfo> = http
        .post(&url)
        .json(&json!({ "file_id": file_id }))
        .send()
        .await
        .map_err(|e| Error::Transcription(format!("telegram getFile: {e}")))?
        .json()
        .await
        .map_err(|e| Error::Transcription(format!("telegram getFile decode: {e}")))?;
    if !resp.ok {
        return Err(Error::Transcription(format!(
            "telegram getFile: {}",
            resp.description.unwrap_or_else(|| "unknown error".into())
        )));
    }
    resp.result
        .ok_or_else(|| Error::Transcription("telegram getFile returned no result".into()))
}

async fn download(http: &Client, base_url: &str, token: &str, file_path: &str) -> Result<Vec<u8>> {
    let url = format!("{base_url}/file/bot{token}/{file_path}");
    let resp = http
        .get(&url)
        .send()
        .await
        .map_err(|e| Error::Transcription(format!("telegram file download: {e}")))?;
    let status = resp.status();
    if !status.is_success() {
        return Err(Error::Transcription(format!(
            "telegram file download returned {status}"
        )));
    }
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| Error::Transcription(format!("telegram file body: {e}")))?;
    Ok(bytes.to_vec())
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
