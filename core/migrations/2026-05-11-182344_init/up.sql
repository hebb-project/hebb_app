-- Initial schema for the cortex graph + spike log.
--
-- Design notes:
--   * `node_type` / `edge_type` are TEXT not ENUM — schema evolution is
--     adding values, not migrations. New kinds of nodes/edges arrive without
--     touching the table.
--   * `metadata` JSONB lets us attach properties (vault frontmatter, layout
--     hints, learned scalars) without a wide-table explosion.
--   * `model_blob_path` points at on-disk per-node state (membrane history,
--     STDP traces). Postgres stores the path; blobs live on the filesystem
--     and can move to S3 / mmap later without a schema change.
--   * spike_log is a rolling log keyed for fast "recent spikes for node X"
--     scans. Truncation / partitioning is a future ops concern, not M0.

CREATE EXTENSION IF NOT EXISTS "pgcrypto";

CREATE TABLE nodes (
    id              UUID         PRIMARY KEY DEFAULT gen_random_uuid(),
    label           TEXT         NOT NULL,
    node_type       TEXT         NOT NULL DEFAULT 'concept',
    source_file     TEXT,
    model_blob_path TEXT,
    metadata        JSONB        NOT NULL DEFAULT '{}'::jsonb,
    created_at      TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);

CREATE UNIQUE INDEX nodes_source_file_idx ON nodes (source_file) WHERE source_file IS NOT NULL;
CREATE INDEX nodes_label_idx ON nodes (label);
CREATE INDEX nodes_node_type_idx ON nodes (node_type);

CREATE TABLE edges (
    id          UUID         PRIMARY KEY DEFAULT gen_random_uuid(),
    pre_id      UUID         NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
    post_id     UUID         NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
    weight      REAL         NOT NULL DEFAULT 0.5,
    edge_type   TEXT         NOT NULL DEFAULT 'association',
    metadata    JSONB        NOT NULL DEFAULT '{}'::jsonb,
    created_at  TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    updated_at  TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    CONSTRAINT edges_no_self_loop CHECK (pre_id <> post_id)
);

CREATE UNIQUE INDEX edges_pre_post_type_idx ON edges (pre_id, post_id, edge_type);
CREATE INDEX edges_post_id_idx ON edges (post_id);

CREATE TABLE spike_log (
    id          BIGSERIAL    PRIMARY KEY,
    node_id     UUID         NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
    t_ms        DOUBLE PRECISION NOT NULL,
    recorded_at TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);

CREATE INDEX spike_log_node_recent_idx ON spike_log (node_id, recorded_at DESC);
CREATE INDEX spike_log_recorded_at_idx ON spike_log (recorded_at DESC);
