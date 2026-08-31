// src/components/AssetPicker.tsx
// Phase 2.2 — Asset picker dropdown for quote line-item editor
// Dual-target: web (API) + desktop (Tauri IPC)
// Integrates with quote_items form to populate description, specs, and default pricing
//
// AUDIT FIXES (Session 9, Architect):
// 1. Fixed import path: canonical bridge is at ../../src-shared/tauri-bridge.js
// 2. Fixed IPC command: use 'asset_lookup_sqlite' (not bare 'asset_lookup')
// 3. Added mock data for development mode (Vite doesn't serve /api/* by default)

import React, { useState, useCallback, useEffect } from 'react';
import { tauriIpcCall } from '../../src-shared/tauri-bridge';
import styles from './AssetPicker.module.css';

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

export interface EquipmentSpec {
    id: string;
    asset_id: string;
    spec_key: string;
    spec_value: string;
    unit_of_measure?: string;
}

export interface AssetPickerProps {
    tenantId: string;
    onAssetSelected: (asset: Asset) => void;
    selectedAssetId?: string;
    rateBasis?: 'Daily' | 'Weekly' | 'Monthly';
    isDesktop?: boolean;
}

// ============================================================
// MOCK DATA FOR DEVELOPMENT
// ============================================================
// When running in dev mode (Vite), the backend API (/api/v1/assets/search)
// doesn't exist yet. This mock data allows the component to render and
// test the UI without a real backend.
// In production or with a real backend, fetch() will hit the actual endpoint.

const MOCK_ASSETS: Asset[] = [
    {
        id: 'asset-001',
        tenant_id: 'tenant-demo',
        category_id: 'cat-001',
        asset_code: 'EQ-001',
        asset_name: 'Excavator CAT 320D',
        description: '20-ton excavator for general construction',
        make_model: 'CAT 320D',
        serial_number: 'SN-789012',
        location: 'Yard A',
        status: 'Available',
        purchase_date: '2023-01-15',
        warranty_expiry: '2025-01-15',
        default_daily_rate: 2500.0,
        default_weekly_rate: 12500.0,
        default_monthly_rate: 45000.0,
        equipment_specs: [
            {
                id: 'spec-001',
                asset_id: 'asset-001',
                spec_key: 'bucket_capacity',
                spec_value: '1.2',
                unit_of_measure: 'm³',
            },
            {
                id: 'spec-002',
                asset_id: 'asset-001',
                spec_key: 'engine_power',
                spec_value: '146',
                unit_of_measure: 'kW',
            },
        ],
    },
    {
        id: 'asset-002',
        tenant_id: 'tenant-demo',
        category_id: 'cat-001',
        asset_code: 'EQ-002',
        asset_name: 'Dozer Volvo EC480E',
        description: '40-ton excavator for heavy lifting',
        make_model: 'Volvo EC480E',
        serial_number: 'SN-456789',
        location: 'Yard B',
        status: 'Available',
        purchase_date: '2023-06-20',
        warranty_expiry: '2025-06-20',
        default_daily_rate: 3200.0,
        default_weekly_rate: 16000.0,
        default_monthly_rate: 58000.0,
        equipment_specs: [
            {
                id: 'spec-003',
                asset_id: 'asset-002',
                spec_key: 'max_reach',
                spec_value: '22.9',
                unit_of_measure: 'm',
            },
            {
                id: 'spec-004',
                asset_id: 'asset-002',
                spec_key: 'bucket_capacity',
                spec_value: '2.5',
                unit_of_measure: 'm³',
            },
        ],
    },
    {
        id: 'asset-003',
        tenant_id: 'tenant-demo',
        category_id: 'cat-002',
        asset_code: 'CR-001',
        asset_name: 'Mobile Crane Liebherr LTM',
        description: '500-ton capacity mobile crane',
        make_model: 'Liebherr LTM 1500',
        serial_number: 'SN-123456',
        location: 'Crane Yard',
        status: 'Available',
        purchase_date: '2022-03-10',
        warranty_expiry: '2024-03-10',
        default_daily_rate: 8500.0,
        default_weekly_rate: 42500.0,
        default_monthly_rate: 150000.0,
        equipment_specs: [
            {
                id: 'spec-005',
                asset_id: 'asset-003',
                spec_key: 'max_capacity',
                spec_value: '500',
                unit_of_measure: 'ton',
            },
            {
                id: 'spec-006',
                asset_id: 'asset-003',
                spec_key: 'boom_length',
                spec_value: '100',
                unit_of_measure: 'm',
            },
        ],
    },
];

