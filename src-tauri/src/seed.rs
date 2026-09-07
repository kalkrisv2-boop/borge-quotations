//! Dev-only data seeder — closes the gap flagged in WORKER_CARRYOVER_R5-FIX.md
//! ("You will need a seeded test user to log in as — the real `users` table has no
//! rows yet").
//!
//! SECURITY/PRIVACY NOTE (this revision): earlier versions of this file hardcoded the
//! real Borge/Servepower company details (VAT TRN, phone numbers, address) and a real
//! working login (email + PBKDF2 hash) directly in source. That's fine for a private
//! repo, but this project is now going public on GitHub — real business data and a
//! real working credential baked into committed source would be visible to anyone,
//! permanently, in git history, even if later removed.
//!
//! Fix: every value below is read from an environment variable (populated via a local
//! `.env` file — see `.env.example` for the full list, and `.gitignore`, which already
//! excludes `.env`/`.env.*` while explicitly allowing `.env.example` through). Non-
//! sensitive fields (company name, address, phone) fall back to obviously-fake
//! placeholder text if `.env` isn't present, so a fresh public clone never displays
//! real company data by accident. The login credential is treated as sensitive
//! specifically: if `SEED_USER_EMAIL` and `SEED_USER_PASSWORD_HASH` aren't both set,
//! **no user is seeded at all** — there is no fallback "default password" anywhere in
//! this binary. A repo with no `.env` configured simply has no way to log in until one
//! is created, which is the safe failure mode for a public repository.

use rusqlite::{params, Connection};
use std::env;

fn env_or(key: &str, placeholder: &str) -> String {
    env::var(key).unwrap_or_else(|_| placeholder.to_string())
}

/// Inserts the seed tenant/entitlements (always, using real-or-placeholder values) and
/// the seed user/sample quote (only if real credentials are configured via `.env`).
/// Idempotent (`INSERT OR IGNORE`) — safe to call on every startup.
pub fn seed_dev_data(conn: &Connection) -> rusqlite::Result<()> {
    let tenant_id = env_or("SEED_TENANT_ID", "t-example-tenant-001");
    let now = current_timestamp(conn);

    conn.execute(
        "INSERT OR IGNORE INTO tenants
            (id, name, company_name_ar, company_tagline_ar, po_box, city, country,
             phone_1, phone_2, email, website, vat_trn, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?13)",
        params![
            tenant_id,
            env_or("SEED_TENANT_NAME", "Example Equipment Rental LLC"),
            env_or("SEED_TENANT_NAME_AR", "مثال لتأجير المعدات"),
            env_or("SEED_TENANT_TAGLINE_AR", "مثال - ليس بيانات حقيقية"),
            env_or("SEED_TENANT_PO_BOX", "P.O. Box: 00000"),
            env_or("SEED_TENANT_CITY", "Example City"),
            "United Arab Emirates",
            env_or("SEED_TENANT_PHONE_1", "+971 50 0000000"),
            env_or("SEED_TENANT_PHONE_2", "+971 56 0000000"),
            env_or("SEED_TENANT_EMAIL", "info@example.com"),
            env_or("SEED_TENANT_WEBSITE", "www.example.com"),
            env_or("SEED_TENANT_VAT_TRN", "000000000000000"),
            now,
        ],
    )?;

    for module_key in [
        "quotes_core",
        "inventory_specs",
        "rate_matrix",
        "compliance_terms",
        "revision_lpo",
    ] {
        conn.execute(
            "INSERT OR IGNORE INTO tenant_entitlements
                (id, tenant_id, module_key, is_enabled, granted_at)
             VALUES (?1, ?2, ?3, 1, ?4)",
            params![
                format!("ent-{}-{}", tenant_id, module_key),
                tenant_id,
                module_key,
                now,
            ],
        )?;
    }

    // SECURITY: deliberately no fallback here. Only seed a login-capable user if a
    // real email + real PBKDF2 hash were provided via .env -- never bake a working
    // credential into the binary as a "default".
    let seed_email = env::var("SEED_USER_EMAIL").ok();
    let seed_hash = env::var("SEED_USER_PASSWORD_HASH").ok();
    let (email, hash) = match (seed_email, seed_hash) {
        (Some(e), Some(h)) => (e, h),
        _ => {
            eprintln!(
                "[seed] SEED_USER_EMAIL / SEED_USER_PASSWORD_HASH not set in .env -- \
                 skipping dev user seed. Copy .env.example to .env and fill these in \
                 if you want to log in locally."
            );
            return Ok(());
        }
    };

    let user_id = env_or("SEED_USER_ID", "u-example-admin-01");
    conn.execute(
        "INSERT OR IGNORE INTO users
            (id, tenant_id, email, password_hash, full_name, phone, role, is_active,
             created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 1, ?8, ?8)",
        params![
            user_id,
            tenant_id,
            email,
            hash,
            env_or("SEED_USER_FULL_NAME", "Example Admin"),
            env_or("SEED_USER_PHONE", "+971 50 0000000"),
            "admin",
            now,
        ],
    )?;

    // Sample quote is only seeded alongside a real user (references user_id via FK, and
    // is genuinely just demo content for exercising save/fetch/restart-persistence and
    // PDF export locally) -- also gated behind SEED_SAMPLE_QUOTE=1 so it's opt-in.
    if env::var("SEED_SAMPLE_QUOTE").as_deref() != Ok("1") {
        return Ok(());
    }

    let quote_id = "q-sample-001";
    conn.execute(
        "INSERT OR IGNORE INTO quotes
            (id, tenant_id, user_id, offer_ref, rev_suffix, quote_date, validity_days,
             customer_name, customer_po_box, customer_city, contact_person,
             salesperson_name, subject_text, notes, terms_conditions, rate_basis_text,
             vat_rate, total_amount, vat_amount, grand_total, status, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16,
                 ?17, ?18, ?19, ?20, ?21, ?22, ?22)",
        params![
            quote_id,
            tenant_id,
            user_id,
            "QN-SAMPLE/001/2026",
            "Rev.01",
            "2026-08-20",
            30,
            "Sample Customer LLC",
            "",
            "Example City, UAE",
            "Sample Contact",
            "Sample Salesperson",
            "Sample equipment rental quotation for local testing.",
            "This is placeholder demo data, not a real client record.",
            "1. Sample term one.\n2. Sample term two.",
            "Daily rate: less than 6 days\nWeekly rate: 7 to 25 days\nMonthly rate: 26 days or more",
            5.00,
            10000.00,
            500.00,
            10500.00,
            "Draft",
            now,
        ],
    )?;

    conn.execute(
        "INSERT OR IGNORE INTO quote_items
            (id, tenant_id, quote_id, item_order, item_description, make_model,
             quantity, unit_rate, rate_basis, line_total, equipment_spec, created_at)
         VALUES (?1, ?2, ?3, 1, ?4, ?5, 1, ?6, 'Monthly', ?6, ?7, ?8)",
        params![
            format!("qi-{}-1", quote_id),
            tenant_id,
            quote_id,
            "Sample Equipment Item",
            "Sample/Model-001",
            10000.0,
            "Sample specification detail",
            now,
        ],
    )?;

    Ok(())
}

