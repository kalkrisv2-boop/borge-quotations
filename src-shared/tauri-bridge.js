/**
 * Defensive Bridge Helper for Tauri v2 / Web Target Dual Compatibility
 * Canonical accessor — all future IPC/dialog/fs calls should import from here
 * per PROJECT_BASELINE.md Section 1.2a. Do not touch window.__TAURI__ directly
 * in feature code.
 */
const getNested = (obj, path) => path.split('.').reduce((acc, part) => acc && acc[part], obj);

export const isTauri = () => {
  return typeof window !== 'undefined' && Boolean(window.__TAURI__);
};

export const invoke = async (cmd, args = {}) => {
  if (isTauri()) {
    const tauriInvoke =
      getNested(window, '__TAURI__.core.invoke') ||
      getNested(window, '__TAURI__.invoke');
    if (typeof tauriInvoke === 'function') {
      return tauriInvoke(cmd, args);
    }
  }
  console.warn(`[Tauri Bridge] Web fallback triggered for IPC command: "${cmd}"`, args);
  return null;
};

export const openDialog = async (options = {}) => {
  if (isTauri()) {
    const dialogOpen =
      getNested(window, '__TAURI__.plugin.dialog.open') ||
      getNested(window, '__TAURI__.dialog.open');
    if (typeof dialogOpen === 'function') {
      return dialogOpen(options);
    }
  }
  console.warn('[Tauri Bridge] Dialog API unavailable in web mode or bridge not initialized.');
  return null;
};

export const readFile = async (filePath, options = {}) => {
  if (isTauri()) {
    const fsReadFile =
      getNested(window, '__TAURI__.plugin.fs.readFile') ||
      getNested(window, '__TAURI__.fs.readFile');
    if (typeof fsReadFile === 'function') {
      return fsReadFile(filePath, options);
    }
  }
  console.warn('[Tauri Bridge] FS Read File API unavailable in web target.');
  return null;
};
