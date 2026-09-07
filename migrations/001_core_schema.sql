-- ============================================================
-- 001_core_schema.sql — CORRECTED (Phase 1.1, Architect review)
-- Compatible with SQLite and PostgreSQL.
--
-- CHANGE FROM ORIGINAL DELIVERY: timestamp triggers were part of the
-- assigned scope (auto-updating `updated_at` on row modification) but
-- were not delivered. Because trigger syntax genuinely differs between
-- SQLite and PostgreSQL (no single statement works on both), this file
-- is split into three sections below: engine-neutral table DDL, then
-- SQLite triggers, then PostgreSQL triggers. Run the neutral section
-- plus whichever engine-specific section matches your target.
-- ============================================================

-- ============================================================
-- SECTION A — ENGINE-NEUTRAL TABLE DDL (run on both targets)
-- ============================================================

CREATE TABLE IF NOT EXISTS tenants (
    id VARCHAR(36) PRIMARY KEY,
    name VARCHAR(255) NOT NULL,
    company_name_ar VARCHAR(255), -- Arabic company name for letterhead header
    company_tagline_ar VARCHAR(255), -- Arabic tagline line for letterhead header
    address_line1 VARCHAR(255),
    address_line2 VARCHAR(255),
    po_box VARCHAR(50),
    city VARCHAR(100),
    country VARCHAR(100) DEFAULT 'United Arab Emirates',
    phone_1 VARCHAR(50),
    phone_2 VARCHAR(50),
    email VARCHAR(255),
    website VARCHAR(255),
    vat_trn VARCHAR(100),
    created_at VARCHAR(30) NOT NULL,
    updated_at VARCHAR(30) NOT NULL
);

