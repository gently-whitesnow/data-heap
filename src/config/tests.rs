use super::*;
use crate::domain::source::TranscriptionProvider;

const SAMPLE: &str = r#"
[daemon]
database_path = "/var/lib/data-heap.sqlite"
poll_interval_secs = 3
http_addr = "127.0.0.1:9000"

[[sources]]
slug = "expenses-bot"
space = "expenses"
bot_token = "111:AAA"
transcription_provider = "mistral"
transcription_token = "sk-mistral"

[[sources]]
slug = "thoughts-bot"
space = "thoughts"
bot_token = "222:BBB"
"#;

#[test]
fn parses_full_config() {
    let cfg = Config::from_toml(SAMPLE).expect("valid config");
    assert_eq!(cfg.daemon.poll_interval_secs, 3);
    assert_eq!(cfg.daemon.http_addr, "127.0.0.1:9000");
    assert_eq!(cfg.sources.len(), 2);

    let first = &cfg.sources[0];
    assert_eq!(first.slug, "expenses-bot");
    assert_eq!(first.transcription_provider, TranscriptionProvider::Mistral);

    let second = &cfg.sources[1];
    assert_eq!(second.transcription_provider, TranscriptionProvider::None);
    assert_eq!(second.to_source().space.as_str(), "thoughts");
}

#[test]
fn applies_daemon_defaults() {
    let cfg = Config::from_toml("").expect("empty config is valid");
    assert_eq!(cfg.daemon.poll_interval_secs, 5);
    assert_eq!(cfg.daemon.http_addr, "127.0.0.1:8080");
    assert_eq!(cfg.daemon.database_path, PathBuf::from("data-heap.sqlite"));
    assert!(cfg.sources.is_empty());
}

#[test]
fn rejects_duplicate_slugs() {
    let raw = r#"
[[sources]]
slug = "dup"
space = "inbox"
bot_token = "1:A"

[[sources]]
slug = "dup"
space = "inbox"
bot_token = "2:B"
"#;
    let err = Config::from_toml(raw).unwrap_err();
    assert!(matches!(err, Error::Config(msg) if msg.contains("duplicate")));
}

#[test]
fn rejects_provider_without_token() {
    let raw = r#"
[[sources]]
slug = "voice"
space = "inbox"
bot_token = "1:A"
transcription_provider = "openai"
"#;
    let err = Config::from_toml(raw).unwrap_err();
    assert!(matches!(err, Error::Config(msg) if msg.contains("transcription_token")));
}

#[test]
fn local_whisper_needs_url_not_token() {
    let with_url = r#"
[[sources]]
slug = "voice"
space = "inbox"
bot_token = "1:A"
transcription_provider = "local_whisper"
transcription_url = "http://127.0.0.1:9000"
"#;
    assert!(Config::from_toml(with_url).is_ok());

    let no_url = r#"
[[sources]]
slug = "voice"
space = "inbox"
bot_token = "1:A"
transcription_provider = "local_whisper"
"#;
    let err = Config::from_toml(no_url).unwrap_err();
    assert!(matches!(err, Error::Config(msg) if msg.contains("transcription_url")));
}
