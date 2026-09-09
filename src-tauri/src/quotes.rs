//! Phase R.3 — Quote Persistence Port.
//!
//! Rust port of `server/ipc_handlers.js`'s `save_quote` / `fetch_quote` cases, backed by
//! the real SQLite `quotes` / `quote_items` tables created by `db.rs`'s migrations
//! (`migrations/001_core_schema.sql`), not the JS reference's in-memory `quotesDb`
//! `Map()`.
//!
//! ## Schema used (verified against the real, attached `001_core_schema.sql` directly —
//! not assumed; see NOTES_R3.md for the honesty statement this comment summarizes)
//! `quotes`: id, tenant_id, user_id, offer_ref, rev_suffix, quote_date, validity_days,
//! customer_name, customer_po_box, customer_city, contact_person, customer_email,
//! customer_ref, salesperson_name, salesperson_phone, subject_text, notes,
//! terms_conditions, rate_basis_text, total_amount, vat_rate, vat_amount, grand_total,
//! status, created_at, updated_at. `UNIQUE(tenant_id, offer_ref, rev_suffix)`.
//! `quote_items`: id, tenant_id, quote_id, item_order, item_description, make_model,
//! quantity, unit_rate, rate_basis, line_total, equipment_spec, created_at.
//!
//! ## Revision-handling decision (flagged in the R.3 Worker brief, Section 3 — NOT
//! silently picked)
//! `server/ipc_handlers.js`'s `save_quote` replaces an existing quote purely by
//! `offer_ref` (`tenantQuotes.findIndex(q => q.offer_ref === quoteData.offer_ref)`),
//! ignoring `rev_suffix` entirely — because its in-memory model never had a `rev_suffix`
//! column to begin with. The real schema's `UNIQUE(tenant_id, offer_ref, rev_suffix)`
//! constraint, plus route-map Phase 5.1's "Document Revision Control System...
//! locking prior approved versions" (Rev.01 → Rev.02 as *separate, retained* rows),
//! point the other way: each revision should be its own row, not an in-place overwrite
//! of the previous revision under the same `offer_ref`.
//!
//! **Decision made here (Worker's inclination, not yet Architect-approved):** `save_quote`
//! upserts on the full `(tenant_id, offer_ref, rev_suffix)` triple, not on `offer_ref`
//! alone. Concretely:
//! - Saving the same `(offer_ref, rev_suffix)` twice updates that one row in place (this
//!   reproduces the JS reference's actual observable behavior in the common case where a
//!   caller re-saves a draft under an unchanged revision suffix — nothing regresses).
//! - Saving a *new* `rev_suffix` under an existing `offer_ref` inserts a new row and
//!   leaves the prior revision's row untouched — satisfying the schema's UNIQUE
//!   constraint and matching Phase 5.1's "lock prior approved versions" intent, which
//!   the JS reference's per-`offer_ref`-only replace would silently violate (it would
//!   overwrite Rev.01 with Rev.02's data instead of retaining both).
//!
//! This is a genuine behavior difference from the JS reference for the specific case of
//! "same offer_ref, different rev_suffix" — flagged explicitly for the Architect to
//! confirm or override, per the brief's instruction not to bury this decision.

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Payload shapes (what the frontend / IPC caller sends and receives)
// ---------------------------------------------------------------------------

/// One line item on a quote. Maps 1:1 to a `quote_items` row minus the columns this
/// module fills in itself (`id`, `tenant_id`, `quote_id`, `item_order`, `created_at`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuoteItemInput {
    pub item_description: String,
    #[serde(default)]
    pub make_model: Option<String>,
    pub quantity: i64,
    pub unit_rate: f64,
    #[serde(default = "default_rate_basis")]
    pub rate_basis: String,
    pub line_total: f64,
    #[serde(default)]
    pub equipment_spec: Option<String>,
}

fn default_rate_basis() -> String {
    "Monthly".to_string()
}

