use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;

mod auth;
mod db;
mod quotes;
mod rate_matrix;

/// Phase R.0: holds the one real SQLite connection the running app uses.
/// Phase R.2: now actually read from — `handle_guarded_ipc` and
/// `admin_seed_entitlements` below query/write the real `tenant_entitlements` table
/// through this connection via `auth::check_tenant_entitlement` /
/// `auth::seed_tenant_entitlement`, replacing the old in-memory `EntitlementState`.
pub struct DbState(pub Mutex<rusqlite::Connection>);

/// HMAC secret used to sign/verify session tokens. Phase R.2 scope note: this phase
/// ports the *capability* to verify tokens in Rust (`auth::verify_session_token`), but
/// does not yet change how a token gets here in the first place -- `register_session`
/// below is unchanged from R.1/R.0 and still trusts a token `server/auth.js` already
/// verified, for the same reason its original comment gives (the shared HMAC secret
/// itself wasn't among this phase's delivered files as a concrete runtime value, only
/// as the algorithm/parameters `auth.js` uses). Wiring `register_session` to actually
/// call `auth::verify_session_token` against a real shared secret, so Rust stops
/// trusting an already-verified claim and independently re-verifies it, is left as an
/// explicit open item for R.5 (full click-through), not silently done here.
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

/// KNOWN LIMITATION — flagged, not silently assumed correct: this trusts a
/// token that was already verified by server/auth.js elsewhere in the
/// stack. See `HmacSecret` doc comment above -- Rust now *can* verify a token
/// independently (`auth::verify_session_token`, built and tested this phase), but this
/// command isn't wired to do so yet; that's an explicit R.5 open item, not an oversight.
#[tauri::command]
fn register_session(
    session_token: String,
    tenant_id: String,
    user_id: String,
    sessions: tauri::State<SessionState>,
) -> Result<IpcResponse, String> {
    if session_token.trim().is_empty() || tenant_id.trim().is_empty() || user_id.trim().is_empty()
    {
        return Ok(IpcResponse {
            status: 400,
            message: "session_token, tenant_id, and user_id are required".into(),
            data: None,
        });
    }
    sessions
        .0
        .lock()
        .unwrap()
        .insert(session_token, (tenant_id, user_id));
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
#[tauri::command]
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
#[tauri::command]
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
                Err(quotes::QuoteError::Db(e)) => Ok(IpcResponse {
                    status: 500,
                    message: format!("Internal Error: {}", e),
                    data: None,
                }),
            }
        }
        _ => Ok(IpcResponse {
            status: 200,
            message: format!("Command '{}' dispatched successfully", command_name),
            data: payload,
        }),
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(SessionState::default())
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
            app.manage(DbState(Mutex::new(conn)));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            greet,
            handle_guarded_ipc,
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
        assert_eq!(required_module_key("admin_seed_entitlements"), None);
        assert_eq!(required_module_key("nonexistent_cmd"), None);
    }

    /// Phase R.2: confirms `required_module_key`'s values are all still real entries in
    /// the now-single-sourced `auth::CANONICAL_MODULE_KEYS` -- i.e. that removing the
    /// old duplicated const here didn't silently desync command routing from the list
    /// that now lives only in auth.rs.
    #[test]
    fn required_module_key_values_are_all_canonical() {
        for cmd in ["save_quote", "fetch_quote", "fetch_inventory_specs", "calculate_rate_matrix"] {
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