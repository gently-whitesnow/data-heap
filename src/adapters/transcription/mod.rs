//! Transcription adapters implementing the
//! [`Transcription`](crate::domain::ports::Transcription) port.
//!
//! Provider-neutral entry point: [`build`] takes the per-source config and
//! returns either a ready adapter or `None` when the source disables voice
//! handling (`transcription_provider = "none"`). The Telegram adapter holds
//! `Option<Arc<dyn Transcription>>` and short-circuits on `None`.

mod hosted;
mod local_whisper;

use std::sync::Arc;

use crate::domain::error::{Error, Result};
use crate::domain::ports::Transcription;
use crate::domain::source::TranscriptionProvider;

pub use hosted::{Hosted, MISTRAL_DEFAULT_MODEL, MISTRAL_DEFAULT_URL, OPENAI_DEFAULT_MODEL, OPENAI_DEFAULT_URL};
pub use local_whisper::LocalWhisper;

/// Build a transcription adapter from per-source config fields. Returns `None`
/// for the `none` provider — the Telegram source will then reply in chat that
/// voice is disabled. The caller has already validated that required fields
/// (token for hosted providers, url for local_whisper) are present; this
/// function still surfaces a clear error if they slipped through.
pub fn build(
    provider: TranscriptionProvider,
    token: Option<&str>,
    url: Option<&str>,
) -> Result<Option<Arc<dyn Transcription>>> {
    match provider {
        TranscriptionProvider::None => Ok(None),
        TranscriptionProvider::Mistral => {
            let token = require_token(token, "mistral")?;
            Ok(Some(Arc::new(Hosted::mistral(
                token,
                url.unwrap_or(MISTRAL_DEFAULT_URL),
            )?)))
        }
        TranscriptionProvider::Openai => {
            let token = require_token(token, "openai")?;
            Ok(Some(Arc::new(Hosted::openai(
                token,
                url.unwrap_or(OPENAI_DEFAULT_URL),
            )?)))
        }
        TranscriptionProvider::LocalWhisper => {
            let url = url.filter(|s| !s.trim().is_empty()).ok_or_else(|| {
                Error::Config("local_whisper requires transcription_url".into())
            })?;
            Ok(Some(Arc::new(LocalWhisper::new(url)?)))
        }
    }
}

fn require_token<'a>(token: Option<&'a str>, name: &str) -> Result<&'a str> {
    token
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .ok_or_else(|| Error::Config(format!("{name} provider requires transcription_token")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expect_config_err(provider: TranscriptionProvider, token: Option<&str>, url: Option<&str>) {
        match build(provider, token, url) {
            Err(Error::Config(_)) => {}
            Err(other) => panic!("expected Config error, got {other:?}"),
            Ok(_) => panic!("expected Config error, got Ok"),
        }
    }

    fn expect_some(provider: TranscriptionProvider, token: Option<&str>, url: Option<&str>) {
        match build(provider, token, url) {
            Ok(Some(_)) => {}
            Ok(None) => panic!("expected Some adapter, got None"),
            Err(e) => panic!("expected adapter, got error {e}"),
        }
    }

    #[test]
    fn none_provider_builds_no_adapter() {
        let got = build(TranscriptionProvider::None, None, None);
        assert!(matches!(got, Ok(None)));
    }

    #[test]
    fn mistral_needs_token() {
        expect_config_err(TranscriptionProvider::Mistral, None, None);
    }

    #[test]
    fn openai_needs_token() {
        expect_config_err(TranscriptionProvider::Openai, Some("   "), None);
    }

    #[test]
    fn local_whisper_needs_url() {
        expect_config_err(TranscriptionProvider::LocalWhisper, None, None);
    }

    #[test]
    fn hosted_providers_build_with_token() {
        expect_some(TranscriptionProvider::Mistral, Some("k"), None);
        expect_some(TranscriptionProvider::Openai, Some("k"), None);
    }

    #[test]
    fn local_whisper_builds_with_url() {
        expect_some(
            TranscriptionProvider::LocalWhisper,
            None,
            Some("http://127.0.0.1:9000"),
        );
    }
}
