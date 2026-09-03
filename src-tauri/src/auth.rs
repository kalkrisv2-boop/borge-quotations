//! Phase R.2 — Auth & Entitlements Port (security-critical).
//!
//! Rust port of `server/auth.js` (password hashing + session token issuance/verification)
//! and `server/entitlements.js` (`CANONICAL_MODULE_KEYS`, `checkTenantEntitlement`,
//! `guardApiRoute`). The JS files are treated as law for behavior; this module is a
//! translation, not a re-derivation.
//!
//! ## What moved here, permanently (per this phase's brief)
//! - `CANONICAL_MODULE_KEYS` is now defined **once**, here. `server/entitlements.js`'s
//!   copy must be deleted in the same change (see NOTES_R2.md), and `lib.rs`'s old
//!   hand-duplicated copy (previously flagged "MUST mirror server/entitlements.js") is
//!   removed and replaced with `auth::CANONICAL_MODULE_KEYS`.
//! - Entitlement checks are now backed by the real SQLite `tenant_entitlements` table
//!   from R.0 (via the `rusqlite::Connection` in `DbState`), not `lib.rs`'s old
//!   in-memory `EntitlementState` `HashMap` stub.
//!
//! ## Schema assumption flagged (per Worker honesty requirement — PROJECT_BASELINE.md
//! Section 2.2)
//! `migrations/001_core_schema.sql` was **not** among this session's attached files
//! (only `db.rs` and the already-applied migration *names* were visible via
//! `db.rs`'s `MIGRATIONS` array — the `.sql` file contents themselves were not
//! attached). This module assumes `tenant_entitlements` has the shape implied by
//! `route-map-v2.docx` Phase 1.1 ("DDL for tenants, users, tenant_entitlements, quotes,
//! quote_items") and by `lib.rs`'s own prior in-memory shape (`tenant_id -> module_key ->
//! bool`):
//! ```sql
//! CREATE TABLE tenant_entitlements (
//!     tenant_id   TEXT NOT NULL,
//!     module_key  TEXT NOT NULL,
//!     is_enabled  INTEGER NOT NULL DEFAULT 0,
//!     PRIMARY KEY (tenant_id, module_key)
//! );
//! ```
//! **This is an assumption, not a verified fact — the Architect must diff it against
//! the real `migrations/001_core_schema.sql` before merge.** If the real column names
//! differ, only `check_entitlement`'s SQL string needs to change; nothing else in this
//! module depends on the exact column names.
//!
//! ## Timing-safety note
//! Two different constant-time comparisons are used, for two different reasons:
//! - Session-token signature verification uses `hmac::Mac::verify_slice`, which is
//!   constant-time by construction (the `hmac` crate documents this) — recomputing the
//!   expected HMAC and calling `verify_slice` gets timing-safety "for free" without a
//!   separate crate.
//! - Password-hash verification compares two already-computed PBKDF2 outputs (not an
//!   HMAC step itself), so it uses a small local `constant_time_eq` helper below rather
//!   than pulling in the `subtle` crate for one function. This mirrors `auth.js`'s use
//!   of `crypto.timingSafeEqual` for the exact same comparison.

use hmac::{Hmac, Mac};
use pbkdf2::pbkdf2_hmac;
use rand::RngCore;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use sha2::{Sha256, Sha512};

/// Canonical entitlement module keys. Moved here from `entitlements.js` and `lib.rs`'s
/// old hand-duplicated copy — this is now the single implementation. MUST still mirror
/// `PROJECT_BASELINE.md` Section 1.3's phase-to-module-key mapping in spirit; the
/// Architect should confirm that mapping is unchanged by this port (it is — this phase
/// only moves *where* the list lives, not what's in it).
pub const CANONICAL_MODULE_KEYS: [&str; 5] = [
    "quotes_core",
    "inventory_specs",
    "rate_matrix",
    "compliance_terms",
    "revision_lpo",
];

