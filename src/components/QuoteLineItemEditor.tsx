// src/components/QuoteLineItemEditor.tsx
// Phase 3.2 Update: Integrated DateRangePicker UI listener
//
// Previously (Phase 3.1): raw rental_duration_days number input
// Now (Phase 3.2): DateRangePicker component that accepts hire start/end dates
// and automatically calculates duration_days to feed into the rate matrix engine.
//
// The calculateRateMatrix() and calculateLineTotal() from src-shared/rate-matrix.ts
// remain the single source of truth for tier logic — see Phase 3.1 for that engine.

import React, { useState } from 'react';
import { AssetPicker, Asset } from './AssetPicker';
import { DateRangePicker, calculateDurationDays } from './DateRangePicker';
import { populateQuoteItemFromAsset } from '../../src-shared/tauri-bridge';
import { calculateRateMatrix, calculateLineTotal as calculateLineTotalFromEngine } from '../../src-shared/rate-matrix';
import styles from './QuoteLineItemEditor.module.css';

export interface QuoteLineItem {
    id: string;
    quote_id: string;
    item_order: number;
    item_description: string;
    make_model?: string;
    quantity: number;
    unit_rate: number;
    rate_basis: 'Daily' | 'Weekly' | 'Monthly';
    rental_duration_days: number;
    line_total: number;
    equipment_spec?: string;
    // Phase 3.2: Track date range for reference/editing
    hire_start_date?: string; // YYYY-MM-DD
    hire_end_date?: string;   // YYYY-MM-DD
}

export interface QuoteLineItemEditorProps {
    tenantId: string;
    quoteId: string;
    itemOrder: number;
    initialItem?: Partial<QuoteLineItem>;
    onSave: (item: QuoteLineItem) => void;
    onCancel: () => void;
}

