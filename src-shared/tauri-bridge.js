function getTauriInvoke() {
  if (typeof window !== 'undefined' && window.__TAURI__) {
    if (window.__TAURI__.core && typeof window.__TAURI__.core.invoke === 'function') {
      return window.__TAURI__.core.invoke;
    }
    if (typeof window.__TAURI__.invoke === 'function') {
      return window.__TAURI__.invoke;
    }
  }
  return null;
}

export async function invokeCommand(cmd, args = {}) {
  const tauriInvoke = getTauriInvoke();
  if (tauriInvoke) {
    try {
      return await tauriInvoke(cmd, args);
    } catch (err) {
      console.error(`Tauri IPC error executing command ${cmd}:`, err);
      throw err;
    }
  }

  console.warn(`[Tauri Bridge] Running in non-desktop mode. Mocking or routing command: ${cmd}`);
  if (typeof window !== 'undefined' && window.__MOCK_BACKEND_IPC__) {
    return window.__MOCK_BACKEND_IPC__(cmd, args);
  }
  
  throw new Error(`Desktop IPC unavailable for command '${cmd}' and no browser mock active.`);
}

export async function executeGuardedCommand(commandName, payload = null, sessionToken = "") {
  return await invokeCommand('handle_guarded_ipc', {
    commandName: commandName || null,
    payload: payload !== undefined ? payload : null,
    sessionToken: sessionToken || null
  });
}