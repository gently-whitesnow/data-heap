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

#[tokio::test]
async fn migrate_is_idempotent() {
    let storage = SqliteStorage::open_in_memory().await.unwrap();
    storage.migrate().await.unwrap();
    storage.migrate().await.unwrap();
}

#[tokio::test]
async fn upsert_and_list_sources() {
    let storage = SqliteStorage::open_in_memory().await.unwrap();
    let mut source = Source {
        slug: "expenses-bot".into(),
        space: Space::new("expenses"),
    };
    storage.upsert_source(source.clone()).await.unwrap();

    source.space = Space::new("inbox");
    storage.upsert_source(source).await.unwrap();

    let sources = storage.list_sources().await.unwrap();
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].space.as_str(), "inbox");
}

#[tokio::test]
async fn insert_item_dedups_on_telegram_message() {
    let storage = SqliteStorage::open_in_memory().await.unwrap();
    let inbox = Space::new("inbox");
    let id1 = storage
        .insert_item(sample_item("inbox", "first", 7))
        .await
        .unwrap();
    let id2 = storage
        .insert_item(sample_item("inbox", "retry", 7))
        .await
        .unwrap();
    assert_eq!(id1, id2);

    let stored = storage.get_item(id1).await.unwrap().expect("item exists");
    assert_eq!(stored.text, "first", "duplicate insert is a no-op");
    assert_eq!(
        storage.fetch_unprocessed("a", &inbox, 10).await.unwrap().len(),
        1
    );
}

#[tokio::test]
async fn insert_and_get_item_roundtrips() {
    let storage = SqliteStorage::open_in_memory().await.unwrap();
    let id = storage
        .insert_item(sample_item("thoughts", "hello", 1))
        .await
        .unwrap();

    let fetched = storage.get_item(id).await.unwrap().expect("item exists");
    assert_eq!(fetched.id, id);
    assert_eq!(fetched.text, "hello");
    assert_eq!(fetched.kind, ItemKind::Text);
    assert_eq!(fetched.telegram.username.as_deref(), Some("alice"));
    assert!(fetched.created_at > 0);
}

#[tokio::test]
async fn get_missing_item_returns_none() {
    let storage = SqliteStorage::open_in_memory().await.unwrap();
    assert!(storage.get_item(ItemId(999)).await.unwrap().is_none());
}

#[tokio::test]
async fn fetch_unprocessed_respects_space_and_marks() {
    let storage = SqliteStorage::open_in_memory().await.unwrap();
    let expenses = Space::new("expenses");
    let id1 = storage
        .insert_item(sample_item("expenses", "first", 1))
        .await
        .unwrap();
    let id2 = storage
        .insert_item(sample_item("expenses", "second", 2))
        .await
        .unwrap();
    storage
        .insert_item(sample_item("thoughts", "other", 3))
        .await
        .unwrap();

    let pending = storage
        .fetch_unprocessed("hermes", &expenses, 10)
        .await
        .unwrap();
    assert_eq!(pending.len(), 2);
    assert_eq!(pending[0].id, id1, "oldest first");
    assert_eq!(pending[1].id, id2);

    storage.mark_processed("hermes", id1).await.unwrap();
    let pending = storage
        .fetch_unprocessed("hermes", &expenses, 10)
        .await
        .unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].id, id2);

    let other = storage
        .fetch_unprocessed("openclaw", &expenses, 10)
        .await
        .unwrap();
    assert_eq!(other.len(), 2);
}

#[tokio::test]
async fn mark_processed_is_idempotent() {
    let storage = SqliteStorage::open_in_memory().await.unwrap();
    let inbox = Space::new("inbox");
    let id = storage
        .insert_item(sample_item("inbox", "x", 1))
        .await
        .unwrap();
    storage.mark_processed("hermes", id).await.unwrap();
    storage.mark_processed("hermes", id).await.unwrap();
    assert!(storage
        .fetch_unprocessed("hermes", &inbox, 10)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn delete_item_hides_from_get_and_fetch() {
    let storage = SqliteStorage::open_in_memory().await.unwrap();
    let inbox = Space::new("inbox");
    let id = storage
        .insert_item(sample_item("inbox", "ephemeral", 1))
        .await
        .unwrap();
    storage.delete_item(id).await.unwrap();

    assert!(
        storage.get_item(id).await.unwrap().is_none(),
        "deleted item is hidden from get_item"
    );
    assert!(
        storage
            .fetch_unprocessed("hermes", &inbox, 10)
            .await
            .unwrap()
            .is_empty(),
        "deleted item is hidden from fetch_unprocessed"
    );
}

#[tokio::test]
async fn delete_item_is_idempotent_and_keeps_processed_marks() {
    let storage = SqliteStorage::open_in_memory().await.unwrap();
    let id = storage
        .insert_item(sample_item("inbox", "x", 1))
        .await
        .unwrap();
    storage.mark_processed("hermes", id).await.unwrap();

    storage.delete_item(id).await.unwrap();
    storage.delete_item(id).await.unwrap();
    // Re-marking a processed-then-deleted item must not violate the FK
    // (the item row survives) and must remain a no-op.
    storage.mark_processed("hermes", id).await.unwrap();
}

#[tokio::test]
async fn delete_unknown_item_is_no_op() {
    let storage = SqliteStorage::open_in_memory().await.unwrap();
    storage.delete_item(ItemId(9999)).await.unwrap();
}

#[tokio::test]
async fn delete_then_reinsert_same_telegram_message_keeps_tombstone() {
    let storage = SqliteStorage::open_in_memory().await.unwrap();
    let id = storage
        .insert_item(sample_item("inbox", "first", 7))
        .await
        .unwrap();
    storage.delete_item(id).await.unwrap();

    // A re-delivered update for the same (chat_id, message_id) must stay
    // deduplicated against the tombstoned row so a deleted item does not
    // come back to life on the next polling pass.
    let id2 = storage
        .insert_item(sample_item("inbox", "retry", 7))
        .await
        .unwrap();
    assert_eq!(id, id2, "dedup still returns the existing (deleted) id");
    assert!(storage.get_item(id).await.unwrap().is_none());
}

#[tokio::test]
async fn fetch_respects_limit() {
    let storage = SqliteStorage::open_in_memory().await.unwrap();
    let inbox = Space::new("inbox");
    for i in 0..5 {
        storage
            .insert_item(sample_item("inbox", "x", i))
            .await
            .unwrap();
    }
    let pending = storage
        .fetch_unprocessed("hermes", &inbox, 2)
        .await
        .unwrap();
    assert_eq!(pending.len(), 2);
}

#[tokio::test]
async fn insert_items_batch_is_transactional() {
    let storage = SqliteStorage::open_in_memory().await.unwrap();
    let inbox = Space::new("inbox");
    let batch = vec![
        sample_item("inbox", "a", 1),
        sample_item("inbox", "b", 2),
        sample_item("inbox", "c", 3),
    ];
    let ids = storage.insert_items(batch).await.unwrap();
    assert_eq!(ids.len(), 3);
    let pending = storage
        .fetch_unprocessed("hermes", &inbox, 10)
        .await
        .unwrap();
    assert_eq!(pending.len(), 3);
}
