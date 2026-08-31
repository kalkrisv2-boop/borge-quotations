// src-shared/tauri-bridge.js
// Canonical IPC bridge for browser-to-Tauri communication
// Single entry point for all desktop IPC calls
// Dual-target aware: gracefully falls back to web API if running in browser mode
//
// AUDIT NOTE (Phase 2.2 Architect audit, Session 9): the delivered version of
// this file used TypeScript-only syntax (return-type annotations like
// `(): boolean =>`, and a generic function `async <T = any>(...)`) while
// shipping with a plain `.js` extension. Confirmed by running it directly
// through Node — it throws `SyntaxError: Unexpected token ')'` on load. Per
// PROJECT_BASELINE.md Section 1.3, this file is "the single canonical
// interface for browser-to-native IPC calls," used by every component that
// needs desktop IPC, not just this phase's AssetPicker — so this wasn't a
// narrow defect, it would have broken the entire bridge for any consumer
// that loads it as plain JS (no TS/JSX loader in the path). Rewritten below
// as valid ES module JavaScript with the same public API and behavior,
// using JSDoc comments in place of TypeScript type annotations.

/**
 * Resolve Tauri API entry point defensively across known valid locations
 * Handles multiple API surface paths and versions
 */
const getTauriInstance = () => {
    // Try global binding first (Tauri v2 with withGlobalTauri: true)
    if (typeof window !== 'undefined' && window.__TAURI__) {
        return window.__TAURI__;
    }

    // Fallback: try imported API (if using ES module imports)
    try {
        // In a real app, this would be: import { invoke } from '@tauri-apps/api/core'
        // For now, we return a placeholder that will fail gracefully
        return null;
    } catch {
        return null;
    }
};

/**
 * Check if running in desktop (Tauri) mode
 * @returns {boolean}
 */
export const isDesktopMode = () => {
    return getTauriInstance() !== null && getTauriInstance() !== undefined;
};

/**
 * Invoke a Tauri command with error handling
 * @param {string} command - Tauri command name (e.g., 'asset_lookup_sqlite', 'asset_lookup_postgres')
 * @param {any} [payload] - Command payload (serialized to JSON automatically)
 * @returns {Promise<any>} Promise resolving to command result
 * @throws {Error} if not in desktop mode or if IPC call fails
 */
export const tauriInvoke = async (command, payload) => {
    const tauri = getTauriInstance();

    if (!tauri) {
        throw new Error(
            'Tauri API not available. This function is only available in desktop mode.'
        );
    }

    // Try to access the invoke API through various known paths
    const invokeApi =
        tauri.core?.invoke || // Tauri v2 with core namespace
        tauri.invoke; // Tauri v1 compatibility

    if (!invokeApi || typeof invokeApi !== 'function') {
        throw new Error('Tauri invoke API not found at any known location');
    }

    try {
        return await invokeApi(command, payload);
    } catch (err) {
        const errorMessage =
            err instanceof Error ? err.message : String(err);
        throw new Error(`IPC command '${command}' failed: ${errorMessage}`);
    }
};

/**
 * Wrapper for asset lookup IPC calls (desktop mode)
 * Maps to either asset_lookup_sqlite (local) or asset_lookup_postgres (cloud)
 *
 * @param {'asset_lookup_sqlite'|'asset_lookup_postgres'} command
 * @param {{tenant_id: string, search_type: {type: 'ByCategory'|'ByCode'|'ByName'|'ByStatus', value: string}}} request
 * @returns {Promise<any[]>} Array of matching assets with embedded equipment_specs
 */
export const tauriIpcCall = async (command, request) => {
    if (!isDesktopMode()) {
        throw new Error('Asset lookup IPC only available in desktop mode');
    }

    try {
        const result = await tauriInvoke(command, request);
        return Array.isArray(result) ? result : [];
    } catch (err) {
        console.error(`Asset lookup IPC failed (${command}):`, err);
        throw err;
    }
};

/**
 * Wrapper for quote line-item field population
 * Called when an asset is selected in the AssetPicker
 * Populates description, make_model, equipment_spec, and unit_rate (based on rate_basis)
 *
 * @param {any} asset
 * @param {'Daily'|'Weekly'|'Monthly'} [rateBasis]
 * @returns {{item_description: string, make_model?: string, equipment_spec?: string, unit_rate: number}}
 */
export const populateQuoteItemFromAsset = (asset, rateBasis = 'Monthly') => {
    const specsText = asset.equipment_specs
        ?.map(
            (spec) =>
                `${spec.spec_key}: ${spec.spec_value}${spec.unit_of_measure ? ` ${spec.unit_of_measure}` : ''}`
        )
        .join(' | ');

    // Select unit_rate based on rate_basis
    let unitRate = 0;
    switch (rateBasis) {
        case 'Daily':
            unitRate = asset.default_daily_rate ?? 0;
            break;
        case 'Weekly':
            unitRate = asset.default_weekly_rate ?? 0;
            break;
        case 'Monthly':
        default:
            unitRate = asset.default_monthly_rate ?? 0;
            break;
    }

    return {
        item_description: asset.asset_name,
        make_model: asset.make_model,
        equipment_spec: specsText,
        unit_rate: unitRate,
    };
};

/**
 * Event listener for Tauri drag-drop events (desktop only)
 * Registers a listener for tauri://drag-drop events
 * Use for file uploads, asset batch operations, etc.
 *
 * @param {(paths: string[]) => void} callback - Function called with dropped file paths
 * @returns {() => void} Unsubscribe function
 */
export const onTauriDragDrop = (callback) => {
    const tauri = getTauriInstance();

    if (!tauri) {
        console.warn('Tauri drag-drop listener only works in desktop mode');
        return () => {};
    }

    try {
        // Register for tauri://drag-drop event
        const unlisten = (tauri.event?.listen || tauri.listen)?.(
            'tauri://drag-drop',
            (event) => {
                if (event?.payload?.paths) {
                    callback(event.payload.paths);
                }
            }
        );

        // Return unsubscribe function
        return typeof unlisten === 'function' ? unlisten : () => {};
    } catch (err) {
        console.error('Failed to register drag-drop listener:', err);
        return () => {};
    }
};

/**
 * Utility: format asset for display in quote line-items
 * @param {any} asset
 * @returns {string}
 */
export const formatAssetForDisplay = (asset) => {
    const parts = [asset.asset_name];

    if (asset.asset_code) {
        parts.push(`(${asset.asset_code})`);
    }

    if (asset.make_model) {
        parts.push(`— ${asset.make_model}`);
    }

    return parts.join(' ');
};

/**
 * Utility: log IPC diagnostics (for debugging)
 */
export const logTauriDiagnostics = () => {
    console.log('=== Tauri Bridge Diagnostics ===');
    console.log('Desktop mode:', isDesktopMode());

    const tauri = getTauriInstance();
    if (tauri) {
        console.log('Tauri instance found:', {
            core: !!tauri.core,
            invoke: !!tauri.invoke,
            event: !!tauri.event,
            plugin: !!tauri.plugin,
        });
    } else {
        console.log('Running in web mode (no Tauri instance)');
    }
};

export default {
    isDesktopMode,
    tauriInvoke,
    tauriIpcCall,
    populateQuoteItemFromAsset,
    onTauriDragDrop,
    formatAssetForDisplay,
    logTauriDiagnostics,
};