CREATE TABLE IF NOT EXISTS users (
    id VARCHAR(36) PRIMARY KEY,
    tenant_id VARCHAR(36) NOT NULL,
    email VARCHAR(255) NOT NULL UNIQUE,
    password_hash VARCHAR(255) NOT NULL,
    full_name VARCHAR(255) NOT NULL,
    phone VARCHAR(50),
    role VARCHAR(20) NOT NULL CHECK (role IN ('admin', 'staff')),
    is_active INT NOT NULL DEFAULT 1,
    created_at VARCHAR(30) NOT NULL,
    updated_at VARCHAR(30) NOT NULL,
    FOREIGN KEY (tenant_id) REFERENCES tenants(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_users_tenant_email ON users(tenant_id, email);

CREATE TABLE IF NOT EXISTS tenant_entitlements (
    id VARCHAR(36) PRIMARY KEY,
    tenant_id VARCHAR(36) NOT NULL,
    module_key VARCHAR(50) NOT NULL,
    is_enabled INT NOT NULL DEFAULT 1,
    granted_at VARCHAR(30) NOT NULL,
    expires_at VARCHAR(30),
    FOREIGN KEY (tenant_id) REFERENCES tenants(id) ON DELETE CASCADE,
    UNIQUE(tenant_id, module_key)
);
CREATE INDEX IF NOT EXISTS idx_entitlements_tenant ON tenant_entitlements(tenant_id, module_key);
-- Canonical module_key vocabulary (registered in PROJECT_BASELINE.md):
-- 'quotes_core' | 'inventory_specs' | 'rate_matrix' | 'compliance_terms' | 'revision_lpo'

CREATE TABLE IF NOT EXISTS quotes (
    id VARCHAR(36) PRIMARY KEY,
    tenant_id VARCHAR(36) NOT NULL,
    user_id VARCHAR(36) NOT NULL,
    offer_ref VARCHAR(100) NOT NULL,       -- e.g. QN-EH/211/2026
    rev_suffix VARCHAR(20) NOT NULL,      -- e.g. Rev.01
    quote_date VARCHAR(10) NOT NULL,      -- YYYY-MM-DD
    validity_days INT DEFAULT 30,
    customer_name VARCHAR(255) NOT NULL,
    customer_po_box VARCHAR(50),
    customer_city VARCHAR(100),
    contact_person VARCHAR(255),
    customer_email VARCHAR(255),
    customer_ref VARCHAR(100),
    salesperson_name VARCHAR(255),
    salesperson_phone VARCHAR(50),
    subject_text VARCHAR(500),
    notes TEXT,
    terms_conditions TEXT,
    rate_basis_text TEXT,
    total_amount NUMERIC(12, 2) DEFAULT 0.00,
    vat_rate NUMERIC(5, 2) DEFAULT 5.00,
    vat_amount NUMERIC(12, 2) DEFAULT 0.00,
    grand_total NUMERIC(12, 2) DEFAULT 0.00,
    status VARCHAR(30) DEFAULT 'Draft',
    created_at VARCHAR(30) NOT NULL,
    updated_at VARCHAR(30) NOT NULL,
    FOREIGN KEY (tenant_id) REFERENCES tenants(id) ON DELETE CASCADE,
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE RESTRICT,
    UNIQUE(tenant_id, offer_ref, rev_suffix)
);
CREATE INDEX IF NOT EXISTS idx_quotes_tenant_ref ON quotes(tenant_id, offer_ref);

CREATE TABLE IF NOT EXISTS quote_items (
    id VARCHAR(36) PRIMARY KEY,
    tenant_id VARCHAR(36) NOT NULL,
    quote_id VARCHAR(36) NOT NULL,
    item_order INT NOT NULL,
    item_description TEXT NOT NULL,
    make_model VARCHAR(255),
    quantity INT NOT NULL DEFAULT 1,
    unit_rate NUMERIC(12, 2) NOT NULL DEFAULT 0.00,
    rate_basis VARCHAR(50) DEFAULT 'Monthly', -- Daily / Weekly / Monthly
    line_total NUMERIC(12, 2) NOT NULL DEFAULT 0.00,
    equipment_spec TEXT,
    created_at VARCHAR(30) NOT NULL,
    FOREIGN KEY (tenant_id) REFERENCES tenants(id) ON DELETE CASCADE,
    FOREIGN KEY (quote_id) REFERENCES quotes(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_quote_items_tenant_quote ON quote_items(tenant_id, quote_id);
-- Note: quote_items intentionally has no updated_at/trigger — line-item edits are
-- expected to flow through the Phase 5 quote_revisions snapshot mechanism rather
-- than in-place mutation. Revisit if Phase 5 design changes this assumption.

-- ============================================================
-- SECTION B — SQLITE-ONLY: updated_at triggers
-- Run this section only against a SQLite target.
-- ============================================================

CREATE TRIGGER IF NOT EXISTS trg_tenants_updated_at
AFTER UPDATE ON tenants
BEGIN
    UPDATE tenants SET updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = NEW.id;
END;

CREATE TRIGGER IF NOT EXISTS trg_users_updated_at
AFTER UPDATE ON users
BEGIN
    UPDATE users SET updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = NEW.id;
END;

CREATE TRIGGER IF NOT EXISTS trg_quotes_updated_at
AFTER UPDATE ON quotes
BEGIN
    UPDATE quotes SET updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = NEW.id;
END;

-- ============================================================
-- SECTION C — POSTGRESQL-ONLY: updated_at trigger function + triggers
-- Run this section only against a PostgreSQL target.
-- ============================================================

-- CREATE OR REPLACE FUNCTION set_updated_at()
-- RETURNS TRIGGER AS $$
-- BEGIN
--     NEW.updated_at = to_char(now() AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"');
--     RETURN NEW;
-- END;
-- $$ LANGUAGE plpgsql;
--
-- CREATE TRIGGER trg_tenants_updated_at
-- BEFORE UPDATE ON tenants
-- FOR EACH ROW EXECUTE FUNCTION set_updated_at();
--
-- CREATE TRIGGER trg_users_updated_at
-- BEFORE UPDATE ON users
-- FOR EACH ROW EXECUTE FUNCTION set_updated_at();
--
-- CREATE TRIGGER trg_quotes_updated_at
-- BEFORE UPDATE ON quotes
-- FOR EACH ROW EXECUTE FUNCTION set_updated_at();
--
-- (Commented out: this project's Postgres deployment path isn't provisioned yet —
-- uncomment and run against an actual Postgres instance once one exists. Syntax
-- reviewed manually for correctness; not yet executed against a live server.)
