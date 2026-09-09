-- ============================================================
-- 003_compliance_terms.sql — Phase 4.1: Terms Library & Regional VAT Schema
-- Compatible with SQLite and PostgreSQL.
--
-- Per route-map-v2.docx Phase 4.1: "Tables compliance_terms and tax_rules. UI
-- manager for editing standard legal disclaimers, warranty conditions, and
-- default liability clauses." Completion Check: "Allows storing, updating,
-- and selecting dynamic legal term templates" — "selecting" is itself data
-- (which specific clauses, in what order, on which quote), not just free
-- text baked into quotes.terms_conditions, so a join table
-- (quote_compliance_terms) is added below to make that selection persistent
-- and auditable, matching quote_items' pattern of owned child rows rather
-- than a denormalized string.
--
-- Cross-tenant isolation: follows 002_asset_directory.sql's established
-- pattern exactly — composite (child_id, tenant_id) -> (parent.id,
-- parent.tenant_id) foreign keys, not a bare parent.id reference. quotes
-- (from 001_core_schema.sql) has no inline UNIQUE(id, tenant_id) to serve as
-- a composite FK's parent side; rather than recreate that table mid-project,
-- a UNIQUE INDEX is added on quotes(id, tenant_id) below — empirically
-- verified (not assumed) that SQLite accepts a UNIQUE INDEX, not only an
-- inline UNIQUE/PRIMARY KEY constraint, as a composite FK's parent key.
-- ============================================================

-- ============================================================
-- SECTION A — ENGINE-NEUTRAL TABLE DDL (run on both targets)
-- ============================================================

-- Required as the parent side of quote_compliance_terms' composite FK below.
-- quotes.id is already the table's PRIMARY KEY (globally unique on its own),
-- so this index changes no existing behavior — it only makes the
-- (id, tenant_id) pair usable as a composite FK target, same shape as
-- 002_asset_directory.sql's asset_categories/assets UNIQUE(id, tenant_id).
CREATE UNIQUE INDEX IF NOT EXISTS idx_quotes_id_tenant ON quotes(id, tenant_id);

CREATE TABLE IF NOT EXISTS compliance_terms (
    id VARCHAR(36) PRIMARY KEY,
    tenant_id VARCHAR(36) NOT NULL,
    category VARCHAR(50) NOT NULL, -- 'Payment' | 'Liability' | 'Warranty' | 'Cancellation' | 'General'
    title VARCHAR(255) NOT NULL,
    body_text TEXT NOT NULL,
    is_default INT NOT NULL DEFAULT 0, -- pre-selected on new quotes if 1
    display_order INT NOT NULL DEFAULT 0,
    is_active INT NOT NULL DEFAULT 1, -- soft-delete: retired clauses stay
                                       -- referenceable by quotes that already
                                       -- selected them (same reasoning as
                                       -- assets.status='Retired'), just
                                       -- excluded from new selection
    created_at VARCHAR(30) NOT NULL,
    updated_at VARCHAR(30) NOT NULL,
    FOREIGN KEY (tenant_id) REFERENCES tenants(id) ON DELETE CASCADE,
    UNIQUE(id, tenant_id) -- required as the target of quote_compliance_terms' composite FK below
);
CREATE INDEX IF NOT EXISTS idx_compliance_terms_tenant ON compliance_terms(tenant_id, category);

CREATE TABLE IF NOT EXISTS tax_rules (
    id VARCHAR(36) PRIMARY KEY,
    tenant_id VARCHAR(36) NOT NULL,
    region_label VARCHAR(255) NOT NULL, -- e.g. 'UAE Standard VAT', 'Zero-Rated Export'
    rate NUMERIC(5, 2) NOT NULL DEFAULT 5.00, -- percentage, e.g. 5.00 = 5%
    is_default INT NOT NULL DEFAULT 0, -- at most one row per tenant should be
                                        -- 1 in practice; enforced at the
                                        -- application layer (tax_rules.rs),
                                        -- not by a DB constraint, so a
                                        -- momentary two-default state during
                                        -- an update transaction is never a
                                        -- hard DB error
    is_active INT NOT NULL DEFAULT 1,
    created_at VARCHAR(30) NOT NULL,
    updated_at VARCHAR(30) NOT NULL,
    FOREIGN KEY (tenant_id) REFERENCES tenants(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_tax_rules_tenant ON tax_rules(tenant_id);

-- quotes.vat_rate (001_core_schema.sql) is kept as-is and remains the value
-- actually used for a saved quote's VAT calculation — tax_rules is a
-- selectable library a user picks from (Phase 4.1's "selecting dynamic...
-- templates"), not a replacement for the stored per-quote rate. Selecting a
-- tax_rule when saving a quote copies its rate into quotes.vat_rate at that
-- moment (a deliberate snapshot, not a live foreign key) — this matches
-- quote_items' existing "line items are replaced wholesale on save" pattern
-- and the module doc in quotes.rs's stated preference for revision-safe
-- snapshots over live references that could silently change historical
-- quotes if a tax rate is edited later.

CREATE TABLE IF NOT EXISTS quote_compliance_terms (
    id VARCHAR(36) PRIMARY KEY,
    tenant_id VARCHAR(36) NOT NULL,
    quote_id VARCHAR(36) NOT NULL,
    compliance_term_id VARCHAR(36) NOT NULL,
    display_order INT NOT NULL DEFAULT 0,
    created_at VARCHAR(30) NOT NULL,
    FOREIGN KEY (tenant_id) REFERENCES tenants(id) ON DELETE CASCADE,
    FOREIGN KEY (quote_id, tenant_id) REFERENCES quotes(id, tenant_id) ON DELETE CASCADE,
    FOREIGN KEY (compliance_term_id, tenant_id) REFERENCES compliance_terms(id, tenant_id) ON DELETE RESTRICT,
    UNIQUE(quote_id, compliance_term_id)
);
CREATE INDEX IF NOT EXISTS idx_quote_compliance_terms_quote ON quote_compliance_terms(tenant_id, quote_id);

-- ============================================================
-- SECTION B — SQLITE-ONLY: updated_at triggers
-- Run this section only against a SQLite target.
-- ============================================================

CREATE TRIGGER IF NOT EXISTS trg_compliance_terms_updated_at
AFTER UPDATE ON compliance_terms
BEGIN
    UPDATE compliance_terms SET updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = NEW.id;
END;

CREATE TRIGGER IF NOT EXISTS trg_tax_rules_updated_at
AFTER UPDATE ON tax_rules
BEGIN
    UPDATE tax_rules SET updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = NEW.id;
END;

-- ============================================================
-- SECTION C — POSTGRESQL-ONLY: updated_at trigger function + triggers
-- Run this section only against a PostgreSQL target.
-- Not yet executed against a live server (same open item already logged
-- against 001_core_schema.sql and 002_asset_directory.sql).
-- ============================================================

-- CREATE TRIGGER trg_compliance_terms_updated_at
-- BEFORE UPDATE ON compliance_terms
-- FOR EACH ROW EXECUTE FUNCTION set_updated_at();
--
-- CREATE TRIGGER trg_tax_rules_updated_at
-- BEFORE UPDATE ON tax_rules
-- FOR EACH ROW EXECUTE FUNCTION set_updated_at();
