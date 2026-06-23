-- Initial schema for the data-heap storage adapter.

CREATE TABLE sources (
    slug  TEXT PRIMARY KEY,
    space TEXT NOT NULL
);

CREATE TABLE items (
    id             INTEGER PRIMARY KEY, -- rowid; append-only, so monotonic
    source         TEXT NOT NULL,
    space          TEXT NOT NULL,
    kind           TEXT NOT NULL,
    text           TEXT NOT NULL,
    -- (chat_id, message_id) is the Telegram message address; promoted out of the
    -- JSON blob so ingestion can dedup repeated polling updates via the index.
    chat_id        INTEGER NOT NULL,
    message_id     INTEGER NOT NULL,
    telegram_extra TEXT NOT NULL, -- JSON: remaining metadata (user, date, …)
    created_at     INTEGER NOT NULL
);

-- Hot path: fetch oldest-first items in a space.
CREATE INDEX idx_items_space_id ON items (space, id);

-- Dedup key for ingestion (slice 2): one row per Telegram message.
CREATE UNIQUE INDEX uq_items_telegram_msg ON items (chat_id, message_id);

-- Per-(agent, item) processing flag. Independent across agents; presence of a
-- row means "processed". FK guards against marking a non-existent item.
CREATE TABLE processed_marks (
    agent_slug   TEXT NOT NULL,
    item_id      INTEGER NOT NULL REFERENCES items (id),
    processed_at INTEGER NOT NULL,
    PRIMARY KEY (agent_slug, item_id)
);
