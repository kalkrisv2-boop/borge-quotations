// src-shared/tauri-bridge.d.ts
// Type declarations for the canonical Tauri IPC bridge
// Resolves TS7016 errors when importing from tauri-bridge.js

/**
 * Asset data structure from backend/IPC
 */
export interface Asset {
    id: string;
    tenant_id: string;
    category_id: string;
    asset_code: string;
    asset_name: string;
    description?: string;
    make_model?: string;
    serial_number?: string;
    location?: string;
    status: string;
    purchase_date?: string;
    warranty_expiry?: string;
    default_daily_rate: number;
    default_weekly_rate: number;
    default_monthly_rate: number;
    equipment_specs: EquipmentSpec[];
}

/**
 * Equipment specification row
 */
export interface EquipmentSpec {
    id: string;
    asset_id: string;
    spec_key: string;
    spec_value: string;
    unit_of_measure?: string;
}

/**
 * Asset search request payload
 */
export interface AssetSearchRequest {
    tenant_id: string;
    search_type: AssetSearchType;
}

/**
 * Asset search type discriminator
 */
export type AssetSearchType =
    | { type: 'ByCategory'; value: string }
    | { type: 'ByCode'; value: string }
    | { type: 'ByName'; value: string }
    | { type: 'ByStatus'; value: string };

/**
 * Quote item population result from asset selection
 */
export interface PopulatedQuoteItem {
    item_description: string;
    make_model?: string;
    equipment_spec?: string;
    unit_rate: number;
}

/**
 * Check if running in desktop (Tauri) mode
 */
export declare function isDesktopMode(): boolean;

/**
 * Invoke a Tauri command with error handling
 * @param command - Tauri command name
 * @param payload - Command payload
 * @returns Promise resolving to command result
 * @throws Error if not in desktop mode or if IPC call fails
 */
export declare function tauriInvoke<T = any>(command: string, payload?: any): Promise<T>;

/**
 * Asset lookup IPC call wrapper (desktop mode)
 * Maps to either asset_lookup_sqlite (local) or asset_lookup_postgres (cloud)
 *
 * @param command - 'asset_lookup_sqlite' or 'asset_lookup_postgres'
 * @param request - Asset search request
 * @returns Promise<Asset[]> - Array of matching assets
 */
export declare function tauriIpcCall(
    command: 'asset_lookup_sqlite' | 'asset_lookup_postgres',
    request: AssetSearchRequest
): Promise<Asset[]>;

/**
 * Populate quote line-item fields from selected asset
 * Auto-selects correct default rate based on rate_basis
 *
 * @param asset - Selected asset
 * @param rateBasis - 'Daily', 'Weekly', or 'Monthly' (default: 'Monthly')
 * @returns Object with populated fields for quote_item form
 */
export declare function populateQuoteItemFromAsset(
    asset: Asset,
    rateBasis?: 'Daily' | 'Weekly' | 'Monthly'
): PopulatedQuoteItem;

/**
 * Register listener for Tauri drag-drop events
 * @param callback - Function called with dropped file paths
 * @returns Unsubscribe function
 */
export declare function onTauriDragDrop(callback: (paths: string[]) => void): () => void;

/**
 * Format asset for display in quote line-items
 */
export declare function formatAssetForDisplay(asset: Asset): string;

/**
 * Log IPC diagnostics (for debugging)
 */
export declare function logTauriDiagnostics(): void;