/// Save-quote input. Field names match the real `quotes` table's columns directly
/// (unlike `server/ipc_handlers.js`'s `generate_quote_pdf` contract, which uses a
/// looser, differently-named shape — that mismatch is between the JS reference's own
/// two handlers, not something this port needs to resolve; `save_quote`/`fetch_quote`
/// are the only handlers in scope for R.3).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuoteInput {
    pub offer_ref: String,
    pub rev_suffix: String,
    pub quote_date: String,
    #[serde(default)]
    pub validity_days: Option<i64>,
    pub customer_name: String,
    #[serde(default)]
    pub customer_po_box: Option<String>,
    #[serde(default)]
    pub customer_city: Option<String>,
    #[serde(default)]
    pub contact_person: Option<String>,
    #[serde(default)]
    pub customer_email: Option<String>,
    #[serde(default)]
    pub customer_ref: Option<String>,
    #[serde(default)]
    pub salesperson_name: Option<String>,
    #[serde(default)]
    pub salesperson_phone: Option<String>,
    #[serde(default)]
    pub subject_text: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub terms_conditions: Option<String>,
    #[serde(default)]
    pub rate_basis_text: Option<String>,
    #[serde(default)]
    pub total_amount: Option<f64>,
    #[serde(default)]
    pub vat_rate: Option<f64>,
    #[serde(default)]
    pub vat_amount: Option<f64>,
    #[serde(default)]
    pub grand_total: Option<f64>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub line_items: Vec<QuoteItemInput>,
    /// Phase 4.2: which `tax_rules` row (if any) this quote's `vat_rate` was taken
    /// from. `None` means "no library rule selected" (the pre-Phase-4 behavior:
    /// `vat_rate` above is used as-is). Validated against `tenant_id` in `save_quote`
    /// below at the application layer — see migration 004's doc comment for why this
    /// isn't also a composite DB-level FK.
    #[serde(default)]
    pub tax_rule_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuoteRecord {
    pub id: String,
    pub tenant_id: String,
    pub user_id: String,
    pub offer_ref: String,
    pub rev_suffix: String,
    pub quote_date: String,
    pub validity_days: i64,
    pub customer_name: String,
    pub customer_po_box: Option<String>,
    pub customer_city: Option<String>,
    pub contact_person: Option<String>,
    pub customer_email: Option<String>,
    pub customer_ref: Option<String>,
    pub salesperson_name: Option<String>,
    pub salesperson_phone: Option<String>,
    pub subject_text: Option<String>,
    pub notes: Option<String>,
    pub terms_conditions: Option<String>,
    pub rate_basis_text: Option<String>,
    pub total_amount: f64,
    pub vat_rate: f64,
    pub vat_amount: f64,
    pub grand_total: f64,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
    pub line_items: Vec<QuoteItemRecord>,
    /// Phase 4.2 — see `QuoteInput::tax_rule_id`'s doc comment.
    pub tax_rule_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuoteItemRecord {
    pub id: String,
    pub item_order: i64,
    pub item_description: String,
    pub make_model: Option<String>,
    pub quantity: i64,
    pub unit_rate: f64,
    pub rate_basis: String,
    pub line_total: f64,
    pub equipment_spec: Option<String>,
}

#[derive(Debug)]
pub enum QuoteError {
    InvalidPayload(String),
    /// Phase 5.1: the target row's `status` is in `LOCKED_STATUSES` — refuse the
    /// modification outright rather than silently overwriting an approved quote's
    /// history. Carries a message naming the current status and pointing the caller at
    /// `branch_new_revision` instead of `save_quote`.
    Locked(String),
    Db(rusqlite::Error),
}

impl From<rusqlite::Error> for QuoteError {
    fn from(e: rusqlite::Error) -> Self {
        QuoteError::Db(e)
    }
}

// ---------------------------------------------------------------------------
// IDs / timestamps
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// save_quote — port of ipc_handlers.js's `save_quote` case
// ---------------------------------------------------------------------------

/// Saves a quote for `tenant_id`/`user_id`. Upserts on `(tenant_id, offer_ref,
/// rev_suffix)` — see module doc for why this differs from the JS reference's
/// offer_ref-only replace, and how it differs.
///
/// Tenant isolation: every statement in this function is scoped by `tenant_id`,
/// either as a bound parameter on the WHERE/UNIQUE-conflict target or as a column
/// written on insert. There is no code path here that can read or write another
/// tenant's row.
pub fn save_quote(
    conn: &Connection,
    tenant_id: &str,
    user_id: &str,
    quote: &QuoteInput,
) -> Result<String, QuoteError> {
    if quote.offer_ref.trim().is_empty() {
        return Err(QuoteError::InvalidPayload(
            "offer_ref is required".to_string(),
        ));
    }
    if quote.rev_suffix.trim().is_empty() {
        return Err(QuoteError::InvalidPayload(
            "rev_suffix is required".to_string(),
        ));
    }

    // Phase 4.2 / migration 004: `tax_rule_id` has no composite (id, tenant_id) DB-level
    // FK (see migration 004's doc comment for why), so tenant ownership is checked here,
    // in application code, before it's ever written to a row — the same "flag it, don't
    // silently accept a weaker guarantee" approach the migration comment promises.
    if let Some(rule_id) = &quote.tax_rule_id {
        let owned: Option<i64> = conn
            .query_row(
                "SELECT 1 FROM tax_rules WHERE id = ?1 AND tenant_id = ?2",
                params![rule_id, tenant_id],
                |row| row.get(0),
            )
            .optional()?;
        if owned.is_none() {
            return Err(QuoteError::InvalidPayload(format!(
                "tax_rule_id '{}' does not belong to this tenant",
                rule_id
            )));
        }
    }

    // Look up an existing row for this exact (tenant_id, offer_ref, rev_suffix) triple
    // — this is the "same revision re-saved" case, updated in place. A different
    // rev_suffix under the same offer_ref is NOT matched here, so it falls through to
    // the INSERT branch below and becomes a new row (the Section-3 decision).
    let existing_id: Option<String> = conn
        .query_row(
            "SELECT id FROM quotes WHERE tenant_id = ?1 AND offer_ref = ?2 AND rev_suffix = ?3",
            params![tenant_id, quote.offer_ref, quote.rev_suffix],
            |row| row.get(0),
        )
        .optional()?;

    // Phase 5.1: if a row already exists for this exact (offer_ref, rev_suffix), refuse
    // to touch it further once its status is locked — this is the actual enforcement
    // behind "locking prior approved versions" (module doc, Phase 5.1 section). Checked
    // before the transaction does any writes, so a locked quote is rejected with zero
    // side effects, not partially modified then rolled back.
    if let Some(id) = &existing_id {
        let current_status: String = conn.query_row(
            "SELECT status FROM quotes WHERE id = ?1 AND tenant_id = ?2",
            params![id, tenant_id],
            |row| row.get(0),
        )?;
        if LOCKED_STATUSES.contains(&current_status.as_str()) {
            return Err(QuoteError::Locked(format!(
                "Quote '{}' {} is locked (status = '{}') and cannot be modified further. \
                 Use branch_new_revision to create a new revision instead.",
                quote.offer_ref, quote.rev_suffix, current_status
            )));
        }
    }

    // Phase R.5: wraps the whole save (quote row update/insert + line-item
    // delete-then-reinsert) in one real SQL transaction — closes the item flagged
    // (non-blocking) since R.3/Session 20: a crash or error between the DELETE and the
    // final INSERT of line_items previously could have left a quote row with zero line
    // items, silently. `Transaction`'s Drop rolls back automatically unless `commit()`
    // is reached, so every early `?`-return in this function (including from
    // `now_timestamp`, the UPDATE/INSERT calls, and the per-item INSERT loop) now rolls
    // back cleanly instead of leaving partial state.
    let tx = conn.unchecked_transaction()?;

    let ts = now_timestamp(conn)?;

    let quote_id = match &existing_id {
        Some(id) => id.clone(),
        None => generate_id(),
    };

    if let Some(id) = &existing_id {
        conn.execute(
            "UPDATE quotes SET
                quote_date = ?1, validity_days = ?2, customer_name = ?3,
                customer_po_box = ?4, customer_city = ?5, contact_person = ?6,
                customer_email = ?7, customer_ref = ?8, salesperson_name = ?9,
                salesperson_phone = ?10, subject_text = ?11, notes = ?12,
                terms_conditions = ?13, rate_basis_text = ?14, total_amount = ?15,
                vat_rate = ?16, vat_amount = ?17, grand_total = ?18, status = ?19,
                updated_at = ?20, tax_rule_id = ?21
             WHERE id = ?22 AND tenant_id = ?23",
            params![
                quote.quote_date,
                quote.validity_days.unwrap_or(30),
                quote.customer_name,
                quote.customer_po_box,
                quote.customer_city,
                quote.contact_person,
                quote.customer_email,
                quote.customer_ref,
                quote.salesperson_name,
                quote.salesperson_phone,
                quote.subject_text,
                quote.notes,
                quote.terms_conditions,
                quote.rate_basis_text,
                quote.total_amount.unwrap_or(0.0),
                quote.vat_rate.unwrap_or(5.0),
                quote.vat_amount.unwrap_or(0.0),
                quote.grand_total.unwrap_or(0.0),
                quote.status.clone().unwrap_or_else(|| "Draft".to_string()),
                ts,
                quote.tax_rule_id,
                id,
                tenant_id,
            ],
        )?;

        // Line items are replaced wholesale on update (delete + reinsert) rather than
        // diffed — matches the JS reference's own behavior of overwriting the whole
        // `line_items` array on every save, and keeps `item_order` trivially correct.
        conn.execute(
            "DELETE FROM quote_items WHERE quote_id = ?1 AND tenant_id = ?2",
            params![id, tenant_id],
        )?;
    } else {
        conn.execute(
            "INSERT INTO quotes (
                id, tenant_id, user_id, offer_ref, rev_suffix, quote_date, validity_days,
                customer_name, customer_po_box, customer_city, contact_person,
                customer_email, customer_ref, salesperson_name, salesperson_phone,
                subject_text, notes, terms_conditions, rate_basis_text, total_amount,
                vat_rate, vat_amount, grand_total, status, tax_rule_id, created_at, updated_at
             ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16,
                ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?26
             )",
            params![
                quote_id,
                tenant_id,
                user_id,
                quote.offer_ref,
                quote.rev_suffix,
                quote.quote_date,
                quote.validity_days.unwrap_or(30),
                quote.customer_name,
                quote.customer_po_box,
                quote.customer_city,
                quote.contact_person,
                quote.customer_email,
                quote.customer_ref,
                quote.salesperson_name,
                quote.salesperson_phone,
                quote.subject_text,
                quote.notes,
                quote.terms_conditions,
                quote.rate_basis_text,
                quote.total_amount.unwrap_or(0.0),
                quote.vat_rate.unwrap_or(5.0),
                quote.vat_amount.unwrap_or(0.0),
                quote.grand_total.unwrap_or(0.0),
                quote.status.clone().unwrap_or_else(|| "Draft".to_string()),
                quote.tax_rule_id,
                ts,
            ],
        )?;
    }

    for (idx, item) in quote.line_items.iter().enumerate() {
        let item_ts = now_timestamp(conn)?;
        conn.execute(
            "INSERT INTO quote_items (
                id, tenant_id, quote_id, item_order, item_description, make_model,
                quantity, unit_rate, rate_basis, line_total, equipment_spec, created_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                generate_id(),
                tenant_id,
                quote_id,
                (idx as i64) + 1,
                item.item_description,
                item.make_model,
                item.quantity,
                item.unit_rate,
                item.rate_basis,
                item.line_total,
                item.equipment_spec,
                item_ts,
            ],
        )?;
    }

    tx.commit()?;

    Ok(quote_id)
}

