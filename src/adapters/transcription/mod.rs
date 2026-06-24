//! Transcription adapters implementing the
//! [`Transcription`](crate::domain::ports::Transcription) port.
//!
//! Provider-neutral entry point: [`build`] takes a shared HTTP client, the
//! per-source provider and token, and returns either a ready adapter or
//! `None` when the source disables voice handling
//! (`transcription_provider = "none"`).

mod hosted;

use std::sync::Arc;

use reqwest::Client;
use secrecy::{ExposeSecret, SecretString};

use crate::domain::error::{Error, Result};
use crate::domain::ports::Transcription;
use crate::domain::source::TranscriptionProvider;

pub use hosted::Hosted;

/// Build a transcription adapter from per-source config. Returns `None` for
/// the `none` provider — the Telegram source will then reply in chat that
/// voice is disabled. The caller has already validated the token; this
/// function still surfaces a clear error if it slipped through.
pub fn build(
    http: &Client,
    provider: TranscriptionProvider,
    token: Option<&SecretString>,
) -> Result<Option<Arc<dyn Transcription>>> {
    match provider {
        TranscriptionProvider::None => Ok(None),
        TranscriptionProvider::Mistral => {
            let token = require_token(token, "mistral")?;
            Ok(Some(Arc::new(Hosted::mistral(http.clone(), token))))
        }
        TranscriptionProvider::Openai => {
            let token = require_token(token, "openai")?;
            Ok(Some(Arc::new(Hosted::openai(http.clone(), token))))
        }
    }
}

fn require_token(token: Option<&SecretString>, name: &str) -> Result<SecretString> {
    match token {
        Some(t) if !t.expose_secret().trim().is_empty() => {
            Ok(SecretString::from(t.expose_secret().to_owned()))
        }
        _ => Err(Error::Config(format!(
            "{name} provider requires transcription_token"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn client() -> Client {
        Client::new()
    }

    fn expect_config_err(provider: TranscriptionProvider, token: Option<SecretString>) {
        match build(&client(), provider, token.as_ref()) {
            Err(Error::Config(_)) => {}
            Err(other) => panic!("expected Config error, got {other:?}"),
            Ok(_) => panic!("expected Config error, got Ok"),
        }
    }

    fn expect_some(provider: TranscriptionProvider, token: Option<SecretString>) {
        match build(&client(), provider, token.as_ref()) {
            Ok(Some(_)) => {}
            Ok(None) => panic!("expected Some adapter, got None"),
            Err(e) => panic!("expected adapter, got error {e}"),
        }
    }

    #[test]
    fn none_provider_builds_no_adapter() {
        let got = build(&client(), TranscriptionProvider::None, None);
        assert!(matches!(got, Ok(None)));
    }

    #[test]
    fn mistral_needs_token() {
        expect_config_err(TranscriptionProvider::Mistral, None);
    }

    #[test]
    fn openai_needs_token() {
        expect_config_err(
            TranscriptionProvider::Openai,
            Some(SecretString::from("   ".to_string())),
        );
    }

    #[test]
    fn hosted_providers_build_with_token() {
        expect_some(
            TranscriptionProvider::Mistral,
            Some(SecretString::from("k".to_string())),
        );
        expect_some(
            TranscriptionProvider::Openai,
            Some(SecretString::from("k".to_string())),
        );
    }
}
