-- ============================================================
-- 005_quote_revisions.sql — Phase 5.1: Document Revision Control System
--
-- route-map-v2.docx Phase 5.1 deliverables: "Revision snapshot engine
-- (quote_revisions table) and UI suffix tracking (e.g., QN-EH/211/2026-Rev.01 to
-- Rev.02)." Completion Check: "Modifying a quote creates an audit-logged revision
-- history while locking prior approved versions."
--
-- Design decision, flagged explicitly rather than silently picked: "UI suffix
-- tracking" (Rev.01 -> Rev.02) already works today, unchanged — quotes.rs's save_quote
-- has upserted on (tenant_id, offer_ref, rev_suffix) since Phase R.3, and a different
-- rev_suffix under the same offer_ref has always created a new row (see quotes.rs's
-- own module doc). What was genuinely missing, and is what this migration + the
-- quotes.rs changes alongside it add, is the OTHER half of the Completion Check:
-- nothing previously stopped an already-approved quote from being silently
-- overwritten in place. quote_revisions is where the pre-overwrite state gets
-- snapshotted, and quotes.rs::save_quote now refuses to modify a row whose status is
-- in LOCKED_STATUSES at all — the only way to change a locked quote further is
-- quotes.rs::branch_new_revision, which snapshots the locked row into
-- quote_revisions and creates a new Rev.NN row to continue editing.
-- ============================================================

-- ============================================================
-- SECTION A — ENGINE-NEUTRAL TABLE DDL (run on both targets)
-- ============================================================

CREATE TABLE IF NOT EXISTS quote_revisions (
    id VARCHAR(36) PRIMARY KEY,
    tenant_id VARCHAR(36) NOT NULL,
    quote_id VARCHAR(36) NOT NULL, -- the quotes.id this snapshot was taken FROM
    revision_number INT NOT NULL, -- 1, 2, 3... sequential per quote_id, not globally
    -- Full serialized quotes.rs::QuoteRecord (including line_items) at the moment of
    -- locking/branching — a real point-in-time audit copy, not a live reference that
    -- could drift if the source row is later deleted or (were locking not enforced)
    -- edited.
    snapshot_json TEXT NOT NULL,
    status_at_snapshot VARCHAR(50) NOT NULL,
    created_by VARCHAR(36) NOT NULL, -- user_id who triggered the snapshot
    created_at VARCHAR(30) NOT NULL,
    FOREIGN KEY (tenant_id) REFERENCES tenants(id) ON DELETE CASCADE,
    FOREIGN KEY (quote_id, tenant_id) REFERENCES quotes(id, tenant_id) ON DELETE CASCADE,
    FOREIGN KEY (created_by) REFERENCES users(id) ON DELETE RESTRICT,
    UNIQUE(quote_id, revision_number)
);
CREATE INDEX IF NOT EXISTS idx_quote_revisions_quote ON quote_revisions(tenant_id, quote_id);

-- ============================================================
-- SECTION B — SQLITE-ONLY: (no triggers needed; quote_revisions rows are
-- write-once/append-only by construction — created only by
-- quotes::branch_new_revision, never updated — so no updated_at trigger applies here,
-- unlike every other table in this project.)
-- ============================================================

-- ============================================================
-- SECTION C — POSTGRESQL-ONLY
-- Not yet executed against a live server (same open item already logged against every
-- prior migration in this project).
-- ============================================================
