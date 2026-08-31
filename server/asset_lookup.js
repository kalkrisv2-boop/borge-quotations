// server/routes/asset_lookup.js
// Phase 2.2 — Asset lookup HTTP API endpoints
// Tenant-isolated by explicit tenant_id filtering on every query
// Gated behind 'inventory_specs' entitlement module key
// Node.js/Express backend

const express = require('express');
const { v4: uuidv4 } = require('uuid');
const router = express.Router();

// Middleware to check entitlements (assumes it's injected into the route context)
const checkEntitlement = (moduleKey) => {
    return async (req, res, next) => {
        if (!req.tenant) {
            return res.status(401).json({ error: 'Tenant context missing', code: 'NO_TENANT' });
        }

        try {
            const entitlementResult = await req.db.get(
                'SELECT is_enabled FROM tenant_entitlements WHERE tenant_id = ? AND module_key = ?',
                [req.tenant.id, moduleKey]
            );

            if (!entitlementResult || !entitlementResult.is_enabled) {
                return res.status(403).json({
                    error: `Entitlement '${moduleKey}' not enabled for this tenant`,
                    code: 'ENTITLEMENT_DENIED',
                });
            }

            next();
        } catch (err) {
            return res.status(500).json({ error: 'Entitlement check failed', code: 'DB_ERROR' });
        }
    };
};

// ============================================================
// GET /assets/search — Multi-parameter asset search
// ============================================================
// Query parameters:
//   - search_type: 'category', 'code', 'name', or 'status'
//   - value: the search value (category_id, asset_code, name pattern, or status)
// Response: array of AssetDetail objects with embedded equipment_specs
router.get('/search', checkEntitlement('inventory_specs'), async (req, res) => {
    const { search_type, value } = req.query;
    const tenantId = req.tenant.id;

    if (!search_type || !value) {
        return res.status(400).json({
            error: 'Missing required query parameters: search_type and value',
            code: 'INVALID_REQUEST',
        });
    }

    try {
        let assets = [];

        switch (search_type) {
            case 'category':
                assets = await assetsByCategory(req.db, tenantId, value);
                break;
            case 'code':
                assets = await assetsByCode(req.db, tenantId, value);
                break;
            case 'name':
                assets = await assetsByName(req.db, tenantId, value);
                break;
            case 'status':
                assets = await assetsByStatus(req.db, tenantId, value);
                break;
            default:
                return res.status(400).json({
                    error: `Unknown search_type: ${search_type}. Valid types: category, code, name, status`,
                    code: 'INVALID_SEARCH_TYPE',
                });
        }

        // Enrich each asset with equipment specs
        const enriched = await Promise.all(
            assets.map(async (asset) => ({
                ...asset,
                equipment_specs: await fetchEquipmentSpecs(req.db, tenantId, asset.id),
            }))
        );

        res.json(enriched);
    } catch (err) {
        console.error('Asset search error:', err);
        res.status(500).json({ error: 'Asset search failed', code: 'DB_ERROR' });
    }
});

// ============================================================
// GET /assets/:id — Fetch a single asset by ID
// ============================================================
// Response: single AssetDetail object with embedded equipment_specs
router.get('/:id', checkEntitlement('inventory_specs'), async (req, res) => {
    const { id } = req.params;
    const tenantId = req.tenant.id;

    try {
        // Tenant-scoped query: MUST include tenant_id in WHERE clause
        const asset = await req.db.get(
            `SELECT 
                id, tenant_id, category_id, asset_code, asset_name, description, 
                make_model, serial_number, location, status, purchase_date, warranty_expiry,
                default_daily_rate, default_weekly_rate, default_monthly_rate
             FROM assets 
             WHERE id = ? AND tenant_id = ?`,
            [id, tenantId]
        );

        if (!asset) {
            return res.status(404).json({
                error: 'Asset not found',
                code: 'NOT_FOUND',
            });
        }

        // Enrich with equipment specs
        const specs = await fetchEquipmentSpecs(req.db, tenantId, id);

        res.json({
            ...asset,
            equipment_specs: specs,
        });
    } catch (err) {
        console.error('Asset fetch error:', err);
        res.status(500).json({ error: 'Asset fetch failed', code: 'DB_ERROR' });
    }
});