const PBKDF2_ITERATIONS: u32 = 100_000;
const PBKDF2_OUTPUT_LEN: usize = 64; // bytes — matches auth.js's pbkdf2Sync(..., 64, 'sha512')
const SESSION_TOKEN_LIFETIME_SECS: u64 = 8 * 3600; // matches auth.js's 8-hour expiry

// ---------------------------------------------------------------------------
// Password hashing (port of auth.js hashPassword / verifyPassword)
// ---------------------------------------------------------------------------

/// Hashes `password` with PBKDF2-HMAC-SHA512, 100,000 iterations, 64-byte output —
/// identical parameters to `auth.js`'s `hashPassword`. Generates a fresh random 16-byte
/// salt (hex-encoded, matching `crypto.randomBytes(16).toString('hex')`) unless one is
/// supplied (mirrors the JS function's optional second parameter, used by its own tests
/// to hash against a fixed salt for reproducibility).
///
/// Returns `"{salt_hex}${hash_hex}"`, matching auth.js's stored format exactly so
/// existing stored hashes (if any ever came from the JS path) remain verifiable by this
/// Rust implementation without a migration.
pub fn hash_password(password: &str, salt: Option<&str>) -> String {
    let salt_owned;
    let salt_hex: &str = match salt {
        Some(s) => s,
        None => {
            let mut salt_bytes = [0u8; 16];
            rand::thread_rng().fill_bytes(&mut salt_bytes);
            salt_owned = hex::encode(salt_bytes);
            &salt_owned
        }
    };

    let mut out = [0u8; PBKDF2_OUTPUT_LEN];
    pbkdf2_hmac::<Sha512>(
        password.as_bytes(),
        salt_hex.as_bytes(),
        PBKDF2_ITERATIONS,
        &mut out,
    );
    format!("{}${}", salt_hex, hex::encode(out))
}

/// Verifies `password` against a stored `"salt$hash"` string. Returns `false` (never
/// panics/errors) on any malformed input, matching `auth.js`'s
/// `if (!salt || !originalHash) return false;` guard.
pub fn verify_password(password: &str, stored_hash: &str) -> bool {
    let mut parts = stored_hash.splitn(2, '$');
    let (salt_hex, original_hash_hex) = match (parts.next(), parts.next()) {
        (Some(s), Some(h)) if !s.is_empty() && !h.is_empty() => (s, h),
        _ => return false,
    };

    let original_hash = match hex::decode(original_hash_hex) {
        Ok(bytes) => bytes,
        Err(_) => return false,
    };

    let mut candidate = [0u8; PBKDF2_OUTPUT_LEN];
    pbkdf2_hmac::<Sha512>(
        password.as_bytes(),
        salt_hex.as_bytes(),
        PBKDF2_ITERATIONS,
        &mut candidate,
    );

    constant_time_eq(&candidate, &original_hash)
}

/// Manual constant-time byte comparison (see module doc for why this isn't `verify_slice`
/// here). Always walks every byte of the shorter representation; length mismatch is
/// itself checked without early-return timing leakage beyond the length check itself
/// (unavoidable — auth.js's own `if (a.length !== b.length) return false;` has the same
/// property, so this matches its actual timing profile, not just its literal behavior).
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

// ---------------------------------------------------------------------------
// Session tokens (port of auth.js generateSessionToken / verifySessionToken)
// ---------------------------------------------------------------------------

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct SessionClaims {
    pub sub: String,
    pub tenant_id: String,
    pub role: String,
    pub iat: u64,
    pub exp: u64,
}

#[derive(Serialize, Deserialize)]
struct JwtHeader<'a> {
    alg: &'a str,
    typ: &'a str,
}

fn base64url_encode(bytes: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn base64url_decode(s: &str) -> Option<Vec<u8>> {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(s).ok()
}

fn sign(data: &str, secret: &[u8]) -> Option<Vec<u8>> {
    let mut mac = HmacSha256::new_from_slice(secret).ok()?;
    mac.update(data.as_bytes());
    Some(mac.finalize().into_bytes().to_vec())
}

/// Minimal user shape needed to issue a token — mirrors what `auth.js`'s
/// `generateSessionToken(user, secretKey)` reads off its `user` argument
/// (`user.id`, `user.tenant_id`, `user.role`).
pub struct UserForToken<'a> {
    pub id: &'a str,
    pub tenant_id: &'a str,
    pub role: &'a str,
}

