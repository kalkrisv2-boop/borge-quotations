//! Phase 4.1 — Terms Library & Regional VAT Schema.
//!
//! Rust-native port targeting `route-map-v2.docx` Phase 4.1's stated deliverables:
//! "Tables compliance_terms and tax_rules. UI manager for editing standard legal
//! disclaimers, warranty conditions, and default liability clauses." Built following the
//! same pattern established in `quotes.rs`/`db.rs` during Phase R (real SQLite via
//! `migrations/003_compliance_terms.sql`, not an in-memory stub; every function scoped
//! by `tenant_id`; tests run against the real, attached schema file via `include_str!`).
//!
//! ## Selection is data, not free text — flagged explicitly, per this project's
//! established practice of stating design decisions rather than silently picking one
//! Phase 4.1's own Completion Check reads "Allows storing, updating, and selecting
//! dynamic legal term templates." "Selecting" implies which specific clauses were
//! chosen for a given quote is itself a fact worth persisting and auditing — not just
//! concatenated into `quotes.terms_conditions` as flat text (which is what happens
//! today, and is a known gap `build_pdf_context` already flags in `lib.rs`). This module
//! therefore manages `quote_compliance_terms` as a real join table (`attach_terms_to_quote`
//! below), analogous to how `quotes.rs` manages `quote_items` as owned child rows rather
//! than a denormalized string.
//!
//! ## Tax rules are a library, not a live reference — flagged explicitly
//! `tax_rules` rows are a *selectable menu* a user picks from; selecting one copies its
//! `rate` into `quotes.vat_rate` at save time (see `resolve_tax_rate` below), rather than
//! quotes storing a live foreign key to a `tax_rules` row. This mirrors `quotes.rs`'s own
//! stated preference (module doc, Phase R.3) for revision-safe snapshots: if a tax rate
//! is edited later, previously saved quotes must not silently change.

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// compliance_terms
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComplianceTermInput {
    pub category: String, // 'Payment' | 'Liability' | 'Warranty' | 'Cancellation' | 'General'
    pub title: String,
    pub body_text: String,
    #[serde(default)]
    pub is_default: bool,
    #[serde(default)]
    pub display_order: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComplianceTermRecord {
    pub id: String,
    pub category: String,
    pub title: String,
    pub body_text: String,
    pub is_default: bool,
    pub display_order: i64,
    pub is_active: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug)]
pub enum ComplianceError {
    InvalidPayload(String),
    NotFound,
    Db(rusqlite::Error),
}

impl From<rusqlite::Error> for ComplianceError {
    fn from(e: rusqlite::Error) -> Self {
        ComplianceError::Db(e)
    }
}

const VALID_CATEGORIES: &[&str] = &["Payment", "Liability", "Warranty", "Cancellation", "General"];

fn generate_id() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 18];
    rand::thread_rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}

fn now_timestamp(conn: &Connection) -> rusqlite::Result<String> {
    conn.query_row("SELECT strftime('%Y-%m-%dT%H:%M:%fZ', 'now')", [], |row| {
        row.get(0)
    })
}

fn validate_term_input(input: &ComplianceTermInput) -> Result<(), ComplianceError> {
    if input.title.trim().is_empty() {
        return Err(ComplianceError::InvalidPayload("title is required".into()));
    }
    if input.body_text.trim().is_empty() {
        return Err(ComplianceError::InvalidPayload(
            "body_text is required".into(),
        ));
    }
    if !VALID_CATEGORIES.contains(&input.category.as_str()) {
        return Err(ComplianceError::InvalidPayload(format!(
            "category must be one of {:?}, got {:?}",
            VALID_CATEGORIES, input.category
        )));
    }
    Ok(())
}

/// Creates a new compliance term for `tenant_id`. Tenant isolation: `tenant_id` is
/// written directly onto the inserted row, never taken from caller-supplied data.
pub fn create_compliance_term(
    conn: &Connection,
    tenant_id: &str,
    input: &ComplianceTermInput,
) -> Result<String, ComplianceError> {
    validate_term_input(input)?;
    let id = generate_id();
    let ts = now_timestamp(conn)?;
    conn.execute(
        "INSERT INTO compliance_terms (
            id, tenant_id, category, title, body_text, is_default, display_order,
            is_active, created_at, updated_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 1, ?8, ?8)",
        params![
            id,
            tenant_id,
            input.category,
            input.title,
            input.body_text,
            input.is_default as i64,
            input.display_order,
            ts,
        ],
    )?;
    Ok(id)
}