// ---------------------------------------------------------------------------
// fetch_quote — port of ipc_handlers.js's `fetch_quote` case
// ---------------------------------------------------------------------------

/// Fetches a quote scoped to `tenant_id`. `rev_suffix`:
/// - `Some(suffix)` — fetch that exact revision (exact `(offer_ref, rev_suffix)` match).
/// - `None` — matches the JS reference's `offer_ref`-only lookup; since the real schema
///   can now hold multiple revisions per `offer_ref` (see module doc), this returns the
///   most recently updated row for that `offer_ref`, not an arbitrary one.
///
/// Tenant isolation: the query is always scoped by `tenant_id` in the WHERE clause — a
/// quote saved by tenant A can never be returned for tenant B's session, regardless of
/// what `offer_ref`/`rev_suffix` tenant B's session queries for.
pub fn fetch_quote(
    conn: &Connection,
    tenant_id: &str,
    offer_ref: &str,
    rev_suffix: Option<&str>,
) -> Result<Option<QuoteRecord>, QuoteError> {
    let row = match rev_suffix {
        Some(rev) => conn
            .query_row(
                "SELECT id, tenant_id, user_id, offer_ref, rev_suffix, quote_date,
                        validity_days, customer_name, customer_po_box, customer_city,
                        contact_person, customer_email, customer_ref, salesperson_name,
                        salesperson_phone, subject_text, notes, terms_conditions,
                        rate_basis_text, total_amount, vat_rate, vat_amount, grand_total,
                        status, tax_rule_id, created_at, updated_at
                 FROM quotes
                 WHERE tenant_id = ?1 AND offer_ref = ?2 AND rev_suffix = ?3",
                params![tenant_id, offer_ref, rev],
                row_to_quote,
            )
            .optional()?,
        None => conn
            .query_row(
                "SELECT id, tenant_id, user_id, offer_ref, rev_suffix, quote_date,
                        validity_days, customer_name, customer_po_box, customer_city,
                        contact_person, customer_email, customer_ref, salesperson_name,
                        salesperson_phone, subject_text, notes, terms_conditions,
                        rate_basis_text, total_amount, vat_rate, vat_amount, grand_total,
                        status, tax_rule_id, created_at, updated_at
                 FROM quotes
                 WHERE tenant_id = ?1 AND offer_ref = ?2
                 ORDER BY updated_at DESC, rowid DESC
                 LIMIT 1",
                params![tenant_id, offer_ref],
                row_to_quote,
            )
            .optional()?,
    };

    let mut quote = match row {
        Some(q) => q,
        None => return Ok(None),
    };

    let mut stmt = conn.prepare(
        "SELECT id, item_order, item_description, make_model, quantity, unit_rate,
                rate_basis, line_total, equipment_spec
         FROM quote_items
         WHERE tenant_id = ?1 AND quote_id = ?2
         ORDER BY item_order ASC",
    )?;
    let items = stmt
        .query_map(params![tenant_id, quote.id], |row| {
            Ok(QuoteItemRecord {
                id: row.get(0)?,
                item_order: row.get(1)?,
                item_description: row.get(2)?,
                make_model: row.get(3)?,
                quantity: row.get(4)?,
                unit_rate: row.get(5)?,
                rate_basis: row.get(6)?,
                line_total: row.get(7)?,
                equipment_spec: row.get(8)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    quote.line_items = items;
    Ok(Some(quote))
}

// ---------------------------------------------------------------------------
// Phase 5.1 — Document Revision Control System
// ---------------------------------------------------------------------------

/// Statuses that lock a quote row against further `save_quote` modification. Flagged
/// explicitly, per this project's practice, rather than silently scoped: Phase 5.1's
/// own Completion Check only names "approved" quotes, but Phase 5.2 (Client LPO &
/// Booking Status, not yet built) describes a longer lifecycle — "Draft → Issued → LPO
/// Confirmed → Job Booked" — and every one of those post-Draft states implies the same
/// "don't silently rewrite this" guarantee an approved quote needs. Defined here now,
/// as the single source of truth Phase 5.2 should extend rather than re-derive, so the
/// two phases don't end up with two different lists of "locked" statuses to keep in
/// sync (the exact class of drift risk Phase R's own module doc already warned about
/// for `CANONICAL_MODULE_KEYS`).
pub const LOCKED_STATUSES: &[&str] = &["Approved", "Issued", "LPO Confirmed", "Job Booked"];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuoteRevisionRecord {
    pub id: String,
    pub quote_id: String,
    pub revision_number: i64,
    pub status_at_snapshot: String,
    pub created_by: String,
    pub created_at: String,
}

/// Fetches a quote by its real row `id` (not `offer_ref`/`rev_suffix`) — needed by
/// `branch_new_revision` to read the exact locked row being branched from, since two
/// different revisions can share the same `offer_ref` and only differ by `rev_suffix`.
/// Tenant isolation: scoped by `tenant_id`, same as `fetch_quote`.
fn fetch_quote_by_id(
    conn: &Connection,
    tenant_id: &str,
    quote_id: &str,
) -> Result<Option<QuoteRecord>, QuoteError> {
    let row = conn
        .query_row(
            "SELECT id, tenant_id, user_id, offer_ref, rev_suffix, quote_date,
                    validity_days, customer_name, customer_po_box, customer_city,
                    contact_person, customer_email, customer_ref, salesperson_name,
                    salesperson_phone, subject_text, notes, terms_conditions,
                    rate_basis_text, total_amount, vat_rate, vat_amount, grand_total,
                    status, tax_rule_id, created_at, updated_at
             FROM quotes
             WHERE tenant_id = ?1 AND id = ?2",
            params![tenant_id, quote_id],
            row_to_quote,
        )
        .optional()?;

    let mut quote = match row {
        Some(q) => q,
        None => return Ok(None),
    };

    let mut stmt = conn.prepare(
        "SELECT id, item_order, item_description, make_model, quantity, unit_rate,
                rate_basis, line_total, equipment_spec
         FROM quote_items
         WHERE tenant_id = ?1 AND quote_id = ?2
         ORDER BY item_order ASC",
    )?;
    let items = stmt
        .query_map(params![tenant_id, quote.id], |row| {
            Ok(QuoteItemRecord {
                id: row.get(0)?,
                item_order: row.get(1)?,
                item_description: row.get(2)?,
                make_model: row.get(3)?,
                quantity: row.get(4)?,
                unit_rate: row.get(5)?,
                rate_basis: row.get(6)?,
                line_total: row.get(7)?,
                equipment_spec: row.get(8)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    quote.line_items = items;
    Ok(Some(quote))
}

/// Lists the audit-logged revision history for `quote_id`, oldest first.
pub fn fetch_quote_revisions(
    conn: &Connection,
    tenant_id: &str,
    quote_id: &str,
) -> Result<Vec<QuoteRevisionRecord>, QuoteError> {
    let mut stmt = conn.prepare(
        "SELECT id, quote_id, revision_number, status_at_snapshot, created_by, created_at
         FROM quote_revisions
         WHERE tenant_id = ?1 AND quote_id = ?2
         ORDER BY revision_number ASC",
    )?;
    let rows = stmt
        .query_map(params![tenant_id, quote_id], |row| {
            Ok(QuoteRevisionRecord {
                id: row.get(0)?,
                quote_id: row.get(1)?,
                revision_number: row.get(2)?,
                status_at_snapshot: row.get(3)?,
                created_by: row.get(4)?,
                created_at: row.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Phase 5.1's actual "modifying a quote creates an audit-logged revision history"
/// mechanism. Takes a locked quote (`from_quote_id`), snapshots its full current state
/// (including line items) into `quote_revisions`, then creates a brand-new `quotes` row
/// under the SAME `offer_ref` with `new_rev_suffix`, copying every editable field and
/// line item forward, with `status` reset to `"Draft"` (i.e. editable again via the
/// normal `save_quote` path) and `tax_rule_id` carried forward unchanged.
///
/// Returns `(new_quote_id, revision_id)`.
///
/// Preconditions, both enforced (not assumed):
/// - `from_quote_id` must exist for `tenant_id` (else `InvalidPayload`).
/// - `from_quote_id`'s current status must actually be in `LOCKED_STATUSES` — branching
///   only makes sense from an approved baseline; a still-Draft quote should just be
///   edited in place via `save_quote` (else `InvalidPayload`, distinct from the
///   `Locked` variant `save_quote` returns — this function's precondition is the
///   opposite direction of that check).
/// - `new_rev_suffix` must not already exist under this `offer_ref` for this tenant
///   (else `InvalidPayload` — reuses `save_quote`'s own uniqueness guarantee rather
///   than relying on the UNIQUE constraint to surface a less specific DB error).
pub fn branch_new_revision(
    conn: &Connection,
    tenant_id: &str,
    user_id: &str,
    from_quote_id: &str,
    new_rev_suffix: &str,
) -> Result<(String, String), QuoteError> {
    if new_rev_suffix.trim().is_empty() {
        return Err(QuoteError::InvalidPayload(
            "new_rev_suffix is required".to_string(),
        ));
    }

    let source = match fetch_quote_by_id(conn, tenant_id, from_quote_id)? {
        Some(q) => q,
        None => {
            return Err(QuoteError::InvalidPayload(format!(
                "Quote id '{}' not found for this tenant",
                from_quote_id
            )))
        }
    };

    if !LOCKED_STATUSES.contains(&source.status.as_str()) {
        return Err(QuoteError::InvalidPayload(format!(
            "Quote '{}' {} is not locked (status = '{}') — edit it directly via \
             save_quote instead of branching a new revision.",
            source.offer_ref, source.rev_suffix, source.status
        )));
    }

    let already_exists: Option<i64> = conn
        .query_row(
            "SELECT 1 FROM quotes WHERE tenant_id = ?1 AND offer_ref = ?2 AND rev_suffix = ?3",
            params![tenant_id, source.offer_ref, new_rev_suffix],
            |row| row.get(0),
        )
        .optional()?;
    if already_exists.is_some() {
        return Err(QuoteError::InvalidPayload(format!(
            "Revision '{}' already exists for offer_ref '{}'",
            new_rev_suffix, source.offer_ref
        )));
    }

    let tx = conn.unchecked_transaction()?;
    let ts = now_timestamp(&tx)?;

    // Snapshot the locked source row's full state (real audit trail, not a live
    // reference — see migration 005's doc comment) before creating anything new.
    let next_revision_number: i64 = tx
        .query_row(
            "SELECT COALESCE(MAX(revision_number), 0) + 1 FROM quote_revisions
             WHERE tenant_id = ?1 AND quote_id = ?2",
            params![tenant_id, from_quote_id],
            |row| row.get(0),
        )
        .unwrap_or(1);
    let snapshot_json = serde_json::to_string(&source)
        .map_err(|e| QuoteError::InvalidPayload(format!("failed to serialize snapshot: {}", e)))?;
    let revision_id = generate_id();
    tx.execute(
        "INSERT INTO quote_revisions (
            id, tenant_id, quote_id, revision_number, snapshot_json, status_at_snapshot,
            created_by, created_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            revision_id,
            tenant_id,
            from_quote_id,
            next_revision_number,
            snapshot_json,
            source.status,
            user_id,
            ts,
        ],
    )?;

    // Create the new, editable Draft row under the same offer_ref — copying forward
    // every field save_quote itself accepts, matching its own INSERT column list
    // exactly so the two never silently diverge in shape.
    let new_quote_id = generate_id();
    tx.execute(
        "INSERT INTO quotes (
            id, tenant_id, user_id, offer_ref, rev_suffix, quote_date, validity_days,
            customer_name, customer_po_box, customer_city, contact_person,
            customer_email, customer_ref, salesperson_name, salesperson_phone,
            subject_text, notes, terms_conditions, rate_basis_text, total_amount,
            vat_rate, vat_amount, grand_total, status, tax_rule_id, created_at, updated_at
         ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16,
            ?17, ?18, ?19, ?20, ?21, ?22, ?23, 'Draft', ?24, ?25, ?25
         )",
        params![
            new_quote_id,
            tenant_id,
            user_id,
            source.offer_ref,
            new_rev_suffix,
            source.quote_date,
            source.validity_days,
            source.customer_name,
            source.customer_po_box,
            source.customer_city,
            source.contact_person,
            source.customer_email,
            source.customer_ref,
            source.salesperson_name,
            source.salesperson_phone,
            source.subject_text,
            source.notes,
            source.terms_conditions,
            source.rate_basis_text,
            source.total_amount,
            source.vat_rate,
            source.vat_amount,
            source.grand_total,
            source.tax_rule_id,
            ts,
        ],
    )?;

    for (idx, item) in source.line_items.iter().enumerate() {
        let item_ts = now_timestamp(&tx)?;
        tx.execute(
            "INSERT INTO quote_items (
                id, tenant_id, quote_id, item_order, item_description, make_model,
                quantity, unit_rate, rate_basis, line_total, equipment_spec, created_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                generate_id(),
                tenant_id,
                new_quote_id,
                (idx as i64) + 1,
                item.item_description,
                item.make_model,
                item.quantity,
                item.unit_rate,
                item.rate_basis,
                item.line_total,
                item.equipment_spec,
                item_ts,
            ],
        )?;
    }

    // quote_compliance_terms (Phase 4.1) is deliberately NOT copied forward here —
    // flagged rather than silently dropped: whether a new Draft revision should inherit
    // the prior revision's selected compliance clauses automatically, or start empty
    // and require re-selecting them, is a product decision this function doesn't make
    // unilaterally. Leaving it empty (the current behavior) is the more conservative
    // choice — a carried-forward clause silently applying to a revised quote without
    // the user re-confirming it felt riskier than requiring an explicit re-selection.

    tx.commit()?;
    Ok((new_quote_id, revision_id))
}

fn row_to_quote(row: &rusqlite::Row) -> rusqlite::Result<QuoteRecord> {
    Ok(QuoteRecord {
        id: row.get(0)?,
        tenant_id: row.get(1)?,
        user_id: row.get(2)?,
        offer_ref: row.get(3)?,
        rev_suffix: row.get(4)?,
        quote_date: row.get(5)?,
        validity_days: row.get(6)?,
        customer_name: row.get(7)?,
        customer_po_box: row.get(8)?,
        customer_city: row.get(9)?,
        contact_person: row.get(10)?,
        customer_email: row.get(11)?,
        customer_ref: row.get(12)?,
        salesperson_name: row.get(13)?,
        salesperson_phone: row.get(14)?,
        subject_text: row.get(15)?,
        notes: row.get(16)?,
        terms_conditions: row.get(17)?,
        rate_basis_text: row.get(18)?,
        total_amount: row.get(19)?,
        vat_rate: row.get(20)?,
        vat_amount: row.get(21)?,
        grand_total: row.get(22)?,
        status: row.get(23)?,
        tax_rule_id: row.get(24)?,
        created_at: row.get(25)?,
        updated_at: row.get(26)?,
        line_items: Vec::new(),
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
//
// NOTE (honest assessment — see NOTES_R3.md): these tests are written against an
// in-memory SQLite connection whose schema is created by executing the REAL, attached
// `migrations/001_core_schema.sql` file verbatim (Section A + Section B, the SQLite
// trigger section) via `include_str!`, the same pattern `db.rs` uses for the real app —
// not a hand-typed mock schema. This was the exact category of gap the R.2 audit found
// (a test fixture matching an assumed schema, not the real one). No `cargo`/`rustc`
// toolchain was available in this Worker session to actually execute `cargo test`
// against these — see NOTES_R3.md for the explicit statement of what was and wasn't run.

#[cfg(test)]
mod tests {
    use super::*;

    const REAL_SCHEMA_SQL: &str = include_str!("../../migrations/001_core_schema.sql");
    const REAL_SCHEMA_003: &str = include_str!("../../migrations/003_compliance_terms.sql");
    const REAL_SCHEMA_004: &str = include_str!("../../migrations/004_quotes_tax_rule.sql");
    const REAL_SCHEMA_005: &str = include_str!("../../migrations/005_quote_revisions.sql");

    fn test_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        // Real schema file mixes Section A (neutral DDL) + Section B (SQLite triggers)
        // + Section C (commented-out Postgres DDL) — safe to execute verbatim against
        // SQLite, same as db.rs's run_migrations does against the real app database.
        conn.execute_batch(REAL_SCHEMA_SQL).unwrap();
        // Phase 4.2: save_quote now validates tax_rule_id against tax_rules (003) and
        // writes quotes.tax_rule_id (004) — both must be applied for these tests to
        // exercise the same schema shape the real app runs against, not a stale subset.
        conn.execute_batch(REAL_SCHEMA_003).unwrap();
        conn.execute_batch(REAL_SCHEMA_004).unwrap();
        conn.execute_batch(REAL_SCHEMA_005).unwrap();
        seed_tenant_and_user(&conn, "tenant-1", "user-1");
        conn
    }

    fn seed_tenant_and_user(conn: &Connection, tenant_id: &str, user_id: &str) {
        conn.execute(
            "INSERT INTO tenants (id, name, created_at, updated_at)
             VALUES (?1, 'Test Tenant', '2026-01-01T00:00:00.000Z', '2026-01-01T00:00:00.000Z')",
            params![tenant_id],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO users (id, tenant_id, email, password_hash, full_name, role, created_at, updated_at)
             VALUES (?1, ?2, ?3, 'hash', 'Test User', 'staff', '2026-01-01T00:00:00.000Z', '2026-01-01T00:00:00.000Z')",
            params![user_id, tenant_id, format!("{}@example.com", user_id)],
        )
        .unwrap();
    }

    fn sample_quote(offer_ref: &str, rev_suffix: &str) -> QuoteInput {
        QuoteInput {
            offer_ref: offer_ref.to_string(),
            rev_suffix: rev_suffix.to_string(),
            quote_date: "2026-03-01".to_string(),
            validity_days: Some(30),
            customer_name: "Acme Construction LLC".to_string(),
            customer_po_box: Some("12345".to_string()),
            customer_city: Some("Dubai".to_string()),
            contact_person: Some("John Doe".to_string()),
            customer_email: None,
            customer_ref: None,
            salesperson_name: Some("Jane Sales".to_string()),
            salesperson_phone: None,
            subject_text: Some("Equipment rental quote".to_string()),
            notes: None,
            terms_conditions: None,
            rate_basis_text: None,
            total_amount: Some(1000.0),
            vat_rate: Some(5.0),
            vat_amount: Some(50.0),
            grand_total: Some(1050.0),
            status: Some("Draft".to_string()),
            tax_rule_id: None,
            line_items: vec![QuoteItemInput {
                item_description: "Excavator, 20T".to_string(),
                make_model: Some("Cat 320".to_string()),
                quantity: 1,
                unit_rate: 1000.0,
                rate_basis: "Monthly".to_string(),
                line_total: 1000.0,
                equipment_spec: Some("20 tonne, tracked".to_string()),
            }],
        }
    }

    #[test]
    fn save_then_fetch_round_trips() {
        let conn = test_conn();
        let quote = sample_quote("QN-EH/211/2026", "Rev.01");
        let id = save_quote(&conn, "tenant-1", "user-1", &quote).expect("save should succeed");
        assert!(!id.is_empty());

        let fetched = fetch_quote(&conn, "tenant-1", "QN-EH/211/2026", None)
            .expect("fetch should succeed")
            .expect("quote should exist");

        assert_eq!(fetched.offer_ref, "QN-EH/211/2026");
        assert_eq!(fetched.rev_suffix, "Rev.01");
        assert_eq!(fetched.customer_name, "Acme Construction LLC");
        assert_eq!(fetched.line_items.len(), 1);
        assert_eq!(fetched.line_items[0].item_description, "Excavator, 20T");
        assert_eq!(fetched.line_items[0].quantity, 1);
    }

    #[test]
    fn item_order_is_1_indexed_not_0_indexed() {
        let conn = test_conn();
        let mut quote = sample_quote("QN-EH/999/2026", "Rev.01");
        quote.line_items.push(QuoteItemInput {
            item_description: "Generator, 100kVA".to_string(),
            make_model: Some("Cummins".to_string()),
            quantity: 1,
            unit_rate: 500.0,
            rate_basis: "Monthly".to_string(),
            line_total: 500.0,
            equipment_spec: None,
        });
        quote.line_items.push(QuoteItemInput {
            item_description: "Compressor".to_string(),
            make_model: Some("Atlas Copco".to_string()),
            quantity: 1,
            unit_rate: 300.0,
            rate_basis: "Monthly".to_string(),
            line_total: 300.0,
            equipment_spec: None,
        });

        save_quote(&conn, "tenant-1", "user-1", &quote).expect("save should succeed");
        let fetched = fetch_quote(&conn, "tenant-1", "QN-EH/999/2026", None)
            .expect("fetch should succeed")
            .expect("quote should exist");

        assert_eq!(fetched.line_items.len(), 3);
        assert_eq!(fetched.line_items[0].item_order, 1, "first item must be item_order 1, not 0");
        assert_eq!(fetched.line_items[1].item_order, 2);
        assert_eq!(fetched.line_items[2].item_order, 3);
    }

    #[test]
    fn saving_same_offer_ref_and_rev_suffix_updates_in_place() {
        let conn = test_conn();
        let mut quote = sample_quote("QN-EH/300/2026", "Rev.01");
        let id1 = save_quote(&conn, "tenant-1", "user-1", &quote).unwrap();

        quote.customer_name = "Updated Customer Name".to_string();
        quote.line_items[0].quantity = 2;
        let id2 = save_quote(&conn, "tenant-1", "user-1", &quote).unwrap();

        assert_eq!(id1, id2, "same (offer_ref, rev_suffix) must update the same row");

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM quotes WHERE tenant_id = 'tenant-1' AND offer_ref = 'QN-EH/300/2026'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1, "must not create a second row for the same revision");

        let fetched = fetch_quote(&conn, "tenant-1", "QN-EH/300/2026", None)
            .unwrap()
            .unwrap();
        assert_eq!(fetched.customer_name, "Updated Customer Name");
        assert_eq!(fetched.line_items[0].quantity, 2);
    }

    #[test]
    fn saving_new_rev_suffix_creates_a_separate_row_and_retains_the_prior_revision() {
        let conn = test_conn();
        let rev1 = sample_quote("QN-EH/400/2026", "Rev.01");
        save_quote(&conn, "tenant-1", "user-1", &rev1).unwrap();

        let mut rev2 = sample_quote("QN-EH/400/2026", "Rev.02");
        rev2.customer_name = "Revised Customer Name".to_string();
        save_quote(&conn, "tenant-1", "user-1", &rev2).unwrap();

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM quotes WHERE tenant_id = 'tenant-1' AND offer_ref = 'QN-EH/400/2026'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 2, "each rev_suffix must be its own row per the schema's UNIQUE constraint");

        // Rev.01 must still be fetchable, unmodified — this is the "locking prior
        // approved versions" behavior the JS reference's offer_ref-only replace would
        // have silently broken.
        let fetched_rev1 = fetch_quote(&conn, "tenant-1", "QN-EH/400/2026", Some("Rev.01"))
            .unwrap()
            .unwrap();
        assert_eq!(fetched_rev1.customer_name, "Acme Construction LLC");

        let fetched_rev2 = fetch_quote(&conn, "tenant-1", "QN-EH/400/2026", Some("Rev.02"))
            .unwrap()
            .unwrap();
        assert_eq!(fetched_rev2.customer_name, "Revised Customer Name");

        // No-rev_suffix lookup returns the most recently updated row (Rev.02).
        let fetched_latest = fetch_quote(&conn, "tenant-1", "QN-EH/400/2026", None)
            .unwrap()
            .unwrap();
        assert_eq!(fetched_latest.rev_suffix, "Rev.02");
    }

    #[test]
    fn fetch_returns_none_for_unknown_offer_ref() {
        let conn = test_conn();
        let result = fetch_quote(&conn, "tenant-1", "DOES-NOT-EXIST", None).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn cross_tenant_fetch_is_isolated() {
        let conn = test_conn();
        seed_tenant_and_user(&conn, "tenant-2", "user-2");

        let quote = sample_quote("QN-EH/500/2026", "Rev.01");
        save_quote(&conn, "tenant-1", "user-1", &quote).unwrap();

        // Same offer_ref/rev_suffix, queried under a DIFFERENT tenant's session, must
        // not return tenant-1's row.
        let cross_tenant_result = fetch_quote(&conn, "tenant-2", "QN-EH/500/2026", None).unwrap();
        assert!(
            cross_tenant_result.is_none(),
            "tenant-2 must not be able to fetch tenant-1's quote"
        );

        // tenant-1 can still fetch its own quote.
        let own_result = fetch_quote(&conn, "tenant-1", "QN-EH/500/2026", None).unwrap();
        assert!(own_result.is_some());
    }

    // ---- Phase 5.1: locking + revision branching ----

    #[test]
    fn save_quote_is_rejected_once_locked() {
        let conn = test_conn();
        let mut quote = sample_quote("QN-EH/800/2026", "Rev.01");
        quote.status = Some("Draft".to_string());
        save_quote(&conn, "tenant-1", "user-1", &quote).unwrap();

        // Transition into a locked status -- still allowed, since the row was Draft
        // (unlocked) at the moment this call started.
        quote.status = Some("Approved".to_string());
        save_quote(&conn, "tenant-1", "user-1", &quote).unwrap();

        // Any further save_quote on this exact (offer_ref, rev_suffix) must now be
        // refused, even a trivial one.
        quote.customer_name = "Attempted Edit After Approval".to_string();
        let result = save_quote(&conn, "tenant-1", "user-1", &quote);
        assert!(matches!(result, Err(QuoteError::Locked(_))));

        // The stored row must be unaffected by the rejected attempt.
        let fetched = fetch_quote(&conn, "tenant-1", "QN-EH/800/2026", None)
            .unwrap()
            .unwrap();
        assert_ne!(fetched.customer_name, "Attempted Edit After Approval");
    }

    #[test]
    fn branch_new_revision_rejects_unlocked_source() {
        let conn = test_conn();
        let quote = sample_quote("QN-EH/801/2026", "Rev.01");
        let quote_id = save_quote(&conn, "tenant-1", "user-1", &quote).unwrap();

        let result = branch_new_revision(&conn, "tenant-1", "user-1", &quote_id, "Rev.02");
        assert!(
            matches!(result, Err(QuoteError::InvalidPayload(_))),
            "branching from a still-Draft (unlocked) quote must be rejected"
        );
    }

    #[test]
    fn branch_new_revision_creates_snapshot_and_editable_draft_copy() {
        let conn = test_conn();
        let mut quote = sample_quote("QN-EH/802/2026", "Rev.01");
        quote.line_items.push(QuoteItemInput {
            item_description: "Generator".to_string(),
            make_model: Some("Cummins".to_string()),
            quantity: 2,
            unit_rate: 400.0,
            rate_basis: "Weekly".to_string(),
            line_total: 800.0,
            equipment_spec: None,
        });
        let quote_id = save_quote(&conn, "tenant-1", "user-1", &quote).unwrap();

        quote.status = Some("Approved".to_string());
        save_quote(&conn, "tenant-1", "user-1", &quote).unwrap();

        let (new_quote_id, revision_id) =
            branch_new_revision(&conn, "tenant-1", "user-1", &quote_id, "Rev.02").unwrap();
        assert_ne!(new_quote_id, quote_id);
        assert!(!revision_id.is_empty());

        // The new row is a real, independent, editable Draft under the same offer_ref.
        let new_quote = fetch_quote(&conn, "tenant-1", "QN-EH/802/2026", Some("Rev.02"))
            .unwrap()
            .unwrap();
        assert_eq!(new_quote.status, "Draft");
        assert_eq!(new_quote.offer_ref, "QN-EH/802/2026");
        assert_eq!(new_quote.line_items.len(), quote.line_items.len());
        assert!(
            new_quote.line_items.iter().any(|i| i.item_description == "Generator"),
            "branched copy must include the extra line item pushed onto the source quote"
        );

        // The new Draft row can be freely edited (it isn't locked).
        let mut editable = quote.clone();
        editable.rev_suffix = "Rev.02".to_string();
        editable.customer_name = "Edited After Branching".to_string();
        editable.status = Some("Draft".to_string());
        save_quote(&conn, "tenant-1", "user-1", &editable).unwrap();
        let refetched = fetch_quote(&conn, "tenant-1", "QN-EH/802/2026", Some("Rev.02"))
            .unwrap()
            .unwrap();
        assert_eq!(refetched.customer_name, "Edited After Branching");

        // The original locked Rev.01 row is untouched.
        let original = fetch_quote(&conn, "tenant-1", "QN-EH/802/2026", Some("Rev.01"))
            .unwrap()
            .unwrap();
        assert_eq!(original.status, "Approved");
        assert_ne!(original.customer_name, "Edited After Branching");

        // A real, audit-logged revision snapshot was recorded.
        let revisions = fetch_quote_revisions(&conn, "tenant-1", &quote_id).unwrap();
        assert_eq!(revisions.len(), 1);
        assert_eq!(revisions[0].revision_number, 1);
        assert_eq!(revisions[0].status_at_snapshot, "Approved");
        assert_eq!(revisions[0].created_by, "user-1");
    }

    #[test]
    fn branch_new_revision_rejects_duplicate_rev_suffix() {
        let conn = test_conn();
        let mut quote = sample_quote("QN-EH/803/2026", "Rev.01");
        let quote_id = save_quote(&conn, "tenant-1", "user-1", &quote).unwrap();
        quote.status = Some("Approved".to_string());
        save_quote(&conn, "tenant-1", "user-1", &quote).unwrap();

        // Rev.02 already exists as a separate quote under the same offer_ref.
        let mut rev2 = quote.clone();
        rev2.rev_suffix = "Rev.02".to_string();
        rev2.status = Some("Draft".to_string());
        save_quote(&conn, "tenant-1", "user-1", &rev2).unwrap();

        let result = branch_new_revision(&conn, "tenant-1", "user-1", &quote_id, "Rev.02");
        assert!(matches!(result, Err(QuoteError::InvalidPayload(_))));
    }

    #[test]
    fn branch_new_revision_is_tenant_isolated() {
        let conn = test_conn();
        seed_tenant_and_user(&conn, "tenant-2", "user-2");
        let mut quote = sample_quote("QN-EH/804/2026", "Rev.01");
        let quote_id = save_quote(&conn, "tenant-1", "user-1", &quote).unwrap();
        quote.status = Some("Approved".to_string());
        save_quote(&conn, "tenant-1", "user-1", &quote).unwrap();

        // tenant-2 cannot branch a revision from tenant-1's locked quote.
        let result = branch_new_revision(&conn, "tenant-2", "user-2", &quote_id, "Rev.02");
        assert!(matches!(result, Err(QuoteError::InvalidPayload(_))));
    }

    #[test]
    fn save_quote_with_owned_tax_rule_id_round_trips() {
        let conn = test_conn();
        conn.execute(
            "INSERT INTO tax_rules (id, tenant_id, region_label, rate, is_default, is_active, created_at, updated_at)
             VALUES ('rule-1', 'tenant-1', 'UAE Standard VAT', 5.0, 1, 1, '2026-01-01T00:00:00.000Z', '2026-01-01T00:00:00.000Z')",
            [],
        )
        .unwrap();

        let mut quote = sample_quote("QN-EH/700/2026", "Rev.01");
        quote.tax_rule_id = Some("rule-1".to_string());
        save_quote(&conn, "tenant-1", "user-1", &quote).unwrap();

        let fetched = fetch_quote(&conn, "tenant-1", "QN-EH/700/2026", None)
            .unwrap()
            .unwrap();
        assert_eq!(fetched.tax_rule_id, Some("rule-1".to_string()));
    }

    #[test]
    fn save_quote_rejects_another_tenants_tax_rule_id() {
        let conn = test_conn();
        seed_tenant_and_user(&conn, "tenant-2", "user-2");
        conn.execute(
            "INSERT INTO tax_rules (id, tenant_id, region_label, rate, is_default, is_active, created_at, updated_at)
             VALUES ('rule-foreign', 'tenant-2', 'Not Yours', 5.0, 1, 1, '2026-01-01T00:00:00.000Z', '2026-01-01T00:00:00.000Z')",
            [],
        )
        .unwrap();

        let mut quote = sample_quote("QN-EH/701/2026", "Rev.01");
        quote.tax_rule_id = Some("rule-foreign".to_string());
        let result = save_quote(&conn, "tenant-1", "user-1", &quote);
        assert!(matches!(result, Err(QuoteError::InvalidPayload(_))));
    }

    #[test]
    fn cross_tenant_save_cannot_collide_with_another_tenants_row() {
        let conn = test_conn();
        seed_tenant_and_user(&conn, "tenant-2", "user-2");

        let quote_a = sample_quote("QN-EH/600/2026", "Rev.01");
        save_quote(&conn, "tenant-1", "user-1", &quote_a).unwrap();

        let mut quote_b = sample_quote("QN-EH/600/2026", "Rev.01");
        quote_b.customer_name = "Tenant Two's Customer".to_string();
        save_quote(&conn, "tenant-2", "user-2", &quote_b).unwrap();

        // Two independent rows, one per tenant, despite identical (offer_ref,
        // rev_suffix) — the UNIQUE constraint is (tenant_id, offer_ref, rev_suffix),
        // so this must not collide.
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM quotes WHERE offer_ref = 'QN-EH/600/2026'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);

        let tenant1_quote = fetch_quote(&conn, "tenant-1", "QN-EH/600/2026", None)
            .unwrap()
            .unwrap();
        assert_eq!(tenant1_quote.customer_name, "Acme Construction LLC");

        let tenant2_quote = fetch_quote(&conn, "tenant-2", "QN-EH/600/2026", None)
            .unwrap()
            .unwrap();
        assert_eq!(tenant2_quote.customer_name, "Tenant Two's Customer");
    }

    #[test]
    fn save_rejects_empty_offer_ref() {
        let conn = test_conn();
        let mut quote = sample_quote("", "Rev.01");
        quote.offer_ref = "".to_string();
        let result = save_quote(&conn, "tenant-1", "user-1", &quote);
        assert!(matches!(result, Err(QuoteError::InvalidPayload(_))));
    }

    #[test]
    fn save_rejects_empty_rev_suffix() {
        let conn = test_conn();
        let mut quote = sample_quote("QN-EH/700/2026", "");
        quote.rev_suffix = "".to_string();
        let result = save_quote(&conn, "tenant-1", "user-1", &quote);
        assert!(matches!(result, Err(QuoteError::InvalidPayload(_))));
    }

    #[test]
    fn updating_a_quote_replaces_line_items_wholesale() {
        let conn = test_conn();
        let mut quote = sample_quote("QN-EH/800/2026", "Rev.01");
        save_quote(&conn, "tenant-1", "user-1", &quote).unwrap();

        // Replace the single line item with two different ones.
        quote.line_items = vec![
            QuoteItemInput {
                item_description: "Crane, 50T".to_string(),
                make_model: None,
                quantity: 1,
                unit_rate: 2000.0,
                rate_basis: "Weekly".to_string(),
                line_total: 2000.0,
                equipment_spec: None,
            },
            QuoteItemInput {
                item_description: "Generator, 100kVA".to_string(),
                make_model: None,
                quantity: 2,
                unit_rate: 300.0,
                rate_basis: "Daily".to_string(),
                line_total: 600.0,
                equipment_spec: None,
            },
        ];
        save_quote(&conn, "tenant-1", "user-1", &quote).unwrap();

        let fetched = fetch_quote(&conn, "tenant-1", "QN-EH/800/2026", None)
            .unwrap()
            .unwrap();
        assert_eq!(fetched.line_items.len(), 2);
        assert_eq!(fetched.line_items[0].item_description, "Crane, 50T");
        assert_eq!(fetched.line_items[1].item_description, "Generator, 100kVA");
    }
}
