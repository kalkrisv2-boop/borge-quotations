use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;

mod db;

/// Phase R.0: holds the one real SQLite connection the running app uses. Not consumed
/// by any command yet this phase — R.1-R.3 are what will actually query through this.
/// Wrapped the same way EntitlementState/SessionState already are (Mutex<T> managed via
/// tauri::State) for consistency with the existing pattern in this file.
pub struct DbState(pub Mutex<rusqlite::Connection>);

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct IpcResponse {
    pub status: u16,
    pub message: String,
    pub data: Option<serde_json::Value>,
}

/// Canonical entitlement module keys. MUST mirror
/// server/entitlements.js::CANONICAL_MODULE_KEYS and
/// PROJECT_BASELINE.md Section 1.3 exactly. Do not invent new ones here.
const CANONICAL_MODULE_KEYS: [&str; 5] = [
    "quotes_core",
    "inventory_specs",
    "rate_matrix",
    "compliance_terms",
    "revision_lpo",
];

/// Maps an IPC command name to the module_key required to invoke it.
/// MUST mirror the switch statement in server/ipc_handlers.js.
fn required_module_key(command_name: &str) -> Option<&'static str> {
    match command_name {
        "save_quote" | "fetch_quote" => Some("quotes_core"),
        "fetch_inventory_specs" => Some("inventory_specs"),
        "calculate_rate_matrix" => Some("rate_matrix"),
        _ => None,
    }
}

/// tenant_id -> module_key -> is_enabled
#[derive(Default)]
pub struct EntitlementState(pub Mutex<HashMap<String, HashMap<String, bool>>>);

/// session_token -> tenant_id. Populated ONLY by register_session, which is
/// called by the frontend right after server/auth.js's verifySessionToken
/// has already confirmed the token server-side. Rust trusts this map
/// because entries are only ever inserted post-verification — it does not
/// re-derive trust from an unverified client claim.
#[derive(Default)]
pub struct SessionState(pub Mutex<HashMap<String, String>>);

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
/// stack. server/auth.js (its HMAC secret and exact token wire-format) was
/// not among this phase's delivered files, so Rust cannot yet independently
/// re-verify the token's signature offline. For a fully offline desktop
/// build (no reachable Node backend) this needs a Rust-side HMAC check
/// against the same secret as auth.js — tracked as an open item for the
/// Phase R.2 Worker brief. Until then, this registration step is the
/// enforcement boundary and must only ever be called with a token the
/// frontend has already had verified.
#[tauri::command]
fn register_session(
    session_token: String,
    tenant_id: String,
    sessions: tauri::State<SessionState>,
) -> Result<IpcResponse, String> {
    if session_token.trim().is_empty() || tenant_id.trim().is_empty() {
        return Ok(IpcResponse {
            status: 400,
            message: "session_token and tenant_id are required".into(),
            data: None,
        });
    }
    sessions.0.lock().unwrap().insert(session_token, tenant_id);
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
#[tauri::command]
fn admin_seed_entitlements(
    session_token: String,
    target_tenant_id: String,
    entitlements: Vec<EntitlementSeedItem>,
    sessions: tauri::State<SessionState>,
    store: tauri::State<EntitlementState>,
) -> Result<IpcResponse, String> {
    let is_known = sessions.0.lock().unwrap().contains_key(&session_token);
    if !is_known {
        return Ok(IpcResponse {
            status: 401,
            message: "Unauthorized: session not registered".into(),
            data: None,
        });
    }

    let mut db = store.0.lock().unwrap();
    let tenant_map = db.entry(target_tenant_id).or_insert_with(HashMap::new);
    for item in entitlements {
        if CANONICAL_MODULE_KEYS.contains(&item.module_key.as_str()) {
            tenant_map.insert(item.module_key, item.is_enabled);
        }
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
/// enforcement at all. This is the exact failure the route-map's Phase 1.2
/// Completion Check calls out: "API routes AND IPC commands both block
/// unlicensed calls." The API-route side (server/ipc_handlers.js via
/// guardApiRoute) was already correct; the IPC side was not. Now looks the
/// token up in the registered-session map to resolve a trusted tenant_id,
/// then checks that tenant's entitlement for the command's required
/// module_key before dispatching.
#[tauri::command]
async fn handle_guarded_ipc(
    command_name: String,
    payload: Option<serde_json::Value>,
    session_token: Option<String>,
    sessions: tauri::State<'_, SessionState>,
    store: tauri::State<'_, EntitlementState>,
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

    let tenant_id = {
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
            let db = store.0.lock().unwrap();
            db.get(&tenant_id)
                .and_then(|m| m.get(required_key))
                .copied()
                .unwrap_or(false)
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

    Ok(IpcResponse {
        status: 200,
        message: format!("Command '{}' dispatched successfully", command_name),
        data: payload,
    })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(EntitlementState::default())
        .manage(SessionState::default())
        .setup(|app| {
            // Phase R.0: open the real SQLite file and run migrations against it, once,
            // at startup — replacing the in-memory Map that server/ipc_handlers.js used.
            // No command reads from this connection yet (that starts in R.3); this
            // setup step only proves the app itself can open and migrate a real,
            // persistent database file on the actual target platform.
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

    #[test]
    fn canonical_keys_are_exactly_five_and_match_baseline() {
        assert_eq!(CANONICAL_MODULE_KEYS.len(), 5);
        assert!(CANONICAL_MODULE_KEYS.contains(&"quotes_core"));
        assert!(CANONICAL_MODULE_KEYS.contains(&"inventory_specs"));
        assert!(CANONICAL_MODULE_KEYS.contains(&"rate_matrix"));
        assert!(CANONICAL_MODULE_KEYS.contains(&"compliance_terms"));
        assert!(CANONICAL_MODULE_KEYS.contains(&"revision_lpo"));
    }
}