//! Transcription adapters implementing the
//! [`Transcription`](crate::domain::ports::Transcription) port.
//!
//! Provider-neutral entry point: [`build`] takes the per-source provider and
//! token, and returns either a ready adapter or `None` when the source
//! disables voice handling (`transcription_provider = "none"`). The Telegram
//! adapter holds `Option<Arc<dyn Transcription>>` and short-circuits on
//! `None`.

mod hosted;

use std::sync::Arc;

use crate::domain::error::{Error, Result};
use crate::domain::ports::Transcription;
use crate::domain::source::TranscriptionProvider;

pub use hosted::Hosted;

/// Build a transcription adapter from per-source config. Returns `None` for
/// the `none` provider — the Telegram source will then reply in chat that
/// voice is disabled. The caller has already validated the token; this
/// function still surfaces a clear error if it slipped through.
pub fn build(
    provider: TranscriptionProvider,
    token: Option<&str>,
) -> Result<Option<Arc<dyn Transcription>>> {
    match provider {
        TranscriptionProvider::None => Ok(None),
        TranscriptionProvider::Mistral => Ok(Some(Arc::new(Hosted::mistral(require_token(
            token, "mistral",
        )?)?))),
        TranscriptionProvider::Openai => Ok(Some(Arc::new(Hosted::openai(require_token(
            token, "openai",
        )?)?))),
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

    fn expect_config_err(provider: TranscriptionProvider, token: Option<&str>) {
        match build(provider, token) {
            Err(Error::Config(_)) => {}
            Err(other) => panic!("expected Config error, got {other:?}"),
            Ok(_) => panic!("expected Config error, got Ok"),
        }
    }

    fn expect_some(provider: TranscriptionProvider, token: Option<&str>) {
        match build(provider, token) {
            Ok(Some(_)) => {}
            Ok(None) => panic!("expected Some adapter, got None"),
            Err(e) => panic!("expected adapter, got error {e}"),
        }
    }

    #[test]
    fn none_provider_builds_no_adapter() {
        let got = build(TranscriptionProvider::None, None);
        assert!(matches!(got, Ok(None)));
    }

    #[test]
    fn mistral_needs_token() {
        expect_config_err(TranscriptionProvider::Mistral, None);
    }

    #[test]
    fn openai_needs_token() {
        expect_config_err(TranscriptionProvider::Openai, Some("   "));
    }

    #[test]
    fn hosted_providers_build_with_token() {
        expect_some(TranscriptionProvider::Mistral, Some("k"));
        expect_some(TranscriptionProvider::Openai, Some("k"));
    }
}
