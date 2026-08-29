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

// ADDED (Architect audit, Phase 1.2): the desktop Rust IPC boundary now
// maintains its own trusted session registry, separate from the
// server/auth.js verification used on the web target — see
// src-tauri/src/lib.rs's register_session and PROJECT_BASELINE.md Handover
// Log, Session 7. Whatever login flow calls server/auth.js's
// verifySessionToken() successfully (web target) or otherwise establishes
// a verified session must, ON THE DESKTOP BUILD ONLY, also call this once
// so handle_guarded_ipc has a tenant_id to check entitlements against.
// Calling this with an unverified token is a no-op from a security
// standpoint client-side, but is meaningless/harmful if wired to a bogus
// tenant_id — callers MUST only invoke this after real verification.
export async function registerDesktopSession(sessionToken, tenantId) {
  return await invokeCommand('register_session', {
    sessionToken,
    tenantId
  });
}

// ADDED (Architect audit, Phase 1.2): desktop-side counterpart to
// server/ipc_handlers.js's admin_seed_entitlements, now with the auth
// check that command was originally missing. Requires an already
// registered (see registerDesktopSession) admin session.
export async function seedEntitlementsDesktop(sessionToken, targetTenantId, entitlements) {
  return await invokeCommand('admin_seed_entitlements', {
    sessionToken,
    targetTenantId,
    entitlements
  });
}