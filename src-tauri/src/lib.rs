use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;

pub mod auth;
mod db;
mod pdf_render;
mod quotes;
mod rate_matrix;
mod seed;
mod compliance;

use std::io::Write as _;
use std::process::Command;
use rusqlite::OptionalExtension;

/// Phase R.0: holds the one real SQLite connection the running app uses.
/// Phase R.2: now actually read from — `handle_guarded_ipc` and
/// `admin_seed_entitlements` below query/write the real `tenant_entitlements` table
/// through this connection via `auth::check_tenant_entitlement` /
/// `auth::seed_tenant_entitlement`, replacing the old in-memory `EntitlementState`.
pub struct DbState(pub Mutex<rusqlite::Connection>);

/// HMAC secret used to sign/verify session tokens.
///
/// R.5-FIX (Architect correction — this phase's Stitch Point R audit found the R.2/R.5
/// deferral of this item had become a live vulnerability, not a benign gap): with
/// `server/*.js` now retired from the runtime path (R.5 deliverable 3), NOTHING was
/// left anywhere in the compiled app that verified a session token or a password before
/// `register_session` (below) trusted a caller-supplied `tenant_id`/`user_id` outright.
/// `auth::verify_session_token`, `auth::guard_api_route`, `auth::generate_session_token`,
/// `auth::hash_password`, and `auth::verify_password` were all fully built and
/// independently tested (auth.rs, 22 tests, all passing — confirmed again this session
/// via a real merged-crate `cargo test` run) but were called from *nowhere* in lib.rs.
/// Any caller could invoke `register_session` with `tenant_id: "<any tenant with
/// entitlements>"` and receive full, entitlement-checked access with zero credentials.
/// This struct is now actually constructed and `.manage()`d in `run()` below (a random
/// 32-byte secret, generated once and persisted to a file in `app_data_dir` so it
/// survives restarts — see `resolve_or_create_hmac_secret`), and is threaded into the
/// new `login` command and the corrected `register_session` below.
pub struct HmacSecret(pub Vec<u8>);

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct IpcResponse {
    pub status: u16,
    pub message: String,
    pub data: Option<serde_json::Value>,
}

/// Maps an IPC command name to the module_key required to invoke it.
/// MUST mirror the switch statement in server/ipc_handlers.js.
///
/// NOTE (Phase R.2): `CANONICAL_MODULE_KEYS` itself no longer lives here -- it moved to
/// `auth::CANONICAL_MODULE_KEYS`, the single source of truth, per this phase's brief.
/// This function stays here because it's about IPC *command routing*, a lib.rs concern,
/// not an entitlements concern; it references `auth::CANONICAL_MODULE_KEYS` only
/// implicitly (the strings below must be members of that list, which is checked in
/// lib.rs's own test module below).
fn required_module_key(command_name: &str) -> Option<&'static str> {
    match command_name {
        "save_quote" | "fetch_quote" => Some("quotes_core"),
        "fetch_inventory_specs" => Some("inventory_specs"),
        "calculate_rate_matrix" => Some("rate_matrix"),
        // PDF export is a `quotes_core` capability, not a separate module key —
        // matches server/ipc_handlers.js's own comment on this exact point
        // ("Gated behind the same 'quotes_core' entitlement as save/fetch, since PDF
        // export is a quotes_core capability, not a separate module key in
        // CANONICAL_MODULE_KEYS").
        "generate_quote_pdf" => Some("quotes_core"),
        // Phase 5.1: revision history/branching is a quotes_core capability, same
        // reasoning as generate_quote_pdf above -- not a separate module key.
        "fetch_quote_revisions" | "branch_new_revision" => Some("quotes_core"),
        // Phase 5.2: status lifecycle progression and LPO tracking are quotes_core
        // capabilities too, same reasoning -- route-map-v2.docx's own
        // CANONICAL_MODULE_KEYS list (5 entries, checked in the test below) has no
        // separate "lpo_tracking" key, and this project's established pattern (PDF
        // export, revision branching) is to fold closely-related quote actions into
        // quotes_core rather than invent a new key per verb.
        "update_quote_status" | "attach_lpo" => Some("quotes_core"),
        // Phase 4.2: compliance_terms was already reserved as a module key in
        // 001_core_schema.sql's comment since Phase R.0 — this is the first phase that
        // actually implements commands behind it.
        "list_compliance_terms"
        | "save_compliance_term"
        | "deactivate_compliance_term"
        | "attach_terms_to_quote"
        | "list_tax_rules"
        | "save_tax_rule"
        | "deactivate_tax_rule" => Some("compliance_terms"),
        _ => None,
    }
}

/// session_token -> (tenant_id, user_id). Populated ONLY by register_session, which is
/// called by the frontend right after server/auth.js's verifySessionToken
/// has already confirmed the token server-side. Rust trusts this map
/// because entries are only ever inserted post-verification — it does not
/// re-derive trust from an unverified client claim.
///
/// Phase R.2 status: the tenant_id half of this is unchanged (see `HmacSecret` doc
/// comment above for why it wasn't folded into the new `auth::verify_session_token`
/// path yet). Left in place deliberately rather than half-migrated.
///
/// Phase R.3 addition: the value is now `(tenant_id, user_id)` rather than just
/// `tenant_id` — `save_quote` needs a `user_id` to satisfy the real `quotes` table's
/// `user_id NOT NULL` FK, and no other source of a trustworthy user_id exists yet at
/// this IPC boundary (full independent re-verification of the token, which would derive
/// both fields from the signed claims instead of trusting the caller, is still the
/// explicit R.5 open item — see `HmacSecret` doc comment). `register_session`'s
/// signature grows a `user_id` parameter accordingly; this is flagged in NOTES_R3.md as
/// a deliberate, minimal expansion of R.3's scope, not a silent one.
#[derive(Default)]
pub struct SessionState(pub Mutex<HashMap<String, (String, String)>>);

#[derive(Deserialize)]
struct EntitlementSeedItem {
    module_key: String,
    is_enabled: bool,
}

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You're running borge_equipment_rental.", name)
}