// ============================================================
// GET /assets/category/:categoryId/list — List all assets in a category
// ============================================================
// Response: array of AssetDetail objects
router.get('/category/:categoryId/list', checkEntitlement('inventory_specs'), async (req, res) => {
    const { categoryId } = req.params;
    const tenantId = req.tenant.id;

    try {
        // Verify category belongs to tenant
        const category = await req.db.get(
            'SELECT id FROM asset_categories WHERE id = ? AND tenant_id = ?',
            [categoryId, tenantId]
        );

        if (!category) {
            return res.status(404).json({
                error: 'Category not found or does not belong to this tenant',
                code: 'NOT_FOUND',
            });
        }

        // Fetch all assets in this category, tenant-scoped
        const assets = await req.db.all(
            `SELECT 
                id, tenant_id, category_id, asset_code, asset_name, description, 
                make_model, serial_number, location, status, purchase_date, warranty_expiry,
                default_daily_rate, default_weekly_rate, default_monthly_rate
             FROM assets 
             WHERE category_id = ? AND tenant_id = ?
             ORDER BY asset_name ASC`,
            [categoryId, tenantId]
        );

        // Enrich each with equipment specs
        const enriched = await Promise.all(
            assets.map(async (asset) => ({
                ...asset,
                equipment_specs: await fetchEquipmentSpecs(req.db, tenantId, asset.id),
            }))
        );

        res.json(enriched);
    } catch (err) {
        console.error('Category asset listing error:', err);
        res.status(500).json({ error: 'Category asset listing failed', code: 'DB_ERROR' });
    }
});

// ============================================================
// GET /assets/available — List all available assets
// ============================================================
// Response: array of AssetDetail objects with status = 'Available'
router.get('/', checkEntitlement('inventory_specs'), async (req, res) => {
    const tenantId = req.tenant.id;

    try {
        const assets = await req.db.all(
            `SELECT 
                id, tenant_id, category_id, asset_code, asset_name, description, 
                make_model, serial_number, location, status, purchase_date, warranty_expiry,
                default_daily_rate, default_weekly_rate, default_monthly_rate
             FROM assets 
             WHERE tenant_id = ?
             ORDER BY asset_name ASC`,
            [tenantId]
        );

        // Enrich each with equipment specs
        const enriched = await Promise.all(
            assets.map(async (asset) => ({
                ...asset,
                equipment_specs: await fetchEquipmentSpecs(req.db, tenantId, asset.id),
            }))
        );

        res.json(enriched);
    } catch (err) {
        console.error('Assets listing error:', err);
        res.status(500).json({ error: 'Assets listing failed', code: 'DB_ERROR' });
    }
});

// ============================================================
// HELPER FUNCTIONS
// ============================================================

async function assetsByCategory(db, tenantId, categoryId) {
    return db.all(
        `SELECT 
            id, tenant_id, category_id, asset_code, asset_name, description, 
            make_model, serial_number, location, status, purchase_date, warranty_expiry,
            default_daily_rate, default_weekly_rate, default_monthly_rate
         FROM assets 
         WHERE tenant_id = ? AND category_id = ?`,
        [tenantId, categoryId]
    );
}

async function assetsByCode(db, tenantId, assetCode) {
    const result = await db.get(
        `SELECT 
            id, tenant_id, category_id, asset_code, asset_name, description, 
            make_model, serial_number, location, status, purchase_date, warranty_expiry,
            default_daily_rate, default_weekly_rate, default_monthly_rate
         FROM assets 
         WHERE tenant_id = ? AND asset_code = ?`,
        [tenantId, assetCode]
    );
    return result ? [result] : [];
}

async function assetsByName(db, tenantId, nameSearch) {
    const pattern = `%${nameSearch}%`;
    return db.all(
        `SELECT 
            id, tenant_id, category_id, asset_code, asset_name, description, 
            make_model, serial_number, location, status, purchase_date, warranty_expiry,
            default_daily_rate, default_weekly_rate, default_monthly_rate
         FROM assets 
         WHERE tenant_id = ? AND asset_name LIKE ?`,
        [tenantId, pattern]
    );
}

async function assetsByStatus(db, tenantId, status) {
    return db.all(
        `SELECT 
            id, tenant_id, category_id, asset_code, asset_name, description, 
            make_model, serial_number, location, status, purchase_date, warranty_expiry,
            default_daily_rate, default_weekly_rate, default_monthly_rate
         FROM assets 
         WHERE tenant_id = ? AND status = ?`,
        [tenantId, status]
    );
}

async function fetchEquipmentSpecs(db, tenantId, assetId) {
    return db.all(
        `SELECT 
            id, tenant_id, asset_id, spec_key, spec_value, unit_of_measure
         FROM equipment_specs 
         WHERE tenant_id = ? AND asset_id = ?`,
        [tenantId, assetId]
    );
}

module.exports = router;
