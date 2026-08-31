// src/components/QuoteLineItemEditor.tsx
// Promoted (this session) from QuoteLineItemEditor.example.tsx to a real,
// mounted production component — see App.tsx.
//
// AUDIT NOTE: this file imports ./QuoteLineItemEditor.module.css, which
// never existed anywhere in this project's delivery history across any
// prior session. Would have broken the build the moment this component was
// actually mounted (which is what's happening now that it's wired into
// App.tsx). A minimal, functional version of that CSS module is delivered
// alongside this file.
//
// Still NOT a complete quote-builder: single line item only, no
// persistence, no IPC/API save call. Sufficient to prove AssetPicker's
// end-to-end field population; not sufficient to ship as a real feature.

import React, { useState } from 'react';
import { AssetPicker, Asset } from './AssetPicker';
// AUDIT FIX (Session 9): same broken-path defect as AssetPicker.tsx — corrected
// to the real canonical bridge location (src-shared/tauri-bridge.js).
import { populateQuoteItemFromAsset } from '../../src-shared/tauri-bridge';
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
    const [formData, setFormData] = useState<Partial<QuoteLineItem>>({
        quote_id: quoteId,
        item_order: itemOrder,
        item_description: initialItem?.item_description || '',
        make_model: initialItem?.make_model,
        quantity: initialItem?.quantity || 1,
        unit_rate: initialItem?.unit_rate || 0,
        rate_basis: initialItem?.rate_basis || 'Monthly',
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
     * Handle rate_basis change
     * Updates unit_rate to match the new basis (from asset's default rates)
     */
    const handleRateBasisChange = (e: React.ChangeEvent<HTMLSelectElement>) => {
        const newBasis = e.target.value as 'Daily' | 'Weekly' | 'Monthly';

        setFormData((prev) => ({
            ...prev,
            rate_basis: newBasis,
        }));

        // If an asset is selected, update the unit_rate to match the new basis
        if (selectedAsset) {
            let newRate = 0;
            switch (newBasis) {
                case 'Daily':
                    newRate = selectedAsset.default_daily_rate;
                    break;
                case 'Weekly':
                    newRate = selectedAsset.default_weekly_rate;
                    break;
                case 'Monthly':
                default:
                    newRate = selectedAsset.default_monthly_rate;
                    break;
            }

            setFormData((prev) => ({
                ...prev,
                unit_rate: newRate,
            }));
        }
    };

    /**
     * Calculate line total as quantity * unit_rate
     */
    const calculateLineTotal = (): number => {
        return (formData.quantity || 1) * (formData.unit_rate || 0);
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
            line_total: calculateLineTotal(),
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
                    QUANTITY & RATE FIELDS
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
                        <label htmlFor="rateBasis">Rate Basis *</label>
                        <select
                            id="rateBasis"
                            value={formData.rate_basis || 'Monthly'}
                            onChange={handleRateBasisChange}
                            required
                        >
                            <option value="Daily">Daily</option>
                            <option value="Weekly">Weekly</option>
                            <option value="Monthly">Monthly</option>
                        </select>
                    </div>

                    <div className={styles.formGroup}>
                        <label htmlFor="unitRate">Unit Rate (AED) *</label>
                        <input
                            id="unitRate"
                            type="number"
                            min="0"
                            step="0.01"
                            value={formData.unit_rate || 0}
                            onChange={(e) =>
                                setFormData((prev) => ({
                                    ...prev,
                                    unit_rate: parseFloat(e.target.value) || 0,
                                }))
                            }
                            required
                        />
                    </div>

                    <div className={styles.formGroup}>
                        <label>Line Total (AED)</label>
                        <div className={styles.lineTotal}>
                            {calculateLineTotal().toFixed(2)}
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
