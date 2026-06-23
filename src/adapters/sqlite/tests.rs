use super::*;
use crate::domain::item::{ItemKind, NewItem, TelegramMetadata};
use crate::domain::source::{Source, Space};

fn sample_item(space: &str, text: &str, message_id: i64) -> NewItem {
    NewItem {
        source: "expenses-bot".into(),
        space: Space::new(space),
        kind: ItemKind::Text,
        text: text.into(),
        telegram: TelegramMetadata {
            chat_id: 42,
            message_id,
            user_id: Some(7),
            username: Some("alice".into()),
            date: 1_700_000_000,
        },
    }
}

#[test]
fn migrate_is_idempotent() {
    let storage = SqliteStorage::open_in_memory().unwrap();
    storage.migrate().unwrap();
    storage.migrate().unwrap();
}

#[test]
fn upsert_and_list_sources() {
    let storage = SqliteStorage::open_in_memory().unwrap();
    let mut source = Source {
        slug: "expenses-bot".into(),
        space: Space::new("expenses"),
    };
    storage.upsert_source(&source).unwrap();

    source.space = Space::new("inbox");
    storage.upsert_source(&source).unwrap();

    let sources = storage.list_sources().unwrap();
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].space.as_str(), "inbox");
}

#[test]
fn insert_item_dedups_on_telegram_message() {
    let storage = SqliteStorage::open_in_memory().unwrap();
    // Same (chat_id, message_id) => same item, stored once.
    let id1 = storage
        .insert_item(&sample_item("inbox", "first", 7))
        .unwrap();
    let id2 = storage
        .insert_item(&sample_item("inbox", "retry", 7))
        .unwrap();
    assert_eq!(id1, id2);

    let stored = storage.get_item(id1).unwrap().expect("item exists");
    assert_eq!(stored.text, "first", "duplicate insert is a no-op");
    assert_eq!(
        storage.fetch_unprocessed("a", "inbox", 10).unwrap().len(),
        1
    );
}

#[test]
fn insert_and_get_item_roundtrips() {
    let storage = SqliteStorage::open_in_memory().unwrap();
    let id = storage
        .insert_item(&sample_item("thoughts", "hello", 1))
        .unwrap();

    let fetched = storage.get_item(id).unwrap().expect("item exists");
    assert_eq!(fetched.id, id);
    assert_eq!(fetched.text, "hello");
    assert_eq!(fetched.kind, ItemKind::Text);
    assert_eq!(fetched.telegram.username.as_deref(), Some("alice"));
    assert!(fetched.created_at > 0);
}

#[test]
fn get_missing_item_returns_none() {
    let storage = SqliteStorage::open_in_memory().unwrap();
    assert!(storage.get_item(ItemId(999)).unwrap().is_none());
}

#[test]
fn fetch_unprocessed_respects_space_and_marks() {
    let storage = SqliteStorage::open_in_memory().unwrap();
    let id1 = storage
        .insert_item(&sample_item("expenses", "first", 1))
        .unwrap();
    let id2 = storage
        .insert_item(&sample_item("expenses", "second", 2))
        .unwrap();
    storage
        .insert_item(&sample_item("thoughts", "other", 3))
        .unwrap();

    let pending = storage.fetch_unprocessed("hermes", "expenses", 10).unwrap();
    assert_eq!(pending.len(), 2);
    assert_eq!(pending[0].id, id1, "oldest first");
    assert_eq!(pending[1].id, id2);

    storage.mark_processed("hermes", id1).unwrap();
    let pending = storage.fetch_unprocessed("hermes", "expenses", 10).unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].id, id2);

    // Marks are per-agent: another agent still sees everything.
    let other = storage
        .fetch_unprocessed("openclaw", "expenses", 10)
        .unwrap();
    assert_eq!(other.len(), 2);
}

#[test]
fn mark_processed_is_idempotent() {
    let storage = SqliteStorage::open_in_memory().unwrap();
    let id = storage.insert_item(&sample_item("inbox", "x", 1)).unwrap();
    storage.mark_processed("hermes", id).unwrap();
    storage.mark_processed("hermes", id).unwrap();
    assert!(storage
        .fetch_unprocessed("hermes", "inbox", 10)
        .unwrap()
        .is_empty());
}

#[test]
fn fetch_respects_limit() {
    let storage = SqliteStorage::open_in_memory().unwrap();
    for i in 0..5 {
        storage.insert_item(&sample_item("inbox", "x", i)).unwrap();
    }
    let pending = storage.fetch_unprocessed("hermes", "inbox", 2).unwrap();
    assert_eq!(pending.len(), 2);
}
