// src/components/QuoteLineItemEditor.tsx
// Phase 3.1 Update: Integrated tiered rental rate matrix engine
//
// Added:
//   - rental_duration_days field for calculating applicable rate tier
//   - Automatic rate_basis determination based on duration (Daily/Weekly/Monthly)
//   - Automatic unit_rate selection from asset's default rates
//   - End-to-end integration with rate-matrix engine
//
// Previous notes (still applicable):
// Promoted (Session 10) from QuoteLineItemEditor.example.tsx to a real,
// mounted production component — see App.tsx. CSS module was missing
// and has been provided in Session 10. Single line item only, no
// persistence, no IPC/API save call yet. Sufficient to prove end-to-end
// field population and rate tier calculation.

import React, { useState } from 'react';
import { AssetPicker, Asset } from './AssetPicker';
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
    rental_duration_days: number; // Phase 3.1: duration in days determines rate tier
    line_total: number;
    equipment_spec?: string;
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
        rental_duration_days: initialItem?.rental_duration_days || 30, // Phase 3.1: default 30 days
        equipment_spec: initialItem?.equipment_spec,
    });

    /**
     * Handle asset selection from AssetPicker
     * Automatically populates form fields with asset data
     */
    const handleAssetSelected = (asset: Asset) => {
        setSelectedAsset(asset);

        // Use the tauri-bridge utility to populate form fields
        const populated = populateQuoteItemFromAsset(asset, formData.rate_basis || 'Monthly');

        setFormData((prev) => ({
            ...prev,
            ...populated,
        }));
    };

    /**
     * Phase 3.1: Handle rental duration change
     * Auto-recalculates rate_basis and unit_rate based on duration
     */
    const handleDurationChange = (e: React.ChangeEvent<HTMLInputElement>) => {
        const durationDays = parseInt(e.target.value, 10) || 0;

        if (!selectedAsset || durationDays < 1) {
            // AUDIT FIX (re-applied Session 12 — was missing from the "corrected"
            // delivery despite WORKER_CARRYOVER claiming it was already included):
            // clear stale rate_basis/unit_rate instead of leaving last-valid values
            // on screen, and surface a real error instead of console-only.
            setDurationError(
                !selectedAsset
                    ? 'Select an asset before entering a rental duration.'
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

        // Calculate new tier and unit_rate based on duration
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
     * Calculate line total as quantity × unit_rate.
     *
     * AUDIT FIX (re-applied Session 12): this previously shadowed the imported,
     * tested `calculateLineTotal` from src-shared/rate-matrix.ts with a same-named
     * local const — legal JS/TS shadowing, so it compiled silently, but it meant
     * the 15 dedicated calculateLineTotal unit tests never covered what the UI
     * displays, and `formData.quantity || 1` silently treated a quantity of 0 as 1
     * in the live total. This now calls the real, tested engine function.
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
                    QUANTITY & RENTAL DURATION & RATE FIELDS
                    Phase 3.1: Added rental_duration_days field which drives
                    automatic rate tier calculation
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
                        <label htmlFor="rentalDuration">Rental Duration (days) *</label>
                        <input
                            id="rentalDuration"
                            type="number"
                            min="1"
                            value={formData.rental_duration_days || 30}
                            onChange={handleDurationChange}
                            placeholder="e.g., 5, 15, 30"
                            required
                        />
                        {durationError ? (
                            <div className={styles.errorText}>{durationError}</div>
                        ) : (
                            <div className={styles.helpText}>
                                Determines: {formData.rate_basis} tier
                            </div>
                        )}
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