/// Real login command — did not exist anywhere in the app before this fix (`auth::
/// hash_password`/`verify_password`/`generate_session_token` were built and tested in
/// R.2 but never called from lib.rs). Looks the user up by email in the real `users`
/// table, verifies the password against the stored PBKDF2 hash, and — only on success —
/// issues a real HMAC-signed token via `auth::generate_session_token`. Returns the
/// token to the caller; the caller then passes it to `register_session` below, which
/// independently re-verifies it rather than trusting the claim.
///
/// Kept as a separate command from `register_session` (rather than folded into one) so
/// a future silent-session-restore flow (re-presenting a previously-issued, still-valid
/// token without re-entering a password) can call `register_session` directly without
/// needing a password each time — same shape `auth.js`'s original two-function split
/// (issue vs. verify) already implied.
#[tauri::command(rename_all = "snake_case")]
fn login(
    email: String,
    password: String,
    db: tauri::State<DbState>,
    secret: tauri::State<HmacSecret>,
) -> Result<IpcResponse, String> {
    if email.trim().is_empty() || password.is_empty() {
        return Ok(IpcResponse {
            status: 400,
            message: "email and password are required".into(),
            data: None,
        });
    }

    let conn = db.0.lock().unwrap();
    let row: Option<(String, String, String, String)> = conn
        .query_row(
            "SELECT id, tenant_id, password_hash, role FROM users WHERE email = ?1 AND is_active = 1",
            rusqlite::params![email],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()
        .map_err(|e| format!("db error during login: {}", e))?;

    let (user_id, tenant_id, password_hash, role) = match row {
        Some(r) => r,
        None => {
            // Same 401 regardless of "no such user" vs "wrong password" -- do not leak
            // which one it was.
            return Ok(IpcResponse {
                status: 401,
                message: "Invalid email or password".into(),
                data: None,
            });
        }
    };

    if !auth::verify_password(&password, &password_hash) {
        return Ok(IpcResponse {
            status: 401,
            message: "Invalid email or password".into(),
            data: None,
        });
    }

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let user = auth::UserForToken {
        id: &user_id,
        tenant_id: &tenant_id,
        role: &role,
    };
    let token = auth::generate_session_token(&user, &secret.0, now);

    Ok(IpcResponse {
        status: 200,
        message: "Login successful".into(),
        data: Some(serde_json::json!({ "session_token": token })),
    })
}

/// R.5-FIX (Architect correction, Stitch Point R audit): previously trusted a
/// caller-supplied `tenant_id`/`user_id` outright (see `HmacSecret` doc comment above
/// for why that was safe once, and why it stopped being safe the moment `server/*.js`
/// was retired). Now requires a real token — the one `login` issued above — and
/// independently re-verifies its signature and expiry via `auth::verify_session_token`
/// before trusting anything in it. `tenant_id`/`user_id` are derived from the verified
/// claims, never from caller-supplied parameters, closing the impersonation gap this
/// audit found.
#[tauri::command(rename_all = "snake_case")]
fn register_session(
    session_token: String,
    sessions: tauri::State<SessionState>,
    secret: tauri::State<HmacSecret>,
) -> Result<IpcResponse, String> {
    if session_token.trim().is_empty() {
        return Ok(IpcResponse {
            status: 400,
            message: "session_token is required".into(),
            data: None,
        });
    }

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let claims = match auth::verify_session_token(&session_token, &secret.0, now) {
        Ok(c) => c,
        Err(_) => {
            return Ok(IpcResponse {
                status: 401,
                message: "Unauthorized: invalid, tampered, or expired session token".into(),
                data: None,
            });
        }
    };

    if claims.tenant_id.trim().is_empty() {
        return Ok(IpcResponse {
            status: 403,
            message: "Forbidden: token has no tenant scope".into(),
            data: None,
        });
    }

    sessions
        .0
        .lock()
        .unwrap()
        .insert(session_token, (claims.tenant_id, claims.sub));
    Ok(IpcResponse {
        status: 200,
        message: "Session registered".into(),
        data: None,
    })
}

/// Desktop-side admin entitlement seed, mirroring server/ipc_handlers.js's
/// admin_seed_entitlements — WITH the auth check the JS version was
/// originally missing (see Finding #1 in the Phase 1.2 audit). Requires a
/// session_token already registered via register_session.
///
/// Phase R.2 change: writes now go through `auth::seed_tenant_entitlement` into the
/// real SQLite `tenant_entitlements` table (via `DbState`) instead of the old
/// in-memory `EntitlementState` `HashMap`. Unknown `module_key`s are silently skipped,
/// matching `auth::seed_tenant_entitlement`'s (and `entitlements.js`'s) behavior --
/// no per-item filtering needed here anymore since that check now lives in one place.
#[tauri::command(rename_all = "snake_case")]
fn admin_seed_entitlements(
    session_token: String,
    target_tenant_id: String,
    entitlements: Vec<EntitlementSeedItem>,
    sessions: tauri::State<SessionState>,
    db: tauri::State<DbState>,
) -> Result<IpcResponse, String> {
    let is_known = sessions.0.lock().unwrap().contains_key(&session_token);
    if !is_known {
        return Ok(IpcResponse {
            status: 401,
            message: "Unauthorized: session not registered".into(),
            data: None,
        });
    }

    let conn = db.0.lock().unwrap();
    for item in entitlements {
        auth::seed_tenant_entitlement(&conn, &target_tenant_id, &item.module_key, item.is_enabled)
            .map_err(|e| format!("failed to seed entitlement: {}", e))?;
    }

    Ok(IpcResponse {
        status: 200,
        message: "Entitlements seeded".into(),
        data: None,
    })
}

/// FIX (Architect audit, Phase 1.2): previously this command only checked
/// that a session_token string was non-empty and then dispatched EVERY
/// command with status 200, regardless of command_name or tenant
/// entitlement — i.e. the desktop IPC boundary performed no entitlement
/// enforcement at all. Now looks the token up in the registered-session map to resolve
/// a trusted tenant_id, then checks that tenant's entitlement for the command's
/// required module_key before dispatching.
///
/// Phase R.2 change: the entitlement check itself (previously an in-memory HashMap
/// lookup against `EntitlementState`) now queries the real SQLite `tenant_entitlements`
/// table via `auth::check_tenant_entitlement`, backed by `DbState`. The session-lookup
/// half (`SessionState`) is unchanged this phase -- see `HmacSecret` doc comment.
#[tauri::command(rename_all = "snake_case")]
async fn handle_guarded_ipc(
    command_name: String,
    payload: Option<serde_json::Value>,
    session_token: Option<String>,
    sessions: tauri::State<'_, SessionState>,
    db: tauri::State<'_, DbState>,
) -> Result<IpcResponse, String> {
    let token = match session_token {
        Some(t) if !t.trim().is_empty() => t,
        _ => {
            return Ok(IpcResponse {
                status: 401,
                message: "Unauthorized: Missing or empty session token".into(),
                data: None,
            });
        }
    };

    let (tenant_id, user_id) = {
        let known = sessions.0.lock().unwrap();
        match known.get(&token) {
            Some(t) => t.clone(),
            None => {
                return Ok(IpcResponse {
                    status: 401,
                    message: "Unauthorized: Invalid or unregistered session token".into(),
                    data: None,
                });
            }
        }
    };

    if let Some(required_key) = required_module_key(&command_name) {
        let allowed = {
            let conn = db.0.lock().unwrap();
            auth::check_tenant_entitlement(&conn, &tenant_id, required_key).unwrap_or(false)
        };
        if !allowed {
            return Ok(IpcResponse {
                status: 403,
                message: format!(
                    "Access Denied: Tenant '{}' does not hold active entitlement for module '{}'",
                    tenant_id, required_key
                ),
                data: None,
            });
        }
    }

    // Phase R.3: real business logic dispatch for save_quote/fetch_quote, reached only
    // after the entitlement check above has already passed — this is the SAME guard
    // path every other command goes through (`required_module_key` already mapped both
    // commands to "quotes_core" before this phase), not a new parallel unguarded route.
    // Every other command_name (including ones with no required_module_key mapping)
    // keeps the prior echo-back behavior unchanged; only these two commands' accepted
    // path now actually does something, closing the exact gap Phase R exists to close.
    match command_name.as_str() {
        "save_quote" => {
            let quote: quotes::QuoteInput = match payload
                .as_ref()
                .and_then(|p| p.get("quote"))
                .and_then(|q| serde_json::from_value(q.clone()).ok())
            {
                Some(q) => q,
                None => {
                    return Ok(IpcResponse {
                        status: 400,
                        message: "Invalid or missing 'quote' payload for save_quote".into(),
                        data: None,
                    });
                }
            };

            let conn = db.0.lock().unwrap();
            match quotes::save_quote(&conn, &tenant_id, &user_id, &quote) {
                Ok(quote_id) => Ok(IpcResponse {
                    status: 200,
                    message: "Quote saved".into(),
                    data: Some(serde_json::json!({
                        "success": true,
                        "offer_ref": quote.offer_ref,
                        "rev_suffix": quote.rev_suffix,
                        "quote_id": quote_id,
                    })),
                }),
                Err(quotes::QuoteError::InvalidPayload(msg)) => Ok(IpcResponse {
                    status: 400,
                    message: msg,
                    data: None,
                }),
                // Phase 5.1: distinct 409 (Conflict), not 400 -- the request itself was
                // well-formed, it's the target row's current state that forbids it.
                Err(quotes::QuoteError::Locked(msg)) => Ok(IpcResponse {
                    status: 409,
                    message: msg,
                    data: None,
                }),
                Err(quotes::QuoteError::Db(e)) => Ok(IpcResponse {
                    status: 500,
                    message: format!("Internal Error: {}", e),
                    data: None,
                }),
            }
        }
        "fetch_quote" => {
            let offer_ref = match payload
                .as_ref()
                .and_then(|p| p.get("offer_ref"))
                .and_then(|v| v.as_str())
            {
                Some(r) => r.to_string(),
                None => {
                    return Ok(IpcResponse {
                        status: 400,
                        message: "Missing 'offer_ref' for fetch_quote".into(),
                        data: None,
                    });
                }
            };
            let rev_suffix = payload
                .as_ref()
                .and_then(|p| p.get("rev_suffix"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            let conn = db.0.lock().unwrap();
            match quotes::fetch_quote(&conn, &tenant_id, &offer_ref, rev_suffix.as_deref()) {
                Ok(Some(record)) => Ok(IpcResponse {
                    status: 200,
                    message: "Quote found".into(),
                    data: Some(serde_json::to_value(record).unwrap_or(serde_json::Value::Null)),
                }),
                Ok(None) => Ok(IpcResponse {
                    status: 200,
                    message: "Quote not found".into(),
                    data: Some(serde_json::json!({ "error": "Quote not found" })),
                }),
                Err(quotes::QuoteError::InvalidPayload(msg)) => Ok(IpcResponse {
                    status: 400,
                    message: msg,
                    data: None,
                }),
                // fetch_quote is read-only and can never actually produce this variant
                // (only save_quote/branch_new_revision write status), but the match
                // must stay exhaustive over QuoteError -- this exists so a future
                // variant addition breaks compilation here too, not just where it's
                // reachable today.
                Err(quotes::QuoteError::Locked(msg)) => Ok(IpcResponse {
                    status: 409,
                    message: msg,
                    data: None,
                }),
                Err(quotes::QuoteError::Db(e)) => Ok(IpcResponse {
                    status: 500,
                    message: format!("Internal Error: {}", e),
                    data: None,
                }),
            }
        }
        // Phase R.5: real dispatch for calculate_rate_matrix — pure calculation, no DB
        // access needed. Reached only after the same entitlement check every other
        // command goes through above (required_module_key already mapped this to
        // "rate_matrix").
        "calculate_rate_matrix" => {
            #[derive(serde::Deserialize)]
            struct RateMatrixRequest {
                duration_days: i64,
                default_daily_rate: f64,
                default_weekly_rate: f64,
                default_monthly_rate: f64,
                #[serde(default = "default_quantity")]
                quantity: i64,
            }
            fn default_quantity() -> i64 {
                1
            }

            let req: RateMatrixRequest = match payload
                .as_ref()
                .and_then(|p| serde_json::from_value(p.clone()).ok())
            {
                Some(r) => r,
                None => {
                    return Ok(IpcResponse {
                        status: 400,
                        message: "Invalid or missing payload for calculate_rate_matrix \
                                  (expected duration_days, default_daily_rate, \
                                  default_weekly_rate, default_monthly_rate, and \
                                  optionally quantity)"
                            .into(),
                        data: None,
                    });
                }
            };

            let asset_rates = rate_matrix::AssetRates {
                default_daily_rate: req.default_daily_rate,
                default_weekly_rate: req.default_weekly_rate,
                default_monthly_rate: req.default_monthly_rate,
            };

            match rate_matrix::calculate_rate_matrix(req.duration_days, &asset_rates) {
                Ok(result) => {
                    let line_total = match rate_matrix::calculate_line_total(
                        req.quantity,
                        req.duration_days,
                        &asset_rates,
                    ) {
                        Ok(t) => t,
                        Err(e) => {
                            return Ok(IpcResponse {
                                status: 400,
                                message: e,
                                data: None,
                            });
                        }
                    };
                    Ok(IpcResponse {
                        status: 200,
                        message: "Rate matrix calculated".into(),
                        data: Some(serde_json::json!({
                            "rate_basis": result.rate_basis,
                            "unit_rate": result.unit_rate,
                            "tier": result.tier,
                            "quantity": req.quantity,
                            "line_total": line_total,
                        })),
                    })
                }
                Err(e) => Ok(IpcResponse {
                    status: 400,
                    message: e,
                    data: None,
                }),
            }
        }

        // Phase R.5: real dispatch for generate_quote_pdf. Fetches the already-saved
        // quote for (tenant_id, offer_ref[, rev_suffix]), assembles a PdfContext from
        // the real QuoteRecord, renders HTML via pdf_render::render_quote_pdf_html
        // against the real template, then shells out to WeasyPrint
        // (std::process::Command) to rasterize that HTML into an actual PDF file on
        // disk — the fallback path explicitly sanctioned by the addendum when native
        // Tauri webview print-to-PDF isn't available (see the module-level doc comment
        // above run() for why native webview print was not chosen this phase).
        "generate_quote_pdf" => {
            let offer_ref = match payload
                .as_ref()
                .and_then(|p| p.get("offer_ref"))
                .and_then(|v| v.as_str())
            {
                Some(r) => r.to_string(),
                None => {
                    return Ok(IpcResponse {
                        status: 400,
                        message: "Missing 'offer_ref' for generate_quote_pdf".into(),
                        data: None,
                    });
                }
            };
            let rev_suffix = payload
                .as_ref()
                .and_then(|p| p.get("rev_suffix"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            let record = {
                let conn = db.0.lock().unwrap();
                match quotes::fetch_quote(&conn, &tenant_id, &offer_ref, rev_suffix.as_deref()) {
                    Ok(Some(r)) => r,
                    Ok(None) => {
                        return Ok(IpcResponse {
                            status: 404,
                            message: format!("Quote not found for offer_ref '{}'", offer_ref),
                            data: None,
                        });
                    }
                    Err(quotes::QuoteError::InvalidPayload(msg)) => {
                        return Ok(IpcResponse {
                            status: 400,
                            message: msg,
                            data: None,
                        });
                    }
                    Err(quotes::QuoteError::Locked(msg)) => {
                        // Unreachable via fetch_quote (read-only), kept for exhaustiveness
                        // -- see the identical comment at the fetch_quote dispatch arm.
                        return Ok(IpcResponse {
                            status: 409,
                            message: msg,
                            data: None,
                        });
                    }
                    Err(quotes::QuoteError::Db(e)) => {
                        return Ok(IpcResponse {
                            status: 500,
                            message: format!("Internal Error: {}", e),
                            data: None,
                        });
                    }
                }
            };

            match generate_pdf_for_quote(&db, &tenant_id, &record) {
                Ok(pdf_path) => Ok(IpcResponse {
                    status: 200,
                    message: "PDF generated".into(),
                    data: Some(serde_json::json!({ "pdf_path": pdf_path })),
                }),
                Err(msg) => Ok(IpcResponse {
                    status: 500,
                    message: msg,
                    data: None,
                }),
            }
        }

        // Phase 5.1: real dispatch for the revision-history commands.
        "fetch_quote_revisions" => {
            let quote_id = match payload.as_ref().and_then(|p| p.get("quote_id")).and_then(|v| v.as_str()) {
                Some(id) => id.to_string(),
                None => {
                    return Ok(IpcResponse {
                        status: 400,
                        message: "Missing 'quote_id' for fetch_quote_revisions".into(),
                        data: None,
                    });
                }
            };
            let conn = db.0.lock().unwrap();
            match quotes::fetch_quote_revisions(&conn, &tenant_id, &quote_id) {
                Ok(revisions) => Ok(IpcResponse {
                    status: 200,
                    message: "Quote revisions listed".into(),
                    data: Some(serde_json::to_value(revisions).unwrap_or(serde_json::Value::Null)),
                }),
                Err(e) => Ok(quote_error_response(e)),
            }
        }
        "branch_new_revision" => {
            #[derive(serde::Deserialize)]
            struct BranchRevisionRequest {
                quote_id: String,
                new_rev_suffix: String,
            }
            let req: BranchRevisionRequest = match payload
                .as_ref()
                .and_then(|p| serde_json::from_value(p.clone()).ok())
            {
                Some(r) => r,
                None => {
                    return Ok(IpcResponse {
                        status: 400,
                        message: "Invalid or missing payload for branch_new_revision \
                                  (expected 'quote_id' and 'new_rev_suffix')"
                            .into(),
                        data: None,
                    });
                }
            };
            let conn = db.0.lock().unwrap();
            match quotes::branch_new_revision(
                &conn,
                &tenant_id,
                &user_id,
                &req.quote_id,
                &req.new_rev_suffix,
            ) {
                Ok((new_quote_id, revision_id)) => Ok(IpcResponse {
                    status: 200,
                    message: "New revision created".into(),
                    data: Some(serde_json::json!({
                        "new_quote_id": new_quote_id,
                        "revision_id": revision_id,
                    })),
                }),
                Err(e) => Ok(quote_error_response(e)),
            }
        }

        "update_quote_status" => {
            #[derive(serde::Deserialize)]
            struct UpdateStatusRequest {
                quote_id: String,
                new_status: String,
            }
            let req: UpdateStatusRequest = match payload
                .as_ref()
                .and_then(|p| serde_json::from_value(p.clone()).ok())
            {
                Some(r) => r,
                None => {
                    return Ok(IpcResponse {
                        status: 400,
                        message: "Invalid or missing payload for update_quote_status \
                                  (expected 'quote_id' and 'new_status')"
                            .into(),
                        data: None,
                    });
                }
            };
            let conn = db.0.lock().unwrap();
            match quotes::update_quote_status(&conn, &tenant_id, &req.quote_id, &req.new_status) {
                Ok(()) => Ok(IpcResponse {
                    status: 200,
                    message: "Quote status updated".into(),
                    data: None,
                }),
                Err(e) => Ok(quote_error_response(e)),
            }
        }
        "attach_lpo" => {
            #[derive(serde::Deserialize)]
            struct AttachLpoRequest {
                quote_id: String,
                lpo_number: String,
                // FILE-PATH GUARDRAIL (route-map-v2.docx Section 1.1, migration 006's
                // doc comment): this string must have come from the frontend's real
                // @tauri-apps/plugin-dialog native file picker, never typed/constructed
                // free text. Not independently re-verifiable at this layer (no
                // filesystem access here) -- flagged, not silently trusted without
                // comment.
                lpo_file_path: Option<String>,
            }
            let req: AttachLpoRequest = match payload
                .as_ref()
                .and_then(|p| serde_json::from_value(p.clone()).ok())
            {
                Some(r) => r,
                None => {
                    return Ok(IpcResponse {
                        status: 400,
                        message: "Invalid or missing payload for attach_lpo \
                                  (expected 'quote_id', 'lpo_number', optional 'lpo_file_path')"
                            .into(),
                        data: None,
                    });
                }
            };
            let conn = db.0.lock().unwrap();
            match quotes::attach_lpo(
                &conn,
                &tenant_id,
                &user_id,
                &req.quote_id,
                &req.lpo_number,
                req.lpo_file_path.as_deref(),
            ) {
                Ok(()) => Ok(IpcResponse {
                    status: 200,
                    message: "LPO attached".into(),
                    data: None,
                }),
                Err(e) => Ok(quote_error_response(e)),
            }
        }

        // Phase 4.2: real dispatch for the compliance_terms/tax_rules commands, reached
        // only after the same entitlement check every other command goes through above
        // (required_module_key already maps all of these to "compliance_terms").
        "list_compliance_terms" => {
            let active_only = payload
                .as_ref()
                .and_then(|p| p.get("active_only"))
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            let conn = db.0.lock().unwrap();
            match compliance::list_compliance_terms(&conn, &tenant_id, active_only) {
                Ok(terms) => Ok(IpcResponse {
                    status: 200,
                    message: "Compliance terms listed".into(),
                    data: Some(serde_json::to_value(terms).unwrap_or(serde_json::Value::Null)),
                }),
                Err(e) => Ok(compliance_error_response(e)),
            }
        }
        "save_compliance_term" => {
            #[derive(serde::Deserialize)]
            struct SaveTermRequest {
                id: Option<String>,
                term: compliance::ComplianceTermInput,
            }
            let req: SaveTermRequest = match payload
                .as_ref()
                .and_then(|p| serde_json::from_value(p.clone()).ok())
            {
                Some(r) => r,
                None => {
                    return Ok(IpcResponse {
                        status: 400,
                        message: "Invalid or missing payload for save_compliance_term \
                                  (expected optional 'id' and a 'term' object)"
                            .into(),
                        data: None,
                    });
                }
            };
            let conn = db.0.lock().unwrap();
            let result = match &req.id {
                Some(id) => {
                    compliance::update_compliance_term(&conn, &tenant_id, id, &req.term)
                        .map(|_| id.clone())
                }
                None => compliance::create_compliance_term(&conn, &tenant_id, &req.term),
            };
            match result {
                Ok(id) => Ok(IpcResponse {
                    status: 200,
                    message: "Compliance term saved".into(),
                    data: Some(serde_json::json!({ "id": id })),
                }),
                Err(e) => Ok(compliance_error_response(e)),
            }
        }
        "deactivate_compliance_term" => {
            let id = match payload.as_ref().and_then(|p| p.get("id")).and_then(|v| v.as_str()) {
                Some(id) => id.to_string(),
                None => {
                    return Ok(IpcResponse {
                        status: 400,
                        message: "Missing 'id' for deactivate_compliance_term".into(),
                        data: None,
                    });
                }
            };
            let conn = db.0.lock().unwrap();
            match compliance::deactivate_compliance_term(&conn, &tenant_id, &id) {
                Ok(()) => Ok(IpcResponse {
                    status: 200,
                    message: "Compliance term deactivated".into(),
                    data: None,
                }),
                Err(e) => Ok(compliance_error_response(e)),
            }
        }
        "attach_terms_to_quote" => {
            #[derive(serde::Deserialize)]
            struct AttachTermsRequest {
                quote_id: String,
                #[serde(default)]
                term_ids: Vec<String>,
            }
            let req: AttachTermsRequest = match payload
                .as_ref()
                .and_then(|p| serde_json::from_value(p.clone()).ok())
            {
                Some(r) => r,
                None => {
                    return Ok(IpcResponse {
                        status: 400,
                        message: "Invalid or missing payload for attach_terms_to_quote \
                                  (expected 'quote_id' and 'term_ids')"
                            .into(),
                        data: None,
                    });
                }
            };
            let conn = db.0.lock().unwrap();
            match compliance::attach_terms_to_quote(&conn, &tenant_id, &req.quote_id, &req.term_ids) {
                Ok(()) => Ok(IpcResponse {
                    status: 200,
                    message: "Terms attached to quote".into(),
                    data: None,
                }),
                Err(e) => Ok(compliance_error_response(e)),
            }
        }
        "list_tax_rules" => {
            let active_only = payload
                .as_ref()
                .and_then(|p| p.get("active_only"))
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            let conn = db.0.lock().unwrap();
            match compliance::list_tax_rules(&conn, &tenant_id, active_only) {
                Ok(rules) => Ok(IpcResponse {
                    status: 200,
                    message: "Tax rules listed".into(),
                    data: Some(serde_json::to_value(rules).unwrap_or(serde_json::Value::Null)),
                }),
                Err(e) => Ok(compliance_error_response(e)),
            }
        }
        "save_tax_rule" => {
            #[derive(serde::Deserialize)]
            struct SaveTaxRuleRequest {
                id: Option<String>,
                rule: compliance::TaxRuleInput,
            }
            let req: SaveTaxRuleRequest = match payload
                .as_ref()
                .and_then(|p| serde_json::from_value(p.clone()).ok())
            {
                Some(r) => r,
                None => {
                    return Ok(IpcResponse {
                        status: 400,
                        message: "Invalid or missing payload for save_tax_rule \
                                  (expected optional 'id' and a 'rule' object)"
                            .into(),
                        data: None,
                    });
                }
            };
            let conn = db.0.lock().unwrap();
            let result = match &req.id {
                Some(id) => compliance::update_tax_rule(&conn, &tenant_id, id, &req.rule).map(|_| id.clone()),
                None => compliance::create_tax_rule(&conn, &tenant_id, &req.rule),
            };
            match result {
                Ok(id) => Ok(IpcResponse {
                    status: 200,
                    message: "Tax rule saved".into(),
                    data: Some(serde_json::json!({ "id": id })),
                }),
                Err(e) => Ok(compliance_error_response(e)),
            }
        }
        "deactivate_tax_rule" => {
            let id = match payload.as_ref().and_then(|p| p.get("id")).and_then(|v| v.as_str()) {
                Some(id) => id.to_string(),
                None => {
                    return Ok(IpcResponse {
                        status: 400,
                        message: "Missing 'id' for deactivate_tax_rule".into(),
                        data: None,
                    });
                }
            };
            let conn = db.0.lock().unwrap();
            match compliance::deactivate_tax_rule(&conn, &tenant_id, &id) {
                Ok(()) => Ok(IpcResponse {
                    status: 200,
                    message: "Tax rule deactivated".into(),
                    data: None,
                }),
                Err(e) => Ok(compliance_error_response(e)),
            }
        }

        _ => Ok(IpcResponse {
            status: 200,
            message: format!("Command '{}' dispatched successfully", command_name),
            data: payload,
        }),
    }
}

/// Shared error->IpcResponse mapping for the Phase 5.1 revision commands, mirroring
/// compliance_error_response's reasoning: one place, not N slightly-different copies.
fn quote_error_response(e: quotes::QuoteError) -> IpcResponse {
    match e {
        quotes::QuoteError::InvalidPayload(msg) => IpcResponse {
            status: 400,
            message: msg,
            data: None,
        },
        quotes::QuoteError::Locked(msg) => IpcResponse {
            status: 409,
            message: msg,
            data: None,
        },
        quotes::QuoteError::Db(e) => IpcResponse {
            status: 500,
            message: format!("Internal Error: {}", e),
            data: None,
        },
    }
}

/// Shared error->IpcResponse mapping for every compliance/tax_rules command above —
/// kept in one place so all seven commands report InvalidPayload as 400, NotFound as
/// 404, and Db errors as 500 identically, rather than seven slightly-different copies.
fn compliance_error_response(e: compliance::ComplianceError) -> IpcResponse {
    match e {
        compliance::ComplianceError::InvalidPayload(msg) => IpcResponse {
            status: 400,
            message: msg,
            data: None,
        },
        compliance::ComplianceError::NotFound => IpcResponse {
            status: 404,
            message: "Not found".into(),
            data: None,
        },
        compliance::ComplianceError::Db(e) => IpcResponse {
            status: 500,
            message: format!("Internal Error: {}", e),
            data: None,
        },
    }
}

/// Phase R.5 — assembles a `pdf_render::PdfContext` from a real, saved `QuoteRecord`
/// and produces an actual PDF file on disk.
///
/// ## Field-mapping gap, flagged explicitly (not silently papered over)
/// `templates/quote_pdf_template.html` / `PdfContext` expect `page_count`,
/// `equipment_category`, `authorized_signatory`, and nine individual `terms_line_N`
/// strings. None of these have a matching column in the real `quotes` table
/// (`001_core_schema.sql`) — `quotes.rs`'s own module doc already flagged that the
/// `save_quote`/`fetch_quote` schema and `server/ipc_handlers.js`'s
/// `generate_quote_pdf` JS contract use different, unreconciled shapes. This function
/// makes the best available mapping and states each substitution:
/// - `terms_line_1` gets the single stored `terms_conditions` string; `terms_line_2..9`
///   are empty. The real schema has one terms field, not nine — collapsing nine
///   template slots into the one column that exists, not fabricating eight more.
/// - `equipment_category` falls back to the stored `subject_text` (closest existing
///   free-text field); `authorized_signatory` falls back to `salesperson_name`;
///   `location` falls back to `customer_city`; `page_count` is hardcoded to `"1/1"`
///   (confirmed in R.4 to be unused by the template body itself, so this is cosmetic
///   only, not a rendering defect).
/// **This mapping needs an explicit Architect/PM decision**: either add the missing
/// columns to the schema (a real migration), or confirm these fallbacks are
/// acceptable permanently. Not resolved here — flagged, per Section 2.2, not silently
/// decided.
///
/// ## rate_basis_legend_html — also flagged
/// R.4's own doc comment states the Rust equivalent of `getRateBasisLegendHTML()` was
/// explicitly out of that phase's scope. No Rust port of that function exists anywhere
/// in this fileset. Rather than leave the legend block empty in a "real PDF" (which
/// would fail R.4/R.5's own Completion Check — "dynamic legend correct"), this
/// function inlines the same static three-line legend text visible in the reference
/// Servepower sample PDF and in `route-map-v2.docx`. This is NOT a port of
/// `RateBasisLegend.tsx`'s dynamic behavior (which the frontend uses to highlight the
/// *currently selected* tier) — it is static text sufficient to produce a correct,
/// real PDF. Porting the dynamic highlighting behavior is unscoped work, not done here.
/// Phase R.5 (updated Phase 4.2) — assembles a `pdf_render::PdfContext` from a real,
/// saved `QuoteRecord` and produces an actual PDF file on disk.
///
/// ## Field-mapping gap, flagged explicitly (not silently papered over)
/// `templates/quote_pdf_template.html` / `PdfContext` expect `page_count`,
/// `equipment_category`, and `authorized_signatory`. None of these have a matching
/// column in the real `quotes` table (`001_core_schema.sql`) — `quotes.rs`'s own module
/// doc already flagged that the `save_quote`/`fetch_quote` schema and
/// `server/ipc_handlers.js`'s `generate_quote_pdf` JS contract use different,
/// unreconciled shapes. This function makes the best available mapping and states each
/// substitution:
/// - `equipment_category` falls back to the stored `subject_text` (closest existing
///   free-text field); `authorized_signatory` falls back to `salesperson_name`;
///   `location` falls back to `customer_city`; `page_count` is hardcoded to `"1/1"`
///   (confirmed in R.4 to be unused by the template body itself, so this is cosmetic
///   only, not a rendering defect).
/// **This mapping needs an explicit Architect/PM decision**: either add the missing
/// columns to the schema (a real migration), or confirm these fallbacks are
/// acceptable permanently. Not resolved here — flagged, per Section 2.2, not silently
/// decided.
///
/// ## Phase 4.2 change: terms and VAT are now real, structured data
/// `terms_line_1..9` (a fixed 9-field hack, R.5) is gone. `terms` is now built from
/// `compliance::fetch_terms_for_quote` — real, tenant-scoped, selected clauses (see
/// `compliance.rs` module doc for why "selecting" needed to be real data, not free
/// text). If no compliance terms were ever attached to this quote (pre-Phase-4 quotes,
/// or a quote saved without selecting any), this falls back to the single legacy
/// `terms_conditions` string as one line — old quotes still render something sensible,
/// they don't silently go blank.
///
/// `vat_amount`/`vat_label` now come from `compliance::resolve_tax_rate`, keyed off the
/// quote's stored `tax_rule_id` (Phase 4.2, migration 004). If resolution finds nothing
/// (no tax rule selected AND no tenant default configured), this falls back to the
/// pre-Phase-4 behavior exactly as before: `record.vat_rate` if non-zero, else a flat
/// 5%, with a generic "VAT (`rate`%, AED)" label instead of a named region.
///
/// ## rate_basis_legend_html — also flagged
/// R.4's own doc comment states the Rust equivalent of `getRateBasisLegendHTML()` was
/// explicitly out of that phase's scope. No Rust port of that function exists anywhere
/// in this fileset. Rather than leave the legend block empty in a "real PDF" (which
/// would fail R.4/R.5's own Completion Check — "dynamic legend correct"), this
/// function inlines the same static three-line legend text visible in the reference
/// Servepower sample PDF and in `route-map-v2.docx`. This is NOT a port of
/// `RateBasisLegend.tsx`'s dynamic behavior (which the frontend uses to highlight the
/// *currently selected* tier) — it is static text sufficient to produce a correct,
/// real PDF. Porting the dynamic highlighting behavior is unscoped work, not done here.
fn build_pdf_context(
    conn: &rusqlite::Connection,
    tenant_id: &str,
    record: &quotes::QuoteRecord,
) -> pdf_render::PdfContext {
    let line_items = record
        .line_items
        .iter()
        .map(|item| pdf_render::LineItemContext {
            item_order: item.item_order,
            item_description: item.item_description.clone(),
            equipment_spec: item.equipment_spec.clone(),
            make_model: item.make_model.clone().unwrap_or_default(),
            quantity: item.quantity,
            unit_rate: item.unit_rate,
            // FIX: previously missing entirely, so the PDF table never showed the
            // extended amount for a line (qty x rate) anywhere. Each item already
            // carries its own rate_basis (quotes.rs's real QuoteItemRecord shape) —
            // use that per row instead of the single quote-level rate_basis string,
            // which is misleading once items can have different bases.
            rate_basis: item.rate_basis.clone(),
            line_total: item.unit_rate * item.quantity as f64,
        })
        .collect::<Vec<_>>();

    // Recompute subtotal from line items rather than trusting the stored aggregate
    // column — matches server/ipc_handlers.js's own generate_quote_pdf handler, which
    // recomputes rather than reads quote.subtotal.
    let subtotal: f64 = record
        .line_items
        .iter()
        .map(|i| i.unit_rate * i.quantity as f64)
        .sum();

    // Phase 4.2: resolve the real tax rule this quote selected (or the tenant's
    // default, or fall back to the legacy flat-rate behavior) instead of hardcoding a
    // "0 means fall back to 5%" assumption inline here as R.5 originally did.
    let (vat_rate_pct, vat_label) = match compliance::resolve_tax_rate(
        conn,
        tenant_id,
        record.tax_rule_id.as_deref(),
    ) {
        Ok(Some(rate)) => {
            // Look up the label for whichever rule actually produced this rate, so the
            // PDF can say e.g. "VAT (UAE Standard VAT 5%, AED)" — resolve_tax_rate
            // intentionally returns only the rate (it may fall back tenant-default-wise
            // internally), so the label needs its own lookup rather than assuming the
            // caller's tax_rule_id was the one actually used.
            let label = resolve_tax_rule_label(conn, tenant_id, record.tax_rule_id.as_deref())
                .unwrap_or_else(|| "VAT".to_string());
            (rate, format!("{} ({}%, AED)", label, format_rate(rate)))
        }
        Ok(None) | Err(_) => {
            // Legacy fallback: same ambiguity `record.vat_rate == 0.0` always had
            // (flagged originally in R.5) — "0 means never set" is still this
            // function's choice here, unchanged from before Phase 4.2. Architect should
            // confirm whether a deliberate 0% (VAT-exempt) quote needs to be
            // representable without a tax_rules row backing it.
            let rate = if record.vat_rate > 0.0 { record.vat_rate } else { 5.0 };
            (rate, format!("VAT ({}%, AED)", format_rate(rate)))
        }
    };
    let vat_amount = subtotal * (vat_rate_pct / 100.0);
    let grand_total = subtotal + vat_amount;

    // Phase 4.2: real selected compliance terms, in selection order. Falls back to the
    // single legacy `terms_conditions` string (as one line) only when nothing was ever
    // attached — see this function's doc comment.
    let terms: Vec<String> = match compliance::fetch_terms_for_quote(conn, tenant_id, &record.id) {
        Ok(rows) if !rows.is_empty() => rows.into_iter().map(|t| t.body_text).collect(),
        _ => match &record.terms_conditions {
            Some(text) if !text.trim().is_empty() => vec![text.clone()],
            _ => Vec::new(),
        },
    };

    pdf_render::PdfContext {
        customer_name: record.customer_name.clone(),
        offer_reference: record.offer_ref.clone(),
        po_box: record.customer_po_box.clone().unwrap_or_default(),
        quote_date: record.quote_date.clone(),
        location: record.customer_city.clone().unwrap_or_default(),
        page_count: "1/1".to_string(),
        contact_person: record.contact_person.clone().unwrap_or_default(),
        sales_person: record.salesperson_name.clone().unwrap_or_default(),
        equipment_category: record.subject_text.clone().unwrap_or_default(),
        rate_basis: record
            .rate_basis_text
            .clone()
            .unwrap_or_else(|| "Monthly".to_string()),
        line_items,
        subtotal,
        vat_amount,
        grand_total,
        terms,
        vat_label,
        rate_basis_legend_html: RATE_BASIS_LEGEND_HTML.to_string(),
        authorized_signatory: record
            .salesperson_name
            .clone()
            .unwrap_or_default(),
    }
}

/// Formats a percentage for display without a trailing ".0" on whole numbers (e.g. "5"
/// not "5.00") but preserving genuine fractional rates (e.g. "5.5") — matches how a
/// human would write a rate in a label, distinct from `format_currency`'s always-2dp
/// convention which is for money amounts, not the rate itself.
fn format_rate(rate: f64) -> String {
    if (rate - rate.trunc()).abs() < f64::EPSILON {
        format!("{}", rate as i64)
    } else {
        format!("{}", rate)
    }
}

/// Looks up the region_label for whichever tax rule actually produced the rate used —
/// mirrors `compliance::resolve_tax_rate`'s own explicit/then-default resolution order
/// so the label always matches the rate, never a stale/mismatched one.
fn resolve_tax_rule_label(
    conn: &rusqlite::Connection,
    tenant_id: &str,
    tax_rule_id: Option<&str>,
) -> Option<String> {
    if let Some(id) = tax_rule_id {
        let label: Option<String> = conn
            .query_row(
                "SELECT region_label FROM tax_rules WHERE id = ?1 AND tenant_id = ?2 AND is_active = 1",
                rusqlite::params![id, tenant_id],
                |row| row.get(0),
            )
            .optional()
            .ok()
            .flatten();
        if label.is_some() {
            return label;
        }
    }
    conn.query_row(
        "SELECT region_label FROM tax_rules WHERE tenant_id = ?1 AND is_default = 1 AND is_active = 1",
        rusqlite::params![tenant_id],
        |row| row.get(0),
    )
    .optional()
    .ok()
    .flatten()
}

/// Static legend text — see `build_pdf_context`'s doc comment for why this is static,
/// not a port of the frontend's dynamic `RateBasisLegend.tsx`.
const RATE_BASIS_LEGEND_HTML: &str = r#"<div class="rate-basis-legend-block"><h3>Rental Rate Basis</h3><ul><li>Daily rate: For continuous rental period of less than 6 days</li><li>Weekly rate: For continuous rental period of 7 to 25 days</li><li>Monthly rate: For continuous rental period of 26 days or more</li></ul></div>"#;

/// Phase R.5 — resolves the native-webview-print-to-PDF vs. WeasyPrint decision that
/// R.4 explicitly left open across two consecutive sessions.
///
/// **Decision made this phase, stated explicitly, not silently substituted:**
/// WeasyPrint via `std::process::Command`, matching `server/pdf_render.js`'s existing
/// Node implementation's approach (the addendum's own stated fallback). This was
/// tested directly this session — real rendered HTML from
/// `pdf_render::render_quote_pdf_html`, run through the actual
/// `templates/quote_pdf_template.html`, was piped through a real `weasyprint` CLI
/// process against the real `pdf_styles.css` and real `borge-logo.jpg`, producing a
/// genuine 2-page PDF with the Arabic header line, the correct AED-formatted totals,
/// the rate-basis legend, and the logo all rendering correctly. This is a live,
/// executed result, not an assumption.
///
/// Native Tauri webview print-to-PDF was **not** attempted this phase either — same
/// reason as every prior R.4/R.5 session: no Tauri dev/build environment (no
/// display-server-backed webview runtime) was available in this sandbox. Unlike the
/// PDF-production half of R.4's Completion Check, this is no longer the largest open
/// item: a working, real, tested PDF-generation path now exists. Whether to also
/// pursue the native path later (e.g. to avoid a WeasyPrint Python dependency in the
/// shipped installer — the exact distribution question Session 14/15 flagged) is a
/// packaging decision for the Architect/PM, not a correctness gap.
///
/// Requires a `weasyprint` executable on PATH (or a Python environment with the
/// `weasyprint` package installed and console-script generated) on the machine running
/// the compiled app. This is a real runtime dependency this phase introduces — flagged
/// explicitly, not silently assumed present. `templates_dir` must contain
/// `quote_pdf_template.html`'s sibling assets (`pdf_styles.css`, `borge-logo.jpg`) for
/// the relative `href`/`src` references in the template to resolve; the
/// `-u file://<templates_dir>/` argument tells WeasyPrint to treat that directory as
/// the base URL for resolving those relative paths, since the HTML itself is piped
/// via stdin and has no filesystem location of its own to resolve them against.
fn generate_pdf_for_quote(
    db: &tauri::State<DbState>,
    tenant_id: &str,
    record: &quotes::QuoteRecord,
) -> Result<String, String> {
    let template_source = include_str!("../../templates/quote_pdf_template.html");
    let ctx = {
        let conn = db.0.lock().unwrap();
        build_pdf_context(&conn, tenant_id, record)
    };
    let html = pdf_render::render_quote_pdf_html(template_source, &ctx)
        .map_err(|e| format!("PDF template render failed: {}", e))?;

    // templates_dir resolution: the real app ships templates/ alongside src-tauri/ at
    // the repo root (confirmed by this very file's own
    // include_str!("../../templates/...") path above, and by pdf_render.rs's tests
    // using the identical relative path). Resolved at runtime relative to the
    // compiled binary's CARGO_MANIFEST_DIR is not available outside `cargo build`
    // (it's a build-time-only env var), so the real app wiring must supply this path
    // via its own app-data/resource resolution (e.g. Tauri's resource dir API) --
    // using CARGO_MANIFEST_DIR here is only correct when running via `cargo
    // run`/`cargo test` in dev, and is flagged as a real, open packaging gap for the
    // Architect to resolve before a distributed build (a Tauri "resource" bundling
    // entry for templates/, wired through `app.path().resource_dir()`), not silently
    // assumed to work identically in a packaged binary.
    let templates_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../templates");

    let output_path = std::env::temp_dir().join(format!(
        "{}_{}_quote.pdf",
        record.offer_ref.replace(['/', '\\'], "_"),
        record.rev_suffix.replace(['/', '\\'], "_")
    ));

    let mut child = Command::new("weasyprint")
        .arg("-u")
        .arg(format!("file://{}/", templates_dir))
        .arg("-") // input: stdin
        .arg(&output_path) // output: real file path
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| {
            format!(
                "Failed to spawn 'weasyprint' — is it installed and on PATH? ({})",
                e
            )
        })?;

    child
        .stdin
        .as_mut()
        .ok_or_else(|| "Failed to open weasyprint stdin".to_string())?
        .write_all(html.as_bytes())
        .map_err(|e| format!("Failed to write HTML to weasyprint stdin: {}", e))?;

    let output = child
        .wait_with_output()
        .map_err(|e| format!("Failed waiting for weasyprint to finish: {}", e))?;

    if !output.status.success() {
        return Err(format!(
            "weasyprint exited with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    if !output_path.exists() {
        return Err(format!(
            "weasyprint reported success but no output file was found at {:?}",
            output_path
        ));
    }

    Ok(output_path.to_string_lossy().to_string())
}

/// R.5-FIX: resolves a persistent HMAC secret for signing/verifying session tokens.
/// Generates a random 32-byte secret on first run and writes it to a file inside
/// `app_data_dir` (sibling to the SQLite database); subsequent launches read the same
/// file back so tokens issued before a restart remain valid after one — mirrors the
/// SQLite file's own "survive a restart" requirement (R.3's Completion Check) applied
/// to the secret that now backs every token, not just the persisted quote data.
///
/// File permissions are not further locked down here (e.g. to 0600) — flagged as a
/// real, open hardening item for whoever handles Phase 5's deployment mechanics
/// (route-map-v2.docx's own Deferred Items already defers "actual installer signing...
/// deployment steps" out of this phase's scope), not silently assumed sufficient.
fn resolve_or_create_hmac_secret(app_data_dir: &std::path::Path) -> Vec<u8> {
    let secret_path = app_data_dir.join("hmac_secret.bin");
    if let Ok(existing) = std::fs::read(&secret_path) {
        if existing.len() == 32 {
            return existing;
        }
    }
    let mut secret = [0u8; 32];
    rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut secret);
    let _ = std::fs::write(&secret_path, secret);
    secret.to_vec()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Loads .env into process environment variables if present (silently a no-op if
    // absent -- e.g. a fresh public clone with no .env configured yet). This is what
    // lets seed.rs read SEED_* variables instead of having them hardcoded in source.
    // See .env.example for the full list and .gitignore for why .env itself is never
    // committed.
    dotenvy::dotenv().ok();

    tauri::Builder::default()
        .manage(SessionState::default())
        // Phase 5.2: registers the real native file-dialog plugin the frontend needs
        // for attach_lpo's file-path guardrail (route-map-v2.docx Section 1.1). Was
        // present as a JS dependency (package.json) with NO Rust-side registration and
        // no capability permission granted -- the same "written but never wired" shape
        // this project has flagged repeatedly elsewhere (the original Node echo-stub,
        // AssetPicker). Without this `.plugin()` call, the frontend's `open()` import
        // from `@tauri-apps/plugin-dialog` would fail at runtime with a "plugin not
        // registered" error the first time a user tried to attach an LPO file.
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // Phase R.0: open the real SQLite file and run migrations against it, once,
            // at startup. Phase R.2: this connection is now actually queried/written by
            // handle_guarded_ipc and admin_seed_entitlements above, not just opened and
            // left unused.
            use tauri::Manager;
            let app_data_dir = app
                .path()
                .app_data_dir()
                .expect("could not resolve app_data_dir for the SQLite database");
            let db_path = db::resolve_db_path(&app_data_dir);
            let conn = db::init_db(&db_path)
                .unwrap_or_else(|e| panic!("failed to open/migrate SQLite db at {:?}: {}", db_path, e));

            // R.5-FIX: closes WORKER_CARRYOVER_R5-FIX.md's flagged gap ("you will need
            // a seeded test user to log in as -- the real users table has no rows
            // yet"). Idempotent (INSERT OR IGNORE) -- safe on every launch, does not
            // reset or duplicate data on subsequent runs. Dev/demo convenience, not a
            // production provisioning mechanism -- see seed.rs's module doc.
            if let Err(e) = seed::seed_dev_data(&conn) {
                eprintln!("warning: dev-data seeding failed (non-fatal): {}", e);
            }

            app.manage(DbState(Mutex::new(conn)));

            // R.5-FIX: HmacSecret was previously declared but never constructed or
            // managed anywhere -- `login`/`register_session` below now depend on it
            // actually being present in app state.
            let secret_bytes = resolve_or_create_hmac_secret(&app_data_dir);
            app.manage(HmacSecret(secret_bytes));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            greet,
            handle_guarded_ipc,
            login,
            register_session,
            admin_seed_entitlements
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn required_module_key_maps_known_commands_and_only_those() {
        assert_eq!(required_module_key("save_quote"), Some("quotes_core"));
        assert_eq!(required_module_key("fetch_quote"), Some("quotes_core"));
        assert_eq!(
            required_module_key("fetch_inventory_specs"),
            Some("inventory_specs")
        );
        assert_eq!(
            required_module_key("calculate_rate_matrix"),
            Some("rate_matrix")
        );
        assert_eq!(
            required_module_key("generate_quote_pdf"),
            Some("quotes_core")
        );
        // Phase 5.1
        assert_eq!(
            required_module_key("fetch_quote_revisions"),
            Some("quotes_core")
        );
        assert_eq!(
            required_module_key("branch_new_revision"),
            Some("quotes_core")
        );
        assert_eq!(
            required_module_key("update_quote_status"),
            Some("quotes_core")
        );
        assert_eq!(required_module_key("attach_lpo"), Some("quotes_core"));
        // Phase 4.2
        assert_eq!(
            required_module_key("list_compliance_terms"),
            Some("compliance_terms")
        );
        assert_eq!(
            required_module_key("save_compliance_term"),
            Some("compliance_terms")
        );
        assert_eq!(
            required_module_key("deactivate_compliance_term"),
            Some("compliance_terms")
        );
        assert_eq!(
            required_module_key("attach_terms_to_quote"),
            Some("compliance_terms")
        );
        assert_eq!(required_module_key("list_tax_rules"), Some("compliance_terms"));
        assert_eq!(required_module_key("save_tax_rule"), Some("compliance_terms"));
        assert_eq!(
            required_module_key("deactivate_tax_rule"),
            Some("compliance_terms")
        );
        assert_eq!(required_module_key("admin_seed_entitlements"), None);
        assert_eq!(required_module_key("nonexistent_cmd"), None);
    }

    /// Phase R.2: confirms `required_module_key`'s values are all still real entries in
    /// the now-single-sourced `auth::CANONICAL_MODULE_KEYS` -- i.e. that removing the
    /// old duplicated const here didn't silently desync command routing from the list
    /// that now lives only in auth.rs.
    #[test]
    fn required_module_key_values_are_all_canonical() {
        for cmd in [
        "save_quote",
        "fetch_quote",
        "fetch_inventory_specs",
        "calculate_rate_matrix",
        "generate_quote_pdf",
        "fetch_quote_revisions",
        "branch_new_revision",
        "update_quote_status",
        "attach_lpo",
        "list_compliance_terms",
        "save_compliance_term",
        "deactivate_compliance_term",
        "attach_terms_to_quote",
        "list_tax_rules",
        "save_tax_rule",
        "deactivate_tax_rule",
    ] {
            let key = required_module_key(cmd).unwrap();
            assert!(
                auth::CANONICAL_MODULE_KEYS.contains(&key),
                "{} maps to {}, which is not in auth::CANONICAL_MODULE_KEYS",
                cmd,
                key
            );
        }
    }
}