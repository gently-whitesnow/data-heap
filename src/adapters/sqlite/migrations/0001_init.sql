-- Initial schema for the data-heap storage adapter.

CREATE TABLE sources (
    slug                   TEXT PRIMARY KEY,
    space                  TEXT NOT NULL,
    transcription_provider TEXT NOT NULL DEFAULT 'none',
    created_at             INTEGER NOT NULL
);

CREATE TABLE items (
    id                 INTEGER PRIMARY KEY AUTOINCREMENT,
    source             TEXT NOT NULL,
    space              TEXT NOT NULL,
    kind               TEXT NOT NULL,
    text               TEXT NOT NULL,
    telegram_metadata  TEXT NOT NULL, -- JSON blob, see domain::item::TelegramMetadata
    created_at         INTEGER NOT NULL
);

-- Hot path: fetch oldest-first items in a space.
CREATE INDEX idx_items_space_id ON items (space, id);

-- Per-(agent, item) processing flag. Independent across agents; presence of a
-- row means "processed". ON DELETE CASCADE keeps marks consistent if an item
-- is ever removed.
CREATE TABLE processed_marks (
    agent_slug   TEXT NOT NULL,
    item_id      INTEGER NOT NULL REFERENCES items (id) ON DELETE CASCADE,
    processed_at INTEGER NOT NULL,
    PRIMARY KEY (agent_slug, item_id)
);

CREATE INDEX idx_processed_marks_item ON processed_marks (item_id);