export const QuoteLineItemEditor: React.FC<QuoteLineItemEditorProps> = ({
    tenantId,
    quoteId,
    itemOrder,
    initialItem,
    onSave,
    onCancel,
}) => {
    const [selectedAsset, setSelectedAsset] = useState<Asset | null>(null);
    const [durationError, setDurationError] = useState<string | null>(null);
    const [formData, setFormData] = useState<Partial<QuoteLineItem>>({
        quote_id: quoteId,
        item_order: itemOrder,
        item_description: initialItem?.item_description || '',
        make_model: initialItem?.make_model,
        quantity: initialItem?.quantity || 1,
        unit_rate: initialItem?.unit_rate || 0,
        rate_basis: initialItem?.rate_basis || 'Monthly',
        rental_duration_days: initialItem?.rental_duration_days || 30,
        equipment_spec: initialItem?.equipment_spec,
        // Phase 3.2: Preserve date range if available
        hire_start_date: initialItem?.hire_start_date,
        hire_end_date: initialItem?.hire_end_date,
    });

    /**
     * Handle asset selection from AssetPicker
     * Automatically populates form fields with asset data
     */
    const handleAssetSelected = (asset: Asset) => {
        setSelectedAsset(asset);

        const populated = populateQuoteItemFromAsset(asset, formData.rate_basis || 'Monthly');

        setFormData((prev) => ({
            ...prev,
            ...populated,
        }));
    };

    /**
     * Phase 3.2: Handle date-range listener callback
     * DateRangePicker emits calculated duration_days; feed into rate matrix engine
     */
    const handleDurationFromDateRange = (durationDays: number) => {
        if (!selectedAsset || durationDays < 1) {
            setDurationError(
                !selectedAsset
                    ? 'Select an asset before setting rental dates.'
                    : 'Rental duration must be at least 1 day.'
            );
            setFormData((prev) => ({
                ...prev,
                rental_duration_days: durationDays,
                rate_basis: undefined,
                unit_rate: 0,
            }));
            return;
        }

        try {
            const matrix = calculateRateMatrix(durationDays, {
                default_daily_rate: selectedAsset.default_daily_rate,
                default_weekly_rate: selectedAsset.default_weekly_rate,
                default_monthly_rate: selectedAsset.default_monthly_rate,
            });

            setDurationError(null);
            setFormData((prev) => ({
                ...prev,
                rental_duration_days: durationDays,
                rate_basis: matrix.rateBasis,
                unit_rate: matrix.unitRate,
            }));
        } catch (err) {
            const message = err instanceof Error ? err.message : 'Invalid rental duration.';
            console.warn('Invalid rental duration:', err);
            setDurationError(message);
            setFormData((prev) => ({
                ...prev,
                rental_duration_days: durationDays,
                rate_basis: undefined,
                unit_rate: 0,
            }));
        }
    };

    /**
     * Calculate line total from quantity and duration
     * Calls the tested calculateLineTotal from src-shared/rate-matrix.ts
     */
    const computeDisplayedLineTotal = (): number => {
        if (!selectedAsset || !formData.quantity || formData.quantity < 1) {
            return 0;
        }
        try {
            return calculateLineTotalFromEngine(
                formData.quantity,
                formData.rental_duration_days || 0,
                {
                    default_daily_rate: selectedAsset.default_daily_rate,
                    default_weekly_rate: selectedAsset.default_weekly_rate,
                    default_monthly_rate: selectedAsset.default_monthly_rate,
                }
            );
        } catch {
            return 0;
        }
    };

    /**
     * Handle form submission
     */
    const handleSubmit = (e: React.FormEvent) => {
        e.preventDefault();

        if (!formData.item_description) {
            alert('Description is required');
            return;
        }

        if ((formData.quantity || 0) <= 0) {
            alert('Quantity must be greater than 0');
            return;
        }

        if ((formData.rental_duration_days || 0) < 1) {
            alert('Rental duration must be at least 1 day');
            return;
        }

        if ((formData.unit_rate || 0) < 0) {
            alert('Unit rate cannot be negative');
            return;
        }

        const lineItem: QuoteLineItem = {
            id: initialItem?.id || `item-${Date.now()}`,
            quote_id: formData.quote_id || quoteId,
            item_order: formData.item_order || itemOrder,
            item_description: formData.item_description || '',
            make_model: formData.make_model,
            quantity: formData.quantity || 1,
            unit_rate: formData.unit_rate || 0,
            rate_basis: formData.rate_basis || 'Monthly',
            rental_duration_days: formData.rental_duration_days || 30,
            line_total: computeDisplayedLineTotal(),
            equipment_spec: formData.equipment_spec,
            hire_start_date: formData.hire_start_date,
            hire_end_date: formData.hire_end_date,
        };

        onSave(lineItem);
    };

    return (
        <div className={styles.editorContainer}>
            <form onSubmit={handleSubmit} className={styles.form}>
                <h3 className={styles.title}>Line Item #{itemOrder}</h3>

                {/* ============================================================
                    ASSET PICKER — Quick-select catalog assets
                    ============================================================ */}
                <div className={styles.section}>
                    <AssetPicker
                        tenantId={tenantId}
                        onAssetSelected={handleAssetSelected}
                        selectedAssetId={selectedAsset?.id}
                        rateBasis={formData.rate_basis as 'Daily' | 'Weekly' | 'Monthly'}
                    />
                </div>

                {/* ============================================================
                    DESCRIPTION FIELD
                    ============================================================ */}
                <div className={styles.formGroup}>
                    <label htmlFor="description">Description *</label>
                    <input
                        id="description"
                        type="text"
                        value={formData.item_description || ''}
                        onChange={(e) =>
                            setFormData((prev) => ({
                                ...prev,
                                item_description: e.target.value,
                            }))
                        }
                        placeholder="Equipment or service description"
                        required
                    />
                </div>

                {/* ============================================================
                    MAKE/MODEL FIELD
                    ============================================================ */}
                <div className={styles.formGroup}>
                    <label htmlFor="makeModel">Make/Model</label>
                    <input
                        id="makeModel"
                        type="text"
                        value={formData.make_model || ''}
                        onChange={(e) =>
                            setFormData((prev) => ({
                                ...prev,
                                make_model: e.target.value,
                            }))
                        }
                        placeholder="e.g., CAT 320D, Volvo EC480E"
                    />
                </div>

                {/* ============================================================
                    EQUIPMENT SPECS DISPLAY (Read-only from asset)
                    ============================================================ */}
                {formData.equipment_spec && (
                    <div className={styles.formGroup}>
                        <label>Specifications</label>
                        <div className={styles.specsDisplay}>
                            {formData.equipment_spec}
                        </div>
                    </div>
                )}

                {/* ============================================================
                    PHASE 3.2: DATE-RANGE LISTENER
                    Replaces raw rental_duration_days input
                    ============================================================ */}
                <DateRangePicker
                    onDurationChange={handleDurationFromDateRange}
                    initialStartDate={formData.hire_start_date}
                    initialEndDate={formData.hire_end_date}
                />

                {/* ============================================================
                    QUANTITY & RATE FIELDS (CALCULATED FROM DURATION)
                    ============================================================ */}
                <div className={styles.formRow}>
                    <div className={styles.formGroup}>
                        <label htmlFor="quantity">Quantity *</label>
                        <input
                            id="quantity"
                            type="number"
                            min="1"
                            value={formData.quantity || 1}
                            onChange={(e) =>
                                setFormData((prev) => ({
                                    ...prev,
                                    quantity: parseInt(e.target.value, 10) || 1,
                                }))
                            }
                            required
                        />
                    </div>

                    <div className={styles.formGroup}>
                        <label htmlFor="rateBasis">Rate Basis (auto-calculated) *</label>
                        <div className={styles.readOnlyField}>
                            {formData.rate_basis || 'Monthly'}
                        </div>
                    </div>

                    <div className={styles.formGroup}>
                        <label htmlFor="unitRate">Unit Rate (AED, auto-calculated) *</label>
                        <div className={styles.readOnlyField}>
                            {(formData.unit_rate || 0).toFixed(2)}
                        </div>
                    </div>

                    <div className={styles.formGroup}>
                        <label>Line Total (AED)</label>
                        <div className={styles.lineTotal}>
                            {computeDisplayedLineTotal().toFixed(2)}
                        </div>
                    </div>
                </div>

                {durationError && <div className={styles.errorText}>{durationError}</div>}

                {/* ============================================================
                    ACTIONS
                    ============================================================ */}
                <div className={styles.actions}>
                    <button type="submit" className={styles.buttonSave}>
                        Save Line Item
                    </button>
                    <button type="button" onClick={onCancel} className={styles.buttonCancel}>
                        Cancel
                    </button>
                </div>
            </form>
        </div>
    );
};

export default QuoteLineItemEditor;
