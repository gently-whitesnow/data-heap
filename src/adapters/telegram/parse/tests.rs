use serde_json::json;

use super::super::api::Message;
use super::*;
use crate::domain::ports::IncomingPayload;

fn msg(value: serde_json::Value) -> Message {
    serde_json::from_value(value).expect("valid message payload")
}

fn base_msg(extra: serde_json::Value) -> Message {
    let mut body = json!({
        "message_id": 10,
        "date": 1_700_000_000,
        "chat": { "id": 42 },
        "from": { "id": 7, "username": "alice" }
    });
    let body_obj = body.as_object_mut().unwrap();
    for (k, v) in extra.as_object().unwrap() {
        body_obj.insert(k.clone(), v.clone());
    }
    msg(body)
}

#[test]
fn text_message_emits_incoming_text() {
    let parsed = parse(&base_msg(json!({ "text": "hello" })));
    let Parsed::Incoming(im) = parsed else {
        panic!("expected Incoming, got {:?}", parsed)
    };
    assert_eq!(im.payload, IncomingPayload::Text("hello".into()));
    assert_eq!(im.chat_id, 42);
    assert_eq!(im.message_id, 10);
    assert_eq!(im.user_id, Some(7));
    assert_eq!(im.username.as_deref(), Some("alice"));
}

#[test]
fn photo_with_caption_emits_incoming_caption() {
    let parsed = parse(&base_msg(json!({
        "photo": [{ "file_id": "x" }],
        "caption": "lunch"
    })));
    assert!(matches!(
        parsed,
        Parsed::Incoming(im) if im.payload == IncomingPayload::Caption("lunch".into())
    ));
}

#[test]
fn photo_without_caption_is_unsupported_binary() {
    let parsed = parse(&base_msg(json!({ "photo": [{ "file_id": "x" }] })));
    assert_eq!(parsed, Parsed::Unsupported(UNSUPPORTED_BINARY));
}

#[test]
fn document_without_caption_is_unsupported_binary() {
    let parsed = parse(&base_msg(json!({ "document": { "file_id": "x" } })));
    assert_eq!(parsed, Parsed::Unsupported(UNSUPPORTED_BINARY));
}

#[test]
fn voice_is_unsupported_until_slice_three() {
    let parsed = parse(&base_msg(json!({ "voice": { "file_id": "x" } })));
    assert_eq!(parsed, Parsed::Unsupported(UNSUPPORTED_VOICE));
}

#[test]
fn sticker_is_unsupported_binary() {
    let parsed = parse(&base_msg(json!({ "sticker": { "file_id": "x" } })));
    assert_eq!(parsed, Parsed::Unsupported(UNSUPPORTED_BINARY));
}

#[test]
fn empty_text_is_unsupported_other() {
    let parsed = parse(&base_msg(json!({ "text": "   " })));
    assert_eq!(parsed, Parsed::Unsupported(UNSUPPORTED_OTHER));
}

#[test]
fn unknown_message_kind_is_unsupported_other() {
    let parsed = parse(&base_msg(json!({})));
    assert_eq!(parsed, Parsed::Unsupported(UNSUPPORTED_OTHER));
}

#[test]
fn link_text_payload_remains_text_at_parse_layer() {
    // Link/Text split is a domain rule applied later; the parser only flags
    // the payload as plain text.
    let parsed = parse(&base_msg(json!({ "text": "https://example.com" })));
    let Parsed::Incoming(im) = parsed else {
        panic!()
    };
    assert!(matches!(im.payload, IncomingPayload::Text(_)));
}
