use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct IpcResponse {
    pub status: u16,
    pub message: String,
    pub data: Option<serde_json::Value>,
}

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You're running borge_equipment_rental.", name)
}

#[tauri::command]
fn handle_guarded_ipc(
    command_name: String,
    payload: Option<serde_json::Value>,
    session_token: Option<String>,
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

    Ok(IpcResponse {
        status: 200,
        message: format!("Command '{}' dispatched successfully", command_name),
        data: payload,
    })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![greet, handle_guarded_ipc])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}