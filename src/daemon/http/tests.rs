use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

use crate::adapters::SqliteStorage;
use crate::domain::item::{ItemId, ItemKind, NewItem, TelegramMetadata};
use crate::domain::ports::Storage;
use crate::domain::source::Space;

use super::router;

async fn spawn() -> (String, Arc<dyn Storage>) {
    let storage: Arc<dyn Storage> = Arc::new(SqliteStorage::open_in_memory().await.unwrap());
    let app = router(storage.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (format!("http://{addr}"), storage)
}

fn new_item(space: &str, message_id: i64, text: &str) -> NewItem {
    NewItem {
        source: "bot".into(),
        space: Space::new(space),
        kind: ItemKind::Text,
        text: text.into(),
        telegram: TelegramMetadata {
            chat_id: 100,
            message_id,
            user_id: None,
            username: None,
            date: 1_700_000_000 + message_id,
        },
    }
}

async fn list(base: &str, agent: &str, space: &str, limit: Option<u32>) -> Vec<Value> {
    use std::fmt::Write;
    let mut url = format!("{base}/v1/items?agent_slug={agent}&space={space}");
    if let Some(l) = limit {
        write!(url, "&limit={l}").unwrap();
    }
    let resp = reqwest::get(&url).await.unwrap();
    assert_eq!(resp.status(), 200, "GET {url} expected 200");
    resp.json::<Vec<Value>>().await.unwrap()
}

async fn mark(base: &str, agent: &str, id: ItemId) -> reqwest::StatusCode {
    let resp = reqwest::Client::new()
        .post(format!("{base}/v1/items/processed"))
        .json(&json!({"agent_slug": agent, "item_id": id.0}))
        .send()
        .await
        .unwrap();
    resp.status()
}

#[tokio::test]
async fn list_returns_empty_when_storage_empty() {
    let (base, _s) = spawn().await;
    let items = list(&base, "hermes", "inbox", None).await;
    assert!(items.is_empty());
}

#[tokio::test]
async fn list_returns_only_items_in_requested_space_oldest_first() {
    let (base, storage) = spawn().await;
    let a = storage
        .insert_item(new_item("inbox", 1, "first"))
        .await
        .unwrap();
    let _ = storage
        .insert_item(new_item("expenses", 2, "wrong space"))
        .await
        .unwrap();
    let b = storage
        .insert_item(new_item("inbox", 3, "second"))
        .await
        .unwrap();

    let items = list(&base, "hermes", "inbox", None).await;
    let ids: Vec<i64> = items.iter().map(|v| v["id"].as_i64().unwrap()).collect();
    assert_eq!(ids, vec![a.0, b.0]);
    assert_eq!(items[0]["text"], "first");
    assert_eq!(items[0]["kind"], "text");
    assert_eq!(items[0]["telegram"]["chat_id"], 100);
    assert_eq!(items[0]["telegram"]["message_id"], 1);
}

#[tokio::test]
async fn mark_processed_hides_item_from_same_agent_only() {
    let (base, storage) = spawn().await;
    let id = storage.insert_item(new_item("inbox", 1, "x")).await.unwrap();

    assert_eq!(mark(&base, "hermes", id).await, 204);

    let for_hermes = list(&base, "hermes", "inbox", None).await;
    assert!(
        for_hermes.is_empty(),
        "hermes should not see processed item"
    );

    let for_openclaw = list(&base, "openclaw", "inbox", None).await;
    assert_eq!(for_openclaw.len(), 1, "other agents are unaffected");
}

#[tokio::test]
async fn mark_processed_is_idempotent() {
    let (base, storage) = spawn().await;
    let id = storage.insert_item(new_item("inbox", 1, "x")).await.unwrap();
    assert_eq!(mark(&base, "hermes", id).await, 204);
    assert_eq!(mark(&base, "hermes", id).await, 204);
}

#[tokio::test]
async fn limit_caps_response_size() {
    let (base, storage) = spawn().await;
    for i in 1..=5 {
        storage
            .insert_item(new_item("inbox", i, "x"))
            .await
            .unwrap();
    }
    let items = list(&base, "hermes", "inbox", Some(2)).await;
    assert_eq!(items.len(), 2);
}

#[tokio::test]
async fn empty_agent_slug_is_bad_request() {
    let (base, _s) = spawn().await;
    let resp = reqwest::get(format!("{base}/v1/items?agent_slug=&space=inbox"))
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);
    let body: Value = resp.json().await.unwrap();
    assert!(
        body["error"].as_str().unwrap().contains("agent_slug"),
        "error mentions field: {body}"
    );
}

#[tokio::test]
async fn missing_query_param_is_bad_request() {
    let (base, _s) = spawn().await;
    let resp = reqwest::get(format!("{base}/v1/items?space=inbox"))
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);
}

#[tokio::test]
async fn soft_deleted_items_are_hidden() {
    let (base, storage) = spawn().await;
    let id = storage.insert_item(new_item("inbox", 1, "x")).await.unwrap();
    storage.delete_item(id).await.unwrap();
    let items = list(&base, "hermes", "inbox", None).await;
    assert!(items.is_empty());
}

#[tokio::test]
async fn graceful_shutdown_drains_in_flight() {
    let storage: Arc<dyn Storage> = Arc::new(SqliteStorage::open_in_memory().await.unwrap());
    let app = router(storage.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let shutdown = CancellationToken::new();
    let signal = {
        let token = shutdown.clone();
        async move { token.cancelled().await }
    };
    let handle = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(signal)
            .await
            .unwrap();
    });

    // Pre-flight: server is reachable.
    let resp = reqwest::get(format!("http://{addr}/v1/items?agent_slug=h&space=inbox"))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    shutdown.cancel();
    let join = tokio::time::timeout(Duration::from_secs(5), handle)
        .await
        .expect("server task must exit promptly");
    join.unwrap();

    // After shutdown, the listener is gone and new requests fail to connect.
    let after = reqwest::Client::new()
        .get(format!("http://{addr}/v1/items?agent_slug=h&space=inbox"))
        .timeout(Duration::from_millis(500))
        .send()
        .await;
    assert!(after.is_err(), "server should no longer accept connections");
}
