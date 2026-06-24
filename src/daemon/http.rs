//! HTTP API for consumer agents (claw / openclaw / hermes / …). Localhost-only,
//! no auth in MVP — the daemon and its agents live on the same machine.
//!
//! Two endpoints over the [`Storage`] port:
//!
//! - `GET  /v1/items?agent_slug=…&space=…&limit=…` — items in `space` that
//!   `agent_slug` has not yet marked processed, oldest first.
//! - `POST /v1/items/processed` body `{"agent_slug": "...", "item_id": N}` —
//!   flip the per-(agent, item) processed flag. Idempotent and independent
//!   across agents.

use std::sync::Arc;

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::domain::error::Error;
use crate::domain::item::{Item, ItemId, ItemKind, TelegramMetadata};
use crate::domain::ports::Storage;

const DEFAULT_LIMIT: u32 = 50;
const MAX_LIMIT: u32 = 500;

/// Bind to `config.daemon.http_addr` and serve until the daemon shuts down.
/// A bind failure is logged and the task idles — the polling loop keeps
/// running so we don't lose ingestion just because the port is busy.
pub async fn run(config: Config, storage: Arc<dyn Storage>) {
    let addr = config.daemon.http_addr.clone();
    let listener = match tokio::net::TcpListener::bind(&addr).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!(addr = %addr, error = %e, "failed to bind HTTP API");
            std::future::pending::<()>().await;
            return;
        }
    };
    tracing::info!(addr = %addr, "HTTP API listening");
    if let Err(e) = axum::serve(listener, router(storage)).await {
        tracing::error!(error = %e, "HTTP server stopped");
    }
}

fn router(storage: Arc<dyn Storage>) -> Router {
    Router::new()
        .route("/v1/items", get(list_items))
        .route("/v1/items/processed", post(mark_processed))
        .with_state(storage)
}

#[derive(Debug, Deserialize)]
struct ListItemsQuery {
    agent_slug: String,
    space: String,
    #[serde(default)]
    limit: Option<u32>,
}

#[derive(Debug, Serialize)]
struct ItemDto {
    id: ItemId,
    source: String,
    space: String,
    kind: ItemKind,
    text: String,
    telegram: TelegramMetadata,
    created_at: i64,
}

impl From<Item> for ItemDto {
    fn from(item: Item) -> Self {
        ItemDto {
            id: item.id,
            source: item.source,
            space: item.space.to_string(),
            kind: item.kind,
            text: item.text,
            telegram: item.telegram,
            created_at: item.created_at,
        }
    }
}

async fn list_items(
    State(storage): State<Arc<dyn Storage>>,
    Query(q): Query<ListItemsQuery>,
) -> Result<Json<Vec<ItemDto>>, ApiError> {
    let agent_slug = require_field(&q.agent_slug, "agent_slug")?;
    let space = require_field(&q.space, "space")?;
    let limit = q.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let items = storage
        .fetch_unprocessed(agent_slug, space, limit)
        .map_err(ApiError::storage)?;
    Ok(Json(items.into_iter().map(ItemDto::from).collect()))
}

#[derive(Debug, Deserialize)]
struct MarkProcessedBody {
    agent_slug: String,
    item_id: ItemId,
}

async fn mark_processed(
    State(storage): State<Arc<dyn Storage>>,
    Json(body): Json<MarkProcessedBody>,
) -> Result<StatusCode, ApiError> {
    let agent_slug = require_field(&body.agent_slug, "agent_slug")?;
    storage
        .mark_processed(agent_slug, body.item_id)
        .map_err(ApiError::storage)?;
    Ok(StatusCode::NO_CONTENT)
}

fn require_field<'a>(value: &'a str, field: &str) -> Result<&'a str, ApiError> {
    let v = value.trim();
    if v.is_empty() {
        return Err(ApiError::bad_request(format!(
            "'{field}' must not be empty"
        )));
    }
    Ok(v)
}

#[derive(Debug, Serialize)]
struct ApiError {
    #[serde(skip)]
    status: StatusCode,
    error: String,
}

impl ApiError {
    fn bad_request(msg: impl Into<String>) -> Self {
        ApiError {
            status: StatusCode::BAD_REQUEST,
            error: msg.into(),
        }
    }

    fn storage(e: Error) -> Self {
        tracing::error!(error = %e, "storage error in HTTP handler");
        ApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            error: e.to_string(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = self.status;
        (status, Json(self)).into_response()
    }
}

#[cfg(test)]
mod tests;