/// Issues an 8-hour HMAC-SHA256-signed, JWT-shaped session token — same wire format as
/// `auth.js`'s `generateSessionToken` (`header.payload.signature`, base64url, no
/// padding), so tokens are interchangeable between the Node and Rust implementations as
/// long as they share the same `secret`.
pub fn generate_session_token(user: &UserForToken, secret: &[u8], now_unix_secs: u64) -> String {
    let header = JwtHeader {
        alg: "HS256",
        typ: "JWT",
    };
    let header_b64 = base64url_encode(
        serde_json::to_string(&header)
            .expect("static header struct always serializes")
            .as_bytes(),
    );

    let claims = SessionClaims {
        sub: user.id.to_string(),
        tenant_id: user.tenant_id.to_string(),
        role: user.role.to_string(),
        iat: now_unix_secs,
        exp: now_unix_secs + SESSION_TOKEN_LIFETIME_SECS,
    };
    let payload_b64 = base64url_encode(
        serde_json::to_string(&claims)
            .expect("claims struct always serializes")
            .as_bytes(),
    );

    let signing_input = format!("{}.{}", header_b64, payload_b64);
    let signature = sign(&signing_input, secret).expect("HMAC accepts any key length");
    format!("{}.{}", signing_input, base64url_encode(&signature))
}

#[derive(Debug, PartialEq)]
pub enum TokenError {
    Malformed,
    BadSignature,
    Expired,
}

/// Verifies a session token's signature and expiry, returning the decoded claims on
/// success. Port of `auth.js`'s `verifySessionToken`, including its exact failure
/// surface (malformed / bad signature / expired all collapse to a rejection — callers
/// needing the distinction use the `TokenError` variant, but the JS function itself only
/// ever returned `null` for all three; `guard_api_route` below preserves that collapsed
/// behavior for parity with `guardApiRoute`'s single 401 branch).
///
/// Signature check uses `Mac::verify_slice`, which the `hmac` crate implements as a
/// constant-time comparison — this is the "for free" timing-safety case described in
/// the module doc, and is the direct equivalent of `auth.js`'s
/// `crypto.timingSafeEqual(a, b)` call on the signature bytes.
pub fn verify_session_token(
    token: &str,
    secret: &[u8],
    now_unix_secs: u64,
) -> Result<SessionClaims, TokenError> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return Err(TokenError::Malformed);
    }
    let (header_b64, payload_b64, signature_b64) = (parts[0], parts[1], parts[2]);

    let signature = base64url_decode(signature_b64).ok_or(TokenError::Malformed)?;

    let signing_input = format!("{}.{}", header_b64, payload_b64);
    let mut mac = HmacSha256::new_from_slice(secret).map_err(|_| TokenError::BadSignature)?;
    mac.update(signing_input.as_bytes());
    mac.verify_slice(&signature).map_err(|_| TokenError::BadSignature)?;

    let payload_bytes = base64url_decode(payload_b64).ok_or(TokenError::Malformed)?;
    let claims: SessionClaims =
        serde_json::from_slice(&payload_bytes).map_err(|_| TokenError::Malformed)?;

    if now_unix_secs >= claims.exp {
        return Err(TokenError::Expired);
    }

    Ok(claims)
}

// ---------------------------------------------------------------------------
// Entitlements (port of entitlements.js checkTenantEntitlement / guardApiRoute)
// ---------------------------------------------------------------------------