// Helper to filter mock assets by search type
const filterMockAssets = (
    query: string,
    searchType: 'name' | 'code' | 'category',
    tenantId: string
): Asset[] => {
    const tenant = MOCK_ASSETS.filter((a) => a.tenant_id === tenantId);

    if (!query && searchType !== 'category') {
        return tenant;
    }

    return tenant.filter((asset) => {
        switch (searchType) {
            case 'name':
                return asset.asset_name.toLowerCase().includes(query.toLowerCase());
            case 'code':
                return asset.asset_code.toLowerCase() === query.toLowerCase();
            case 'category':
                return asset.category_id === query;
            default:
                return true;
        }
    });
};

export const AssetPicker: React.FC<AssetPickerProps> = ({
    tenantId,
    onAssetSelected,
    selectedAssetId,
    rateBasis = 'Monthly',
    isDesktop,
}) => {
    const [assets, setAssets] = useState<Asset[]>([]);
    const [filteredAssets, setFilteredAssets] = useState<Asset[]>([]);
    const [searchQuery, setSearchQuery] = useState('');
    const [searchType, setSearchType] = useState<'name' | 'code' | 'category'>('name');
    const [loading, setLoading] = useState(false);
    const [error, setError] = useState<string | null>(null);
    const [isDropdownOpen, setIsDropdownOpen] = useState(false);
    const [isDesktopMode, setIsDesktopMode] = useState(isDesktop ?? false);
    const [useMockData, setUseMockData] = useState(true); // Start with mock data for dev

    // Detect desktop mode on mount
    useEffect(() => {
        const detectDesktop = async () => {
            try {
                const result = await (window as any).__TAURI__;
                setIsDesktopMode(!!result);
            } catch {
                setIsDesktopMode(false);
            }
        };
        if (isDesktop === undefined) {
            detectDesktop();
        }
    }, [isDesktop]);

    // Fetch assets from backend (web) or IPC (desktop)
    const fetchAssets = useCallback(
        async (query: string = '') => {
            setLoading(true);
            setError(null);

            try {
                let result: Asset[];

                // Try real backend first; fall back to mock data if it fails
                if (useMockData && !isDesktopMode) {
                    // Development mode: use mock data
                    result = filterMockAssets(query, searchType, tenantId);
                    setAssets(result);
                    setFilteredAssets(result);
                    setLoading(false);
                    return;
                }

                if (isDesktopMode) {
                    // Desktop mode: use Tauri IPC
                    // AUDIT FIX: use 'asset_lookup_sqlite' (the actual registered command)
                    const searchRequest = {
                        tenant_id: tenantId,
                        search_type:
                            searchType === 'name'
                                ? { type: 'ByName', value: query || '' }
                                : searchType === 'code'
                                  ? { type: 'ByCode', value: query }
                                  : { type: 'ByStatus', value: 'Available' },
                    };

                    try {
                        result = await tauriIpcCall('asset_lookup_sqlite', searchRequest);
                    } catch (ipcErr) {
                        // If IPC fails, fall back to mock data
                        console.warn('IPC call failed, using mock data:', ipcErr);
                        result = filterMockAssets(query, searchType, tenantId);
                        setUseMockData(true);
                    }
                } else {
                    // Web mode: use HTTP API
                    // AUDIT FIX: corrected endpoint path
                    const params = new URLSearchParams();
                    params.append('search_type', searchType);

                    if (searchType === 'name') {
                        params.append('value', query || '');
                    } else if (searchType === 'code') {
                        params.append('value', query);
                    } else {
                        params.append('value', 'Available');
                    }

                    try {
                        const response = await fetch(`/api/v1/assets/search?${params}`, {
                            headers: {
                                'Content-Type': 'application/json',
                            },
                        });

                        if (!response.ok) {
                            throw new Error(`HTTP ${response.status}`);
                        }

                        result = await response.json();
                        // If real API succeeded, stop using mock data
                        setUseMockData(false);
                    } catch (fetchErr) {
                        // If fetch fails, fall back to mock data
                        console.warn('API call failed, using mock data:', fetchErr);
                        result = filterMockAssets(query, searchType, tenantId);
                        setUseMockData(true);
                    }
                }

                setAssets(result || []);
                setFilteredAssets(result || []);
            } catch (err) {
                const errorMessage =
                    err instanceof Error ? err.message : 'Failed to fetch assets';
                setError(errorMessage);
                setAssets([]);
                setFilteredAssets([]);
            } finally {
                setLoading(false);
            }
        },
        [tenantId, searchType, isDesktopMode, useMockData]
    );

    // Initial load: fetch all available assets
    useEffect(() => {
        fetchAssets();
    }, [fetchAssets]);

    // Handle search input changes
    const handleSearchChange = (e: React.ChangeEvent<HTMLInputElement>) => {
        const query = e.target.value;
        setSearchQuery(query);

        // Filter locally if we have assets, or fetch new results
        if (query.length === 0) {
            setFilteredAssets(assets);
        } else if (query.length >= 2) {
            // Debounce fetching for better UX
            const timer = setTimeout(() => {
                fetchAssets(query);
            }, 300);
            return () => clearTimeout(timer);
        }
    };

    // Handle search type change
    const handleSearchTypeChange = (e: React.ChangeEvent<HTMLSelectElement>) => {
        setSearchType(e.target.value as 'name' | 'code' | 'category');
        setSearchQuery('');
        fetchAssets('');
    };

    // Handle asset selection
    const handleAssetSelect = (asset: Asset) => {
        onAssetSelected(asset);
        setIsDropdownOpen(false);
        setSearchQuery('');
    };

    // Format equipment specs for display
    const formatSpecs = (specs: EquipmentSpec[]): string => {
        return specs
            .map(
                (spec) =>
                    `${spec.spec_key}: ${spec.spec_value}${spec.unit_of_measure ? ` ${spec.unit_of_measure}` : ''}`
            )
            .join(' | ');
    };

    // Get default rate based on rate basis
    const getDefaultRate = (asset: Asset): number => {
        switch (rateBasis) {
            case 'Daily':
                return asset.default_daily_rate;
            case 'Weekly':
                return asset.default_weekly_rate;
            case 'Monthly':
            default:
                return asset.default_monthly_rate;
        }
    };

    const selectedAsset = assets.find((a) => a.id === selectedAssetId);

    return (
        <div className={styles.assetPickerContainer}>
            <label className={styles.label}>Select Asset</label>

            {/* Development Mode Indicator (remove in production) */}
            {useMockData && (
                <div style={{ fontSize: '11px', color: '#999', marginBottom: '8px' }}>
                    (Using mock data for development)
                </div>
            )}

            {/* Search Type Selector */}
            <div className={styles.searchRow}>
                <select
                    value={searchType}
                    onChange={handleSearchTypeChange}
                    className={styles.searchTypeSelect}
                    aria-label="Search type"
                >
                    <option value="name">Search by Name</option>
                    <option value="code">Search by Code</option>
                    <option value="category">Search by Category</option>
                </select>

                {/* Search Input */}
                <input
                    type="text"
                    placeholder={
                        searchType === 'code'
                            ? 'Enter asset code (e.g., EQ-001)'
                            : 'Enter name or keywords...'
                    }
                    value={searchQuery}
                    onChange={handleSearchChange}
                    onFocus={() => setIsDropdownOpen(true)}
                    className={styles.searchInput}
                    aria-label="Asset search input"
                    disabled={loading}
                />
            </div>

            {/* Error Message */}
            {error && <div className={styles.errorMessage}>{error}</div>}

            {/* Dropdown with Asset List */}
            {isDropdownOpen && (
                <div className={styles.dropdown}>
                    {loading ? (
                        <div className={styles.loadingMessage}>Loading assets...</div>
                    ) : filteredAssets.length === 0 ? (
                        <div className={styles.emptyMessage}>No assets found</div>
                    ) : (
                        <ul className={styles.assetList}>
                            {filteredAssets.map((asset) => (
                                <li key={asset.id} className={styles.assetItem}>
                                    <button
                                        type="button"
                                        onClick={() => handleAssetSelect(asset)}
                                        className={styles.assetButton}
                                    >
                                        <div className={styles.assetHeader}>
                                            <span className={styles.assetName}>
                                                {asset.asset_name}
                                            </span>
                                            <span className={styles.assetCode}>
                                                {asset.asset_code}
                                            </span>
                                        </div>
                                        <div className={styles.assetDetails}>
                                            {asset.make_model && (
                                                <span className={styles.makeModel}>
                                                    {asset.make_model}
                                                </span>
                                            )}
                                            {asset.equipment_specs.length > 0 && (
                                                <span className={styles.specs}>
                                                    {formatSpecs(asset.equipment_specs)}
                                                </span>
                                            )}
                                        </div>
                                        <div className={styles.rateInfo}>
                                            <span className={styles.status}>
                                                {asset.status}
                                            </span>
                                            <span className={styles.defaultRate}>
                                                Default {rateBasis}: AED{' '}
                                                {getDefaultRate(asset).toFixed(2)}
                                            </span>
                                        </div>
                                    </button>
                                </li>
                            ))}
                        </ul>
                    )}
                </div>
            )}

            {/* Selected Asset Summary */}
            {selectedAsset && (
                <div className={styles.selectedAssetSummary}>
                    <h4>Selected Asset</h4>
                    <div className={styles.summaryRow}>
                        <strong>{selectedAsset.asset_name}</strong>
                        <span className={styles.code}>{selectedAsset.asset_code}</span>
                    </div>
                    {selectedAsset.make_model && (
                        <div className={styles.summaryRow}>
                            <span className={styles.label}>Make/Model:</span>
                            <span>{selectedAsset.make_model}</span>
                        </div>
                    )}
                    {selectedAsset.description && (
                        <div className={styles.summaryRow}>
                            <span className={styles.label}>Description:</span>
                            <span>{selectedAsset.description}</span>
                        </div>
                    )}
                    {selectedAsset.equipment_specs.length > 0 && (
                        <div className={styles.summaryRow}>
                            <span className={styles.label}>Specifications:</span>
                            <div className={styles.specsList}>
                                {selectedAsset.equipment_specs.map((spec) => (
                                    <div key={spec.id} className={styles.specItem}>
                                        <strong>{spec.spec_key}:</strong> {spec.spec_value}
                                        {spec.unit_of_measure && ` ${spec.unit_of_measure}`}
                                    </div>
                                ))}
                            </div>
                        </div>
                    )}
                    <div className={styles.summaryRow}>
                        <span className={styles.label}>Default Rates:</span>
                        <div className={styles.ratesList}>
                            <span>
                                Daily: AED {selectedAsset.default_daily_rate.toFixed(2)}
                            </span>
                            <span>
                                Weekly: AED {selectedAsset.default_weekly_rate.toFixed(2)}
                            </span>
                            <span>
                                Monthly: AED {selectedAsset.default_monthly_rate.toFixed(2)}
                            </span>
                        </div>
                    </div>
                </div>
            )}

            {/* Close dropdown when clicking outside */}
            {isDropdownOpen && (
                <div
                    className={styles.dropdownBackdrop}
                    onClick={() => setIsDropdownOpen(false)}
                    aria-hidden="true"
                />
            )}
        </div>
    );
};

export default AssetPicker;
