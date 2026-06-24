-- Soft-delete column for items. Tombstone instead of hard delete so per-agent
-- processed_marks stay consistent with whatever the slice-4 agents may have
-- already grabbed. Filtering happens at read time in fetch_unprocessed/get_item.

ALTER TABLE items ADD COLUMN deleted_at INTEGER;

CREATE INDEX idx_items_deleted_at ON items (deleted_at);