/// Checks whether `tenant_id` holds an active entitlement for `module_key`, against the
/// real SQLite `tenant_entitlements` table (see module doc for the assumed schema).
/// Port of `checkTenantEntitlement` — same short-circuit behavior: an unknown
/// `module_key` (not in `CANONICAL_MODULE_KEYS`) is rejected before ever touching the
/// database, matching the JS version's `if (!CANONICAL_MODULE_KEYS.includes(moduleKey))
/// return false;` guard.
pub fn check_tenant_entitlement(
    conn: &Connection,
    tenant_id: &str,
    module_key: &str,
) -> rusqlite::Result<bool> {
    if tenant_id.is_empty() || module_key.is_empty() {
        return Ok(false);
    }
    if !CANONICAL_MODULE_KEYS.contains(&module_key) {
        return Ok(false);
    }

    let is_enabled: Option<i64> = conn
        .query_row(
            "SELECT is_enabled FROM tenant_entitlements WHERE tenant_id = ?1 AND module_key = ?2",
            rusqlite::params![tenant_id, module_key],
            |row| row.get(0),
        )
        .ok(); // no row => None, same as entitlements.js's "tenantMap not found -> false"

    Ok(is_enabled == Some(1))
}

/// Seeds (inserts or updates) one tenant's entitlement for one module. Port of
/// `entitlements.js`'s `seedTenantEntitlements`, adapted to write through to the real
/// table instead of the in-memory `Map`. Callers (e.g. an `admin_seed_entitlements`
/// command) are responsible for their own authorization check before calling this —
/// this function does not itself re-check who's allowed to seed, same division of
/// responsibility as the JS version.
pub fn seed_tenant_entitlement(
    conn: &Connection,
    tenant_id: &str,
    module_key: &str,
    is_enabled: bool,
) -> rusqlite::Result<()> {
    if tenant_id.is_empty() {
        return Err(rusqlite::Error::InvalidParameterName(
            "tenant_id is required for entitlement seeding".into(),
        ));
    }
    if !CANONICAL_MODULE_KEYS.contains(&module_key) {
        // entitlements.js silently skips unknown module_keys rather than erroring
        // (`if (CANONICAL_MODULE_KEYS.includes(item.module_key)) { ... }` with no else) —
        // mirrored here as a silent no-op for parity.
        return Ok(());
    }
    // FIX (Architect audit, Phase R.2): the previous version of this INSERT only set
    // (tenant_id, module_key, is_enabled), which matched this module's *assumed* schema
    // but not the REAL migrations/001_core_schema.sql table, which additionally has
    // `id VARCHAR(36) PRIMARY KEY` (no auto-default — SQLite only autogenerates rowids
    // for `INTEGER PRIMARY KEY`, not VARCHAR ones) and `granted_at VARCHAR(30) NOT NULL`
    // (no default). Confirmed by direct execution against the real migration file that
    // the old INSERT raised `NOT NULL constraint failed: tenant_entitlements.granted_at`.
    // `id` is generated here (random-hex, matching this schema's VARCHAR(36) convention
    // used elsewhere, e.g. tenants.id); `granted_at` is set via SQLite's own `strftime`
    // to stay byte-identical in format to db.rs's/the migration triggers' own timestamp
    // convention, rather than adding a datetime crate for one column. On conflict
    // (tenant already has a row for this module_key), only `is_enabled` is updated —
    // `id`/`granted_at` intentionally keep their original values, since re-seeding an
    // existing entitlement is a state flip, not a new grant.
    conn.execute(
        "INSERT INTO tenant_entitlements (id, tenant_id, module_key, is_enabled, granted_at)
         VALUES (?1, ?2, ?3, ?4, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
         ON CONFLICT(tenant_id, module_key) DO UPDATE SET is_enabled = excluded.is_enabled",
        rusqlite::params![generate_entitlement_id(), tenant_id, module_key, is_enabled as i64],
    )?;
    Ok(())
}

/// Generates a random 36-character hex id for a new `tenant_entitlements` row, matching
/// the VARCHAR(36) id convention used elsewhere in this schema (e.g. `tenants.id`). Not a
/// spec-compliant UUID (no version/variant bits set) — sufficient for this table's needs
/// (uniqueness, fixed length) without adding a `uuid` crate dependency for one call site.
fn generate_entitlement_id() -> String {
    let mut bytes = [0u8; 18];
    rand::thread_rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}

#[derive(Debug, PartialEq)]
pub enum GuardError {
    /// Port of guardApiRoute's `{ status: 401, error: 'Unauthorized: Invalid or expired
    /// session token' }`.
    Unauthorized,
    /// Port of guardApiRoute's `{ status: 403, error: 'Forbidden: Missing tenant scope
    /// in session' }` — kept as a distinct variant even though a token built by
    /// `generate_session_token` here can never actually produce an empty `tenant_id`
    /// (mirrors the JS function's own defensive check on a field the *issuing* code
    /// always sets, since `guardApiRoute` is meant to be robust to any well-formed
    /// token, not only ones this codebase issued).
    MissingTenantScope,
    /// Port of guardApiRoute's `{ status: 403, error: "Access Denied: Tenant '...' does
    /// not hold active entitlement for module '...'" }`.
    EntitlementDenied { tenant_id: String, module_key: String },
}

/// Rust equivalent of `entitlements.js`'s `guardApiRoute`, minus the `handlerFn`
/// execution/exception-trapping part (that belongs to whatever Tauri command calls this
/// — R.5's job, not R.2's, per this phase's "library module only" scope). Returns the
/// verified `SessionClaims` on success, so the caller can proceed with `tenant_id` and
/// `sub` (user id) already resolved and trustworthy — same contract as `guardApiRoute`
/// passing `{ ...payload, tenant_id, user_id }` into its handler.
pub fn guard_api_route(
    conn: &Connection,
    session_token: &str,
    secret: &[u8],
    required_module_key: &str,
    now_unix_secs: u64,
) -> Result<SessionClaims, GuardError> {
    let claims = verify_session_token(session_token, secret, now_unix_secs)
        .map_err(|_| GuardError::Unauthorized)?;

    if claims.tenant_id.trim().is_empty() {
        return Err(GuardError::MissingTenantScope);
    }

    let allowed = check_tenant_entitlement(conn, &claims.tenant_id, required_module_key)
        .unwrap_or(false); // a DB error here must fail closed, not open

    if !allowed {
        return Err(GuardError::EntitlementDenied {
            tenant_id: claims.tenant_id.clone(),
            module_key: required_module_key.to_string(),
        });
    }

    Ok(claims)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// FIX (Architect audit, Phase R.2): this fixture previously used a 3-column table
    /// (`tenant_id`, `module_key`, `is_enabled`) that matched this module's *assumed*
    /// schema rather than the real `migrations/001_core_schema.sql` table. Every test in
    /// this module passed against that assumption while `seed_tenant_entitlement`'s real
    /// INSERT was silently broken against the actual schema (missing `id`/`granted_at`,
    /// both NOT NULL with no default) — confirmed by direct execution against the real
    /// migration file. Updated to the real column set so this test suite can no longer
    /// pass while the production code is broken.
    fn test_conn_with_entitlements_table() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE tenant_entitlements (
                id          TEXT PRIMARY KEY,
                tenant_id   TEXT NOT NULL,
                module_key  TEXT NOT NULL,
                is_enabled  INTEGER NOT NULL DEFAULT 0,
                granted_at  TEXT NOT NULL,
                expires_at  TEXT,
                UNIQUE(tenant_id, module_key)
            );",
        )
        .unwrap();
        conn
    }

    // --- password hashing: translated from auth.js's implicit contract ---

    #[test]
    fn hash_and_verify_round_trip_succeeds() {
        let hash = hash_password("correct horse battery staple", None);
        assert!(verify_password("correct horse battery staple", &hash));
    }

    #[test]
    fn verify_rejects_wrong_password() {
        let hash = hash_password("correct horse battery staple", None);
        assert!(!verify_password("wrong password", &hash));
    }

    #[test]
    fn verify_rejects_malformed_stored_hash() {
        assert!(!verify_password("anything", "not-a-valid-stored-hash"));
        assert!(!verify_password("anything", ""));
        assert!(!verify_password("anything", "$onlyhash"));
        assert!(!verify_password("anything", "onlysalt$"));
    }

    #[test]
    fn same_password_different_calls_produce_different_hashes_different_salts() {
        // No fixed salt supplied -> random salt each time -> different stored strings,
        // same as auth.js's default `crypto.randomBytes(16)` behavior.
        let h1 = hash_password("same-password", None);
        let h2 = hash_password("same-password", None);
        assert_ne!(h1, h2);
        assert!(verify_password("same-password", &h1));
        assert!(verify_password("same-password", &h2));
    }

    #[test]
    fn fixed_salt_produces_deterministic_hash() {
        let h1 = hash_password("same-password", Some("deadbeef00112233"));
        let h2 = hash_password("same-password", Some("deadbeef00112233"));
        assert_eq!(h1, h2);
    }

    // --- session tokens: translated from auth.js's generate/verifySessionToken pair ---

    #[test]
    fn generated_token_verifies_successfully() {
        let secret = b"test-secret-key";
        let user = UserForToken {
            id: "user-1",
            tenant_id: "tenant-1",
            role: "staff",
        };
        let now = 1_700_000_000u64;
        let token = generate_session_token(&user, secret, now);
        let claims = verify_session_token(&token, secret, now + 10).expect("should verify");
        assert_eq!(claims.sub, "user-1");
        assert_eq!(claims.tenant_id, "tenant-1");
        assert_eq!(claims.role, "staff");
        assert_eq!(claims.iat, now);
        assert_eq!(claims.exp, now + SESSION_TOKEN_LIFETIME_SECS);
    }

    #[test]
    fn token_expired_is_rejected() {
        let secret = b"test-secret-key";
        let user = UserForToken {
            id: "user-1",
            tenant_id: "tenant-1",
            role: "staff",
        };
        let now = 1_700_000_000u64;
        let token = generate_session_token(&user, secret, now);
        let past_expiry = now + SESSION_TOKEN_LIFETIME_SECS; // exactly at exp: auth.js uses `>=`
        let result = verify_session_token(&token, secret, past_expiry);
        assert_eq!(result, Err(TokenError::Expired));
    }

    #[test]
    fn tampered_payload_is_rejected() {
        let secret = b"test-secret-key";
        let user = UserForToken {
            id: "user-1",
            tenant_id: "tenant-1",
            role: "staff",
        };
        let now = 1_700_000_000u64;
        let token = generate_session_token(&user, secret, now);
        let mut parts: Vec<&str> = token.split('.').collect();
        // Swap in a payload claiming a different (and higher-privileged) tenant, without
        // re-signing -- this is exactly the attack a signature check exists to catch.
        let forged_claims = SessionClaims {
            sub: "user-1".into(),
            tenant_id: "some-other-tenant".into(),
            role: "admin".into(),
            iat: now,
            exp: now + SESSION_TOKEN_LIFETIME_SECS,
        };
        let forged_payload_b64 =
            base64url_encode(serde_json::to_string(&forged_claims).unwrap().as_bytes());
        parts[1] = &forged_payload_b64;
        let forged_token = parts.join(".");
        let result = verify_session_token(&forged_token, secret, now + 10);
        assert_eq!(result, Err(TokenError::BadSignature));
    }

    #[test]
    fn tampered_signature_is_rejected() {
        let secret = b"test-secret-key";
        let user = UserForToken {
            id: "user-1",
            tenant_id: "tenant-1",
            role: "staff",
        };
        let now = 1_700_000_000u64;
        let mut token = generate_session_token(&user, secret, now);
        token.push('x'); // corrupt the trailing signature bytes
        let result = verify_session_token(&token, secret, now + 10);
        assert_eq!(result, Err(TokenError::BadSignature));
    }

    #[test]
    fn token_signed_with_wrong_secret_is_rejected() {
        let user = UserForToken {
            id: "user-1",
            tenant_id: "tenant-1",
            role: "staff",
        };
        let now = 1_700_000_000u64;
        let token = generate_session_token(&user, b"secret-a", now);
        let result = verify_session_token(&token, b"secret-b", now + 10);
        assert_eq!(result, Err(TokenError::BadSignature));
    }

    #[test]
    fn malformed_token_shapes_are_rejected() {
        let secret = b"test-secret-key";
        assert_eq!(
            verify_session_token("not-a-jwt", secret, 0),
            Err(TokenError::Malformed)
        );
        assert_eq!(
            verify_session_token("a.b", secret, 0),
            Err(TokenError::Malformed)
        );
        assert_eq!(
            verify_session_token("a.b.c.d", secret, 0),
            Err(TokenError::Malformed)
        );
        assert_eq!(
            verify_session_token("", secret, 0),
            Err(TokenError::Malformed)
        );
    }

    // --- entitlements: translated from entitlements.js's contract ---

    #[test]
    fn check_entitlement_false_when_no_row_seeded() {
        let conn = test_conn_with_entitlements_table();
        assert!(!check_tenant_entitlement(&conn, "tenant-1", "quotes_core").unwrap());
    }

    #[test]
    fn seed_then_check_reflects_enabled_state() {
        let conn = test_conn_with_entitlements_table();
        seed_tenant_entitlement(&conn, "tenant-1", "quotes_core", true).unwrap();
        assert!(check_tenant_entitlement(&conn, "tenant-1", "quotes_core").unwrap());
        assert!(!check_tenant_entitlement(&conn, "tenant-1", "rate_matrix").unwrap());
    }

    #[test]
    fn seed_can_flip_entitlement_back_off() {
        let conn = test_conn_with_entitlements_table();
        seed_tenant_entitlement(&conn, "tenant-1", "quotes_core", true).unwrap();
        assert!(check_tenant_entitlement(&conn, "tenant-1", "quotes_core").unwrap());
        seed_tenant_entitlement(&conn, "tenant-1", "quotes_core", false).unwrap();
        assert!(!check_tenant_entitlement(&conn, "tenant-1", "quotes_core").unwrap());
    }

    #[test]
    fn check_entitlement_rejects_unknown_module_key_without_querying_db() {
        let conn = test_conn_with_entitlements_table();
        // Even if somehow seeded under a bogus key via a direct insert, an unknown key
        // must be rejected -- entitlements.js checks CANONICAL_MODULE_KEYS membership
        // before ever consulting the store.
        conn.execute(
            "INSERT INTO tenant_entitlements (id, tenant_id, module_key, is_enabled, granted_at)
             VALUES ('test-id-1', ?1, ?2, 1, '2026-01-01T00:00:00.000Z')",
            rusqlite::params!["tenant-1", "not_a_real_module"],
        )
        .unwrap();
        assert!(!check_tenant_entitlement(&conn, "tenant-1", "not_a_real_module").unwrap());
    }

    #[test]
    fn seed_silently_skips_unknown_module_key() {
        let conn = test_conn_with_entitlements_table();
        seed_tenant_entitlement(&conn, "tenant-1", "not_a_real_module", true).unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM tenant_entitlements", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0, "unknown module_key must not be written");
    }

    #[test]
    fn cross_tenant_entitlement_is_isolated() {
        let conn = test_conn_with_entitlements_table();
        seed_tenant_entitlement(&conn, "tenant-a", "quotes_core", true).unwrap();
        assert!(check_tenant_entitlement(&conn, "tenant-a", "quotes_core").unwrap());
        assert!(
            !check_tenant_entitlement(&conn, "tenant-b", "quotes_core").unwrap(),
            "tenant-b must not inherit tenant-a's entitlement"
        );
    }

    // --- guard_api_route: translated from guardApiRoute's contract ---

    #[test]
    fn guard_rejects_invalid_token() {
        let conn = test_conn_with_entitlements_table();
        let result = guard_api_route(&conn, "garbage", b"secret", "quotes_core", 1_700_000_000);
        assert_eq!(result.unwrap_err(), GuardError::Unauthorized);
    }

    #[test]
    fn guard_rejects_expired_token() {
        let conn = test_conn_with_entitlements_table();
        let secret = b"secret";
        let user = UserForToken {
            id: "u1",
            tenant_id: "tenant-1",
            role: "staff",
        };
        let now = 1_700_000_000u64;
        let token = generate_session_token(&user, secret, now);
        seed_tenant_entitlement(&conn, "tenant-1", "quotes_core", true).unwrap();
        let result = guard_api_route(
            &conn,
            &token,
            secret,
            "quotes_core",
            now + SESSION_TOKEN_LIFETIME_SECS + 1,
        );
        assert_eq!(result.unwrap_err(), GuardError::Unauthorized);
    }

    #[test]
    fn guard_rejects_valid_token_without_entitlement() {
        let conn = test_conn_with_entitlements_table();
        let secret = b"secret";
        let user = UserForToken {
            id: "u1",
            tenant_id: "tenant-1",
            role: "staff",
        };
        let now = 1_700_000_000u64;
        let token = generate_session_token(&user, secret, now);
        // Deliberately NOT seeding any entitlement for tenant-1.
        let result = guard_api_route(&conn, &token, secret, "quotes_core", now + 5);
        match result {
            Err(GuardError::EntitlementDenied { tenant_id, module_key }) => {
                assert_eq!(tenant_id, "tenant-1");
                assert_eq!(module_key, "quotes_core");
            }
            other => panic!("expected EntitlementDenied, got {:?}", other),
        }
    }

    #[test]
    fn guard_accepts_valid_token_with_entitlement() {
        let conn = test_conn_with_entitlements_table();
        let secret = b"secret";
        let user = UserForToken {
            id: "u1",
            tenant_id: "tenant-1",
            role: "staff",
        };
        let now = 1_700_000_000u64;
        let token = generate_session_token(&user, secret, now);
        seed_tenant_entitlement(&conn, "tenant-1", "quotes_core", true).unwrap();
        let claims = guard_api_route(&conn, &token, secret, "quotes_core", now + 5).unwrap();
        assert_eq!(claims.tenant_id, "tenant-1");
        assert_eq!(claims.sub, "u1");
    }

    #[test]
    fn guard_rejects_tampered_token_even_with_real_entitlement() {
        let conn = test_conn_with_entitlements_table();
        let secret = b"secret";
        let user = UserForToken {
            id: "u1",
            tenant_id: "tenant-1",
            role: "staff",
        };
        let now = 1_700_000_000u64;
        let token = generate_session_token(&user, secret, now);
        seed_tenant_entitlement(&conn, "tenant-1", "quotes_core", true).unwrap();

        let mut parts: Vec<&str> = token.split('.').collect();
        let forged_claims = SessionClaims {
            sub: "u1".into(),
            tenant_id: "tenant-1".into(),
            role: "admin".into(), // privilege-escalation attempt via forged payload
            iat: now,
            exp: now + SESSION_TOKEN_LIFETIME_SECS,
        };
        let forged_payload_b64 =
            base64url_encode(serde_json::to_string(&forged_claims).unwrap().as_bytes());
        parts[1] = &forged_payload_b64;
        let forged_token = parts.join(".");

        let result = guard_api_route(&conn, &forged_token, secret, "quotes_core", now + 5);
        assert_eq!(result.unwrap_err(), GuardError::Unauthorized);
    }

    #[test]
    fn guard_rejects_unknown_required_module_key() {
        let conn = test_conn_with_entitlements_table();
        let secret = b"secret";
        let user = UserForToken {
            id: "u1",
            tenant_id: "tenant-1",
            role: "staff",
        };
        let now = 1_700_000_000u64;
        let token = generate_session_token(&user, secret, now);
        let result = guard_api_route(&conn, &token, secret, "not_a_real_module", now + 5);
        assert!(matches!(result, Err(GuardError::EntitlementDenied { .. })));
    }
}