fn current_timestamp(conn: &Connection) -> String {
    conn.query_row("SELECT strftime('%Y-%m-%dT%H:%M:%fZ', 'now')", [], |row| {
        row.get(0)
    })
    .unwrap_or_else(|_| "2026-01-01T00:00:00.000Z".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    #[test]
    fn without_env_vars_no_user_is_seeded_and_no_real_data_appears() {
        let path = std::env::temp_dir().join(format!(
            "borge_seed_test_noenv_{}.sqlite3",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let conn = db::init_db(&path).expect("init_db should succeed");
        seed_dev_data(&conn).expect("seeding with no env vars should not error");

        let user_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM users", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            user_count, 0,
            "no user should be seeded when SEED_USER_EMAIL/SEED_USER_PASSWORD_HASH are unset"
        );

        let tenant_name: String = conn
            .query_row("SELECT name FROM tenants LIMIT 1", [], |r| r.get(0))
            .unwrap();
        assert!(
            tenant_name.contains("Example"),
            "tenant name should be an obvious placeholder, not real company data"
        );

        drop(conn);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn with_env_vars_set_a_real_login_capable_user_is_seeded() {
        std::env::set_var("SEED_USER_EMAIL", "test@example.com");
        std::env::set_var(
            "SEED_USER_PASSWORD_HASH",
            crate::auth::hash_password("TestPassword123!", None),
        );

        let path = std::env::temp_dir().join(format!(
            "borge_seed_test_withenv_{}.sqlite3",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let conn = db::init_db(&path).expect("init_db should succeed");
        seed_dev_data(&conn).expect("seeding with env vars set should succeed");

        let hash: String = conn
            .query_row(
                "SELECT password_hash FROM users WHERE email = ?1",
                ["test@example.com"],
                |r| r.get(0),
            )
            .unwrap();
        assert!(crate::auth::verify_password("TestPassword123!", &hash));

        std::env::remove_var("SEED_USER_EMAIL");
        std::env::remove_var("SEED_USER_PASSWORD_HASH");
        drop(conn);
        let _ = std::fs::remove_file(&path);
    }
}
