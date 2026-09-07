-- ============================================================
-- 002_asset_directory.sql — CORRECTED (Phase 2.1, Architect review)
-- Compatible with SQLite and PostgreSQL.
--
-- CHANGES FROM ORIGINAL DELIVERY:
-- 1. Added default rate fields to `assets` — route-map-v2.docx's Phase 2.1
--    deliverables list explicitly reads "Tables assets, asset_categories, and
--    equipment_specs (model, capacity, serial number, default rates)". The
--    original delivery covered model/serial (make_model/serial_number) and left
--    capacity to the flexible equipment_specs key-value design (a reasonable
--    choice), but had NO default-rate field anywhere. Phase 2.2's own
--    Completion Check ("Selecting an asset populates description, technical
--    specifications, and default pricing in the quote builder instantly")
--    cannot be met without this — there is nothing to populate. Added three
--    tier-matched fields (default_daily_rate/default_weekly_rate/
--    default_monthly_rate) mirroring quote_items.rate_basis's existing
--    Daily/Weekly/Monthly vocabulary and Phase 3.1's planned tier boundaries,
--    rather than inventing a new rate shape.
-- 2. Cross-tenant leakage closed via composite foreign keys. The original
--    schema's tenant_id columns were present and indexed (correct), but the
--    category_id and asset_id foreign keys only pointed at the parent table's
--    bare `id` — nothing stopped a row from tenant A referencing a parent row
--    belonging to tenant B (confirmed empirically: inserting an `assets` row
--    for Tenant 2 that referenced Tenant 1's asset_categories.id succeeded
--    under the original schema with foreign_keys=ON). Fixed by adding
--    UNIQUE(id, tenant_id) to asset_categories and assets (the parent side of
--    each relationship) and changing the child-side FKs to composite
--    (child_id, tenant_id) -> (parent.id, parent.tenant_id). Verified this
--    now rejects a mismatched-tenant insert with a genuine FK constraint
--    failure, and still accepts same-tenant inserts normally.
-- ============================================================

-- ============================================================
-- SECTION A — ENGINE-NEUTRAL TABLE DDL (run on both targets)
-- ============================================================

CREATE TABLE IF NOT EXISTS asset_categories (
    id VARCHAR(36) PRIMARY KEY,
    tenant_id VARCHAR(36) NOT NULL,
    category_name VARCHAR(255) NOT NULL,
    description TEXT,
    created_at VARCHAR(30) NOT NULL,
    updated_at VARCHAR(30) NOT NULL,
    FOREIGN KEY (tenant_id) REFERENCES tenants(id) ON DELETE CASCADE,
    UNIQUE(tenant_id, category_name),
    UNIQUE(id, tenant_id) -- required as the target of assets' composite FK below
);
CREATE INDEX IF NOT EXISTS idx_asset_categories_tenant ON asset_categories(tenant_id);

CREATE TABLE IF NOT EXISTS assets (
    id VARCHAR(36) PRIMARY KEY,
    tenant_id VARCHAR(36) NOT NULL,
    category_id VARCHAR(36) NOT NULL,
    asset_code VARCHAR(100) NOT NULL,
    asset_name VARCHAR(255) NOT NULL,
    description TEXT,
    make_model VARCHAR(255),
    serial_number VARCHAR(100),
    location VARCHAR(255),
    status VARCHAR(50) DEFAULT 'Available', -- Available / In-Use / Maintenance / Retired
    purchase_date VARCHAR(10), -- YYYY-MM-DD
    warranty_expiry VARCHAR(10), -- YYYY-MM-DD
    default_daily_rate NUMERIC(12, 2) DEFAULT 0.00,   -- Phase 3 tier: Daily < 6d
    default_weekly_rate NUMERIC(12, 2) DEFAULT 0.00,  -- Phase 3 tier: Weekly 7-25d
    default_monthly_rate NUMERIC(12, 2) DEFAULT 0.00, -- Phase 3 tier: Monthly 26d+
    created_at VARCHAR(30) NOT NULL,
    updated_at VARCHAR(30) NOT NULL,
    FOREIGN KEY (tenant_id) REFERENCES tenants(id) ON DELETE CASCADE,
    FOREIGN KEY (category_id, tenant_id) REFERENCES asset_categories(id, tenant_id) ON DELETE RESTRICT,
    UNIQUE(tenant_id, asset_code),
    UNIQUE(id, tenant_id) -- required as the target of equipment_specs' composite FK below
);
CREATE INDEX IF NOT EXISTS idx_assets_tenant ON assets(tenant_id);
CREATE INDEX IF NOT EXISTS idx_assets_category ON assets(tenant_id, category_id);
CREATE INDEX IF NOT EXISTS idx_assets_status ON assets(tenant_id, status);

CREATE TABLE IF NOT EXISTS equipment_specs (
    id VARCHAR(36) PRIMARY KEY,
    tenant_id VARCHAR(36) NOT NULL,
    asset_id VARCHAR(36) NOT NULL,
    spec_key VARCHAR(100) NOT NULL, -- e.g. 'storage_capacity', 'gas_type', 'tank_volume'
    spec_value VARCHAR(500) NOT NULL,
    unit_of_measure VARCHAR(50), -- e.g. 'Litres', 'PSI', 'Kg', NULL for dimensionless
    created_at VARCHAR(30) NOT NULL,
    updated_at VARCHAR(30) NOT NULL,
    FOREIGN KEY (tenant_id) REFERENCES tenants(id) ON DELETE CASCADE,
    FOREIGN KEY (asset_id, tenant_id) REFERENCES assets(id, tenant_id) ON DELETE CASCADE,
    UNIQUE(asset_id, spec_key)
);
CREATE INDEX IF NOT EXISTS idx_equipment_specs_tenant ON equipment_specs(tenant_id);
CREATE INDEX IF NOT EXISTS idx_equipment_specs_asset ON equipment_specs(asset_id);

-- ============================================================
-- SECTION B — SQLITE-ONLY: updated_at triggers
-- Run this section only against a SQLite target.
-- ============================================================

CREATE TRIGGER IF NOT EXISTS trg_asset_categories_updated_at
AFTER UPDATE ON asset_categories
BEGIN
    UPDATE asset_categories SET updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = NEW.id;
END;

CREATE TRIGGER IF NOT EXISTS trg_assets_updated_at
AFTER UPDATE ON assets
BEGIN
    UPDATE assets SET updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = NEW.id;
END;

CREATE TRIGGER IF NOT EXISTS trg_equipment_specs_updated_at
AFTER UPDATE ON equipment_specs
BEGIN
    UPDATE equipment_specs SET updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = NEW.id;
END;

-- ============================================================
-- SECTION C — POSTGRESQL-ONLY: updated_at trigger function + triggers
-- Run this section only against a PostgreSQL target.
-- Not yet executed against a live server (no Postgres instance provisioned in
-- this project yet, same open item already logged against 001_core_schema.sql).
-- ============================================================

-- CREATE OR REPLACE FUNCTION set_updated_at()
-- RETURNS TRIGGER AS $$
-- BEGIN
--     NEW.updated_at = to_char(now() AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"');
--     RETURN NEW;
-- END;
-- $$ LANGUAGE plpgsql;
--
-- CREATE TRIGGER trg_asset_categories_updated_at
-- BEFORE UPDATE ON asset_categories
-- FOR EACH ROW EXECUTE FUNCTION set_updated_at();
--
-- CREATE TRIGGER trg_assets_updated_at
-- BEFORE UPDATE ON assets
-- FOR EACH ROW EXECUTE FUNCTION set_updated_at();
--
-- CREATE TRIGGER trg_equipment_specs_updated_at
-- BEFORE UPDATE ON equipment_specs
-- FOR EACH ROW EXECUTE FUNCTION set_updated_at();