/// Updates an existing compliance term. Tenant isolation: the UPDATE's WHERE clause
/// requires both `id` AND `tenant_id` to match — a caller can never modify another
/// tenant's row even if it guesses a valid id.
pub fn update_compliance_term(
    conn: &Connection,
    tenant_id: &str,
    id: &str,
    input: &ComplianceTermInput,
) -> Result<(), ComplianceError> {
    validate_term_input(input)?;
    let ts = now_timestamp(conn)?;
    let rows = conn.execute(
        "UPDATE compliance_terms SET
            category = ?1, title = ?2, body_text = ?3, is_default = ?4,
            display_order = ?5, updated_at = ?6
         WHERE id = ?7 AND tenant_id = ?8",
        params![
            input.category,
            input.title,
            input.body_text,
            input.is_default as i64,
            input.display_order,
            ts,
            id,
            tenant_id,
        ],
    )?;
    if rows == 0 {
        return Err(ComplianceError::NotFound);
    }
    Ok(())
}

/// Soft-deletes a compliance term (`is_active = 0`) rather than a hard DELETE — mirrors
/// `assets.status = 'Retired'`'s reasoning (Phase 2.1): a term already selected on a
/// past quote (`quote_compliance_terms`, `ON DELETE RESTRICT`) must remain readable for
/// that quote's history, just excluded from new selection going forward.
pub fn deactivate_compliance_term(
    conn: &Connection,
    tenant_id: &str,
    id: &str,
) -> Result<(), ComplianceError> {
    let ts = now_timestamp(conn)?;
    let rows = conn.execute(
        "UPDATE compliance_terms SET is_active = 0, updated_at = ?1 WHERE id = ?2 AND tenant_id = ?3",
        params![ts, id, tenant_id],
    )?;
    if rows == 0 {
        return Err(ComplianceError::NotFound);
    }
    Ok(())
}

/// Lists compliance terms for `tenant_id`. `active_only = true` is the normal UI-facing
/// view (what a user picks from when building a quote); `false` includes retired terms
/// (needed to render historical quotes that selected a since-retired term).
pub fn list_compliance_terms(
    conn: &Connection,
    tenant_id: &str,
    active_only: bool,
) -> Result<Vec<ComplianceTermRecord>, ComplianceError> {
    let sql = if active_only {
        "SELECT id, category, title, body_text, is_default, display_order, is_active,
                created_at, updated_at
         FROM compliance_terms
         WHERE tenant_id = ?1 AND is_active = 1
         ORDER BY category ASC, display_order ASC, title ASC"
    } else {
        "SELECT id, category, title, body_text, is_default, display_order, is_active,
                created_at, updated_at
         FROM compliance_terms
         WHERE tenant_id = ?1
         ORDER BY category ASC, display_order ASC, title ASC"
    };
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt
        .query_map(params![tenant_id], row_to_term)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn row_to_term(row: &rusqlite::Row) -> rusqlite::Result<ComplianceTermRecord> {
    Ok(ComplianceTermRecord {
        id: row.get(0)?,
        category: row.get(1)?,
        title: row.get(2)?,
        body_text: row.get(3)?,
        is_default: row.get::<_, i64>(4)? != 0,
        display_order: row.get(5)?,
        is_active: row.get::<_, i64>(6)? != 0,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
    })
}

// ---------------------------------------------------------------------------
// quote_compliance_terms — the "which clauses did this quote actually use" join
// ---------------------------------------------------------------------------

/// Replaces the full set of compliance terms attached to `quote_id` with exactly
/// `term_ids`, in the given order. Wholesale delete-then-reinsert, matching
/// `quotes.rs::save_quote`'s existing pattern for `quote_items` (Phase R.3) rather than
/// a diff — keeps `display_order` trivially correct and avoids a second, subtly
/// different update strategy for what is structurally the same kind of child-row set.
///
/// Tenant isolation: every statement is scoped by `tenant_id`. The composite FK on
/// `quote_compliance_terms(compliance_term_id, tenant_id) -> compliance_terms(id,
/// tenant_id)` (migration 003) additionally makes it a real constraint violation, not
/// just an application-level check, for `term_ids` to reference another tenant's term.
pub fn attach_terms_to_quote(
    conn: &Connection,
    tenant_id: &str,
    quote_id: &str,
    term_ids: &[String],
) -> Result<(), ComplianceError> {
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "DELETE FROM quote_compliance_terms WHERE quote_id = ?1 AND tenant_id = ?2",
        params![quote_id, tenant_id],
    )?;
    for (idx, term_id) in term_ids.iter().enumerate() {
        let ts = now_timestamp(&tx)?;
        tx.execute(
            "INSERT INTO quote_compliance_terms (
                id, tenant_id, quote_id, compliance_term_id, display_order, created_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![generate_id(), tenant_id, quote_id, term_id, idx as i64, ts],
        )?;
    }
    tx.commit()?;
    Ok(())
}

