-- 0002_search_docs.sql — FTS rowid 稳定映射（D-05）
CREATE TABLE IF NOT EXISTS search_docs (
  entity_id TEXT PRIMARY KEY,
  rowid     INTEGER NOT NULL UNIQUE
);
