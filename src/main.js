import { isTauri } from '../src-shared/tauri-bridge.js';

document.addEventListener('DOMContentLoaded', () => {
  const statusEl = document.getElementById('status');
  if (isTauri()) {
    statusEl.textContent = 'Runtime detected: Desktop Shell (Tauri v2 native bridge active)';
  } else {
    statusEl.textContent = 'Runtime detected: Web Application target (browser fallback active)';
  }
});