/// Fetches the compliance terms attached to `quote_id`, in selection order — this is
/// what Phase 4.2's PDF injection step reads from, replacing the current
/// single-`terms_conditions`-string hack flagged in `lib.rs::build_pdf_context`.
pub fn fetch_terms_for_quote(
    conn: &Connection,
    tenant_id: &str,
    quote_id: &str,
) -> Result<Vec<ComplianceTermRecord>, ComplianceError> {
    let mut stmt = conn.prepare(
        "SELECT ct.id, ct.category, ct.title, ct.body_text, ct.is_default,
                qct.display_order, ct.is_active, ct.created_at, ct.updated_at
         FROM quote_compliance_terms qct
         JOIN compliance_terms ct ON ct.id = qct.compliance_term_id AND ct.tenant_id = qct.tenant_id
         WHERE qct.tenant_id = ?1 AND qct.quote_id = ?2
         ORDER BY qct.display_order ASC",
    )?;
    let rows = stmt
        .query_map(params![tenant_id, quote_id], row_to_term)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

// ---------------------------------------------------------------------------
// tax_rules
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaxRuleInput {
    pub region_label: String,
    pub rate: f64,
    #[serde(default)]
    pub is_default: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaxRuleRecord {
    pub id: String,
    pub region_label: String,
    pub rate: f64,
    pub is_default: bool,
    pub is_active: bool,
    pub created_at: String,
    pub updated_at: String,
}

fn validate_tax_rule_input(input: &TaxRuleInput) -> Result<(), ComplianceError> {
    if input.region_label.trim().is_empty() {
        return Err(ComplianceError::InvalidPayload(
            "region_label is required".into(),
        ));
    }
    if input.rate < 0.0 || input.rate > 100.0 {
        return Err(ComplianceError::InvalidPayload(format!(
            "rate must be between 0 and 100, got {}",
            input.rate
        )));
    }
    Ok(())
}

/// Creates a tax rule. If `input.is_default` is true, every other active tax rule for
/// this tenant is unset first (application-level "at most one default" enforcement —
/// see migration 003's comment on why this isn't a DB constraint) within the same
/// transaction, so there is never a moment where two rows are simultaneously default for
/// a caller reading between the two statements.
pub fn create_tax_rule(
    conn: &Connection,
    tenant_id: &str,
    input: &TaxRuleInput,
) -> Result<String, ComplianceError> {
    validate_tax_rule_input(input)?;
    let tx = conn.unchecked_transaction()?;
    let ts = now_timestamp(&tx)?;
    if input.is_default {
        tx.execute(
            "UPDATE tax_rules SET is_default = 0, updated_at = ?1 WHERE tenant_id = ?2 AND is_default = 1",
            params![ts, tenant_id],
        )?;
    }
    let id = generate_id();
    tx.execute(
        "INSERT INTO tax_rules (
            id, tenant_id, region_label, rate, is_default, is_active, created_at, updated_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6, ?6)",
        params![id, tenant_id, input.region_label, input.rate, input.is_default as i64, ts],
    )?;
    tx.commit()?;
    Ok(id)
}

pub fn update_tax_rule(
    conn: &Connection,
    tenant_id: &str,
    id: &str,
    input: &TaxRuleInput,
) -> Result<(), ComplianceError> {
    validate_tax_rule_input(input)?;
    let tx = conn.unchecked_transaction()?;
    let ts = now_timestamp(&tx)?;
    if input.is_default {
        tx.execute(
            "UPDATE tax_rules SET is_default = 0, updated_at = ?1
             WHERE tenant_id = ?2 AND is_default = 1 AND id != ?3",
            params![ts, tenant_id, id],
        )?;
    }
    let rows = tx.execute(
        "UPDATE tax_rules SET region_label = ?1, rate = ?2, is_default = ?3, updated_at = ?4
         WHERE id = ?5 AND tenant_id = ?6",
        params![input.region_label, input.rate, input.is_default as i64, ts, id, tenant_id],
    )?;
    if rows == 0 {
        return Err(ComplianceError::NotFound);
    }
    tx.commit()?;
    Ok(())
}

pub fn deactivate_tax_rule(conn: &Connection, tenant_id: &str, id: &str) -> Result<(), ComplianceError> {
    let ts = now_timestamp(conn)?;
    let rows = conn.execute(
        "UPDATE tax_rules SET is_active = 0, is_default = 0, updated_at = ?1 WHERE id = ?2 AND tenant_id = ?3",
        params![ts, id, tenant_id],
    )?;
    if rows == 0 {
        return Err(ComplianceError::NotFound);
    }
    Ok(())
}

pub fn list_tax_rules(
    conn: &Connection,
    tenant_id: &str,
    active_only: bool,
) -> Result<Vec<TaxRuleRecord>, ComplianceError> {
    let sql = if active_only {
        "SELECT id, region_label, rate, is_default, is_active, created_at, updated_at
         FROM tax_rules WHERE tenant_id = ?1 AND is_active = 1
         ORDER BY is_default DESC, region_label ASC"
    } else {
        "SELECT id, region_label, rate, is_default, is_active, created_at, updated_at
         FROM tax_rules WHERE tenant_id = ?1
         ORDER BY is_default DESC, region_label ASC"
    };
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt
        .query_map(params![tenant_id], row_to_tax_rule)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Resolves the rate a quote should actually use: the given `tax_rule_id` if present and
/// active for this tenant, else this tenant's default rule, else `None` (caller falls
/// back to the existing `quotes.vat_rate` default column behavior — this function adds a
/// selectable library on top of that default, it doesn't remove it).
pub fn resolve_tax_rate(
    conn: &Connection,
    tenant_id: &str,
    tax_rule_id: Option<&str>,
) -> Result<Option<f64>, ComplianceError> {
    if let Some(id) = tax_rule_id {
        let rate: Option<f64> = conn
            .query_row(
                "SELECT rate FROM tax_rules WHERE id = ?1 AND tenant_id = ?2 AND is_active = 1",
                params![id, tenant_id],
                |row| row.get(0),
            )
            .optional()?;
        if rate.is_some() {
            return Ok(rate);
        }
        // Explicit id given but not found/inactive/wrong-tenant: fall through to
        // tenant default rather than silently erroring — a retired rule referenced by
        // an old quote id shouldn't break resolution, it should degrade gracefully.
    }
    let default_rate: Option<f64> = conn
        .query_row(
            "SELECT rate FROM tax_rules WHERE tenant_id = ?1 AND is_default = 1 AND is_active = 1",
            params![tenant_id],
            |row| row.get(0),
        )
        .optional()?;
    Ok(default_rate)
}

fn row_to_tax_rule(row: &rusqlite::Row) -> rusqlite::Result<TaxRuleRecord> {
    Ok(TaxRuleRecord {
        id: row.get(0)?,
        region_label: row.get(1)?,
        rate: row.get(2)?,
        is_default: row.get::<_, i64>(3)? != 0,
        is_active: row.get::<_, i64>(4)? != 0,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
    })
}

// ---------------------------------------------------------------------------
// Tests — against the real, attached schema (Phase R's established pattern, not a
// hand-typed mock schema)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const REAL_SCHEMA_001: &str = include_str!("../../migrations/001_core_schema.sql");
    const REAL_SCHEMA_003: &str = include_str!("../../migrations/003_compliance_terms.sql");

    fn test_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        conn.execute_batch(REAL_SCHEMA_001).unwrap();
        conn.execute_batch(REAL_SCHEMA_003).unwrap();
        conn.execute(
            "INSERT INTO tenants (id, name, created_at, updated_at)
             VALUES ('tenant-1', 'Test Tenant', '2026-01-01T00:00:00.000Z', '2026-01-01T00:00:00.000Z')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO users (id, tenant_id, email, password_hash, full_name, role, created_at, updated_at)
             VALUES ('user-1', 'tenant-1', 'u1@example.com', 'hash', 'Test User', 'staff', '2026-01-01T00:00:00.000Z', '2026-01-01T00:00:00.000Z')",
            [],
        )
        .unwrap();
        conn
    }

    fn insert_quote(conn: &Connection, id: &str) {
        conn.execute(
            "INSERT INTO quotes (
                id, tenant_id, user_id, offer_ref, rev_suffix, quote_date, customer_name,
                created_at, updated_at
             ) VALUES (?1, 'tenant-1', 'user-1', ?1, 'Rev.01', '2026-03-01', 'Acme LLC',
                       '2026-01-01T00:00:00.000Z', '2026-01-01T00:00:00.000Z')",
            params![id],
        )
        .unwrap();
    }

    fn sample_term(category: &str, title: &str) -> ComplianceTermInput {
        ComplianceTermInput {
            category: category.to_string(),
            title: title.to_string(),
            body_text: "Sample clause body text.".to_string(),
            is_default: false,
            display_order: 0,
        }
    }

    // ---- compliance_terms CRUD ----

    #[test]
    fn create_then_list_round_trips() {
        let conn = test_conn();
        let id = create_compliance_term(&conn, "tenant-1", &sample_term("Payment", "Net 30")).unwrap();
        let terms = list_compliance_terms(&conn, "tenant-1", true).unwrap();
        assert_eq!(terms.len(), 1);
        assert_eq!(terms[0].id, id);
        assert_eq!(terms[0].title, "Net 30");
        assert_eq!(terms[0].category, "Payment");
        assert!(terms[0].is_active);
    }

    #[test]
    fn create_rejects_invalid_category() {
        let conn = test_conn();
        let mut input = sample_term("Payment", "Net 30");
        input.category = "NotARealCategory".to_string();
        let result = create_compliance_term(&conn, "tenant-1", &input);
        assert!(matches!(result, Err(ComplianceError::InvalidPayload(_))));
    }

    #[test]
    fn create_rejects_empty_title() {
        let conn = test_conn();
        let mut input = sample_term("Payment", "");
        input.title = "".to_string();
        let result = create_compliance_term(&conn, "tenant-1", &input);
        assert!(matches!(result, Err(ComplianceError::InvalidPayload(_))));
    }

    #[test]
    fn update_changes_fields_and_rejects_unknown_id() {
        let conn = test_conn();
        let id = create_compliance_term(&conn, "tenant-1", &sample_term("Payment", "Net 30")).unwrap();
        let mut updated = sample_term("Liability", "Net 30 Revised");
        updated.display_order = 5;
        update_compliance_term(&conn, "tenant-1", &id, &updated).unwrap();

        let terms = list_compliance_terms(&conn, "tenant-1", true).unwrap();
        assert_eq!(terms[0].category, "Liability");
        assert_eq!(terms[0].title, "Net 30 Revised");
        assert_eq!(terms[0].display_order, 5);

        let result = update_compliance_term(&conn, "tenant-1", "does-not-exist", &updated);
        assert!(matches!(result, Err(ComplianceError::NotFound)));
    }

    #[test]
    fn deactivate_removes_from_active_list_but_not_all_list() {
        let conn = test_conn();
        let id = create_compliance_term(&conn, "tenant-1", &sample_term("Warranty", "Standard Warranty")).unwrap();
        deactivate_compliance_term(&conn, "tenant-1", &id).unwrap();

        let active = list_compliance_terms(&conn, "tenant-1", true).unwrap();
        assert_eq!(active.len(), 0);

        let all = list_compliance_terms(&conn, "tenant-1", false).unwrap();
        assert_eq!(all.len(), 1);
        assert!(!all[0].is_active);
    }

    #[test]
    fn cross_tenant_operations_are_isolated() {
        let conn = test_conn();
        conn.execute(
            "INSERT INTO tenants (id, name, created_at, updated_at)
             VALUES ('tenant-2', 'Other Tenant', '2026-01-01T00:00:00.000Z', '2026-01-01T00:00:00.000Z')",
            [],
        )
        .unwrap();

        let id = create_compliance_term(&conn, "tenant-1", &sample_term("Payment", "Tenant 1 Term")).unwrap();

        // tenant-2 cannot see tenant-1's term.
        let tenant2_terms = list_compliance_terms(&conn, "tenant-2", true).unwrap();
        assert_eq!(tenant2_terms.len(), 0);

        // tenant-2 cannot update or deactivate tenant-1's term.
        let update_result = update_compliance_term(&conn, "tenant-2", &id, &sample_term("Payment", "Hijacked"));
        assert!(matches!(update_result, Err(ComplianceError::NotFound)));
        let deactivate_result = deactivate_compliance_term(&conn, "tenant-2", &id);
        assert!(matches!(deactivate_result, Err(ComplianceError::NotFound)));

        // tenant-1's term is unaffected.
        let tenant1_terms = list_compliance_terms(&conn, "tenant-1", true).unwrap();
        assert_eq!(tenant1_terms.len(), 1);
        assert_eq!(tenant1_terms[0].title, "Tenant 1 Term");
    }

    // ---- quote_compliance_terms selection ----

    #[test]
    fn attach_terms_then_fetch_round_trips_in_order() {
        let conn = test_conn();
        insert_quote(&conn, "quote-1");
        let t1 = create_compliance_term(&conn, "tenant-1", &sample_term("Payment", "Term A")).unwrap();
        let t2 = create_compliance_term(&conn, "tenant-1", &sample_term("Liability", "Term B")).unwrap();

        attach_terms_to_quote(&conn, "tenant-1", "quote-1", &[t2.clone(), t1.clone()]).unwrap();

        let attached = fetch_terms_for_quote(&conn, "tenant-1", "quote-1").unwrap();
        assert_eq!(attached.len(), 2);
        assert_eq!(attached[0].id, t2, "selection order must be preserved (Term B first)");
        assert_eq!(attached[1].id, t1);
    }

    #[test]
    fn attach_terms_replaces_wholesale_on_resave() {
        let conn = test_conn();
        insert_quote(&conn, "quote-1");
        let t1 = create_compliance_term(&conn, "tenant-1", &sample_term("Payment", "Term A")).unwrap();
        let t2 = create_compliance_term(&conn, "tenant-1", &sample_term("Liability", "Term B")).unwrap();

        attach_terms_to_quote(&conn, "tenant-1", "quote-1", &[t1.clone(), t2.clone()]).unwrap();
        attach_terms_to_quote(&conn, "tenant-1", "quote-1", &[t2.clone()]).unwrap();

        let attached = fetch_terms_for_quote(&conn, "tenant-1", "quote-1").unwrap();
        assert_eq!(attached.len(), 1);
        assert_eq!(attached[0].id, t2);
    }

    #[test]
    fn attach_terms_rejects_cross_tenant_term_reference() {
        let conn = test_conn();
        conn.execute(
            "INSERT INTO tenants (id, name, created_at, updated_at)
             VALUES ('tenant-2', 'Other Tenant', '2026-01-01T00:00:00.000Z', '2026-01-01T00:00:00.000Z')",
            [],
        )
        .unwrap();
        insert_quote(&conn, "quote-1");
        let foreign_term = create_compliance_term(&conn, "tenant-2", &sample_term("Payment", "Not Yours")).unwrap();

        // The composite FK (compliance_term_id, tenant_id) -> compliance_terms(id,
        // tenant_id) must reject this at the DB layer, not just silently succeed with
        // a term from another tenant.
        let result = attach_terms_to_quote(&conn, "tenant-1", "quote-1", &[foreign_term]);
        assert!(result.is_err(), "attaching another tenant's term must fail");
    }

    // ---- tax_rules ----

    #[test]
    fn create_tax_rule_then_list_round_trips() {
        let conn = test_conn();
        let id = create_tax_rule(
            &conn,
            "tenant-1",
            &TaxRuleInput { region_label: "UAE Standard VAT".into(), rate: 5.0, is_default: true },
        )
        .unwrap();
        let rules = list_tax_rules(&conn, "tenant-1", true).unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].id, id);
        assert_eq!(rules[0].rate, 5.0);
        assert!(rules[0].is_default);
    }

    #[test]
    fn create_tax_rule_rejects_out_of_range_rate() {
        let conn = test_conn();
        let result = create_tax_rule(
            &conn,
            "tenant-1",
            &TaxRuleInput { region_label: "Bad Rate".into(), rate: 150.0, is_default: false },
        );
        assert!(matches!(result, Err(ComplianceError::InvalidPayload(_))));
    }

    #[test]
    fn only_one_default_tax_rule_survives_at_a_time() {
        let conn = test_conn();
        let id1 = create_tax_rule(
            &conn,
            "tenant-1",
            &TaxRuleInput { region_label: "Standard".into(), rate: 5.0, is_default: true },
        )
        .unwrap();
        let id2 = create_tax_rule(
            &conn,
            "tenant-1",
            &TaxRuleInput { region_label: "Zero-Rated".into(), rate: 0.0, is_default: true },
        )
        .unwrap();

        let rules = list_tax_rules(&conn, "tenant-1", true).unwrap();
        let defaults: Vec<&TaxRuleRecord> = rules.iter().filter(|r| r.is_default).collect();
        assert_eq!(defaults.len(), 1, "exactly one tax rule must be default at a time");
        assert_eq!(defaults[0].id, id2, "the most recently set default wins");

        let rules_by_id: std::collections::HashMap<_, _> =
            rules.iter().map(|r| (r.id.clone(), r)).collect();
        assert!(!rules_by_id[&id1].is_default);
    }

    #[test]
    fn resolve_tax_rate_prefers_explicit_id_then_falls_back_to_default() {
        let conn = test_conn();
        let default_id = create_tax_rule(
            &conn,
            "tenant-1",
            &TaxRuleInput { region_label: "Standard".into(), rate: 5.0, is_default: true },
        )
        .unwrap();
        let other_id = create_tax_rule(
            &conn,
            "tenant-1",
            &TaxRuleInput { region_label: "Zero-Rated".into(), rate: 0.0, is_default: false },
        )
        .unwrap();

        assert_eq!(
            resolve_tax_rate(&conn, "tenant-1", Some(&other_id)).unwrap(),
            Some(0.0),
            "explicit id must win over the default"
        );
        assert_eq!(
            resolve_tax_rate(&conn, "tenant-1", None).unwrap(),
            Some(5.0),
            "no explicit id must fall back to the tenant's default"
        );
        assert_eq!(
            resolve_tax_rate(&conn, "tenant-1", Some("does-not-exist")).unwrap(),
            Some(5.0),
            "an unknown/retired id must degrade to the default, not error"
        );
        let _ = default_id;
    }

    #[test]
    fn resolve_tax_rate_returns_none_when_no_default_configured() {
        let conn = test_conn();
        assert_eq!(resolve_tax_rate(&conn, "tenant-1", None).unwrap(), None);
    }

    #[test]
    fn deactivate_tax_rule_also_clears_default_flag() {
        let conn = test_conn();
        let id = create_tax_rule(
            &conn,
            "tenant-1",
            &TaxRuleInput { region_label: "Standard".into(), rate: 5.0, is_default: true },
        )
        .unwrap();
        deactivate_tax_rule(&conn, "tenant-1", &id).unwrap();

        assert_eq!(resolve_tax_rate(&conn, "tenant-1", None).unwrap(), None);
        let all = list_tax_rules(&conn, "tenant-1", false).unwrap();
        assert!(!all[0].is_active);
        assert!(!all[0].is_default);
    }

    #[test]
    fn tax_rule_cross_tenant_isolation() {
        let conn = test_conn();
        conn.execute(
            "INSERT INTO tenants (id, name, created_at, updated_at)
             VALUES ('tenant-2', 'Other Tenant', '2026-01-01T00:00:00.000Z', '2026-01-01T00:00:00.000Z')",
            [],
        )
        .unwrap();
        create_tax_rule(
            &conn,
            "tenant-1",
            &TaxRuleInput { region_label: "Tenant 1 Rate".into(), rate: 5.0, is_default: true },
        )
        .unwrap();

        let tenant2_rules = list_tax_rules(&conn, "tenant-2", true).unwrap();
        assert_eq!(tenant2_rules.len(), 0);
        assert_eq!(resolve_tax_rate(&conn, "tenant-2", None).unwrap(), None);
    }
}
