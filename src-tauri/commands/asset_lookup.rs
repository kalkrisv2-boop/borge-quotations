// src-tauri/commands/asset_lookup.rs
// Phase 2.2 — Asset lookup commands for quote line-item picker
// Dual-target: SQLite (local) + PostgreSQL (cloud)
// Tenant-isolated by explicit tenant_id filtering on every query
// Gated behind 'inventory_specs' entitlement module key

use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool, PgPool};
use std::sync::Arc;
use tauri::State;

// ============================================================
// DATA STRUCTURES
// ============================================================

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AssetDetail {
    pub id: String,
    pub tenant_id: String,
    pub category_id: String,
    pub asset_code: String,
    pub asset_name: String,
    pub description: Option<String>,
    pub make_model: Option<String>,
    pub serial_number: Option<String>,
    pub location: Option<String>,
    pub status: String,
    pub purchase_date: Option<String>,
    pub warranty_expiry: Option<String>,
    pub default_daily_rate: f64,
    pub default_weekly_rate: f64,
    pub default_monthly_rate: f64,
    pub equipment_specs: Vec<EquipmentSpec>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct EquipmentSpec {
    pub id: String,
    pub asset_id: String,
    pub spec_key: String,
    pub spec_value: String,
    pub unit_of_measure: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AssetSearchRequest {
    pub tenant_id: String,
    pub search_type: AssetSearchType,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", content = "value")]
pub enum AssetSearchType {
    ByCategory(String),           // category_id
    ByCode(String),               // asset_code (exact match)
    ByName(String),               // asset_name (contains search)
    ByStatus(String),             // status filter: Available, In-Use, Maintenance, Retired
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AssetLookupError {
    pub error: String,
    pub code: String,
}

// ============================================================
// SQLITE IMPLEMENTATION
// ============================================================

#[tauri::command]
pub async fn asset_lookup_sqlite(
    request: AssetSearchRequest,
    pool: State<'_, Arc<SqlitePool>>,
    tenant_check: State<'_, Arc<dyn TenantCheckFn>>,
) -> Result<Vec<AssetDetail>, AssetLookupError> {
    // Entitlement check (gated behind 'inventory_specs')
    if let Err(_) = tenant_check.check_entitlement(&request.tenant_id, "inventory_specs").await {
        return Err(AssetLookupError {
            error: "Entitlement 'inventory_specs' not enabled for this tenant".to_string(),
            code: "ENTITLEMENT_DENIED".to_string(),
        });
    }

    // Execute query based on search type, always filtering by tenant_id
    match request.search_type {
        AssetSearchType::ByCategory(category_id) => {
            asset_lookup_by_category_sqlite(&pool, &request.tenant_id, &category_id).await
        }
        AssetSearchType::ByCode(asset_code) => {
            asset_lookup_by_code_sqlite(&pool, &request.tenant_id, &asset_code).await
        }
        AssetSearchType::ByName(name_search) => {
            asset_lookup_by_name_sqlite(&pool, &request.tenant_id, &name_search).await
        }
        AssetSearchType::ByStatus(status) => {
            asset_lookup_by_status_sqlite(&pool, &request.tenant_id, &status).await
        }
    }
}

async fn asset_lookup_by_category_sqlite(
    pool: &SqlitePool,
    tenant_id: &str,
    category_id: &str,
) -> Result<Vec<AssetDetail>, AssetLookupError> {
    let assets = sqlx::query_as::<_, (String, String, String, String, String, Option<String>, Option<String>, Option<String>, Option<String>, String, Option<String>, Option<String>, f64, f64, f64)>(
        "SELECT id, tenant_id, category_id, asset_code, asset_name, description, make_model, serial_number, location, status, purchase_date, warranty_expiry, default_daily_rate, default_weekly_rate, default_monthly_rate FROM assets WHERE tenant_id = ? AND category_id = ?"
    )
    .bind(tenant_id)
    .bind(category_id)
    .fetch_all(pool)
    .await
    .map_err(|e| AssetLookupError {
        error: format!("Database error: {}", e),
        code: "DB_ERROR".to_string(),
    })?;

    let mut results = Vec::new();
    for (id, asset_tenant_id, cat_id, code, name, desc, make_model, serial, loc, status, purch_date, warranty, daily_rate, weekly_rate, monthly_rate) in assets {
        let specs = fetch_equipment_specs_sqlite(pool, tenant_id, &id).await?;
        results.push(AssetDetail {
            id,
            tenant_id: asset_tenant_id,
            category_id: cat_id,
            asset_code: code,
            asset_name: name,
            description: desc,
            make_model,
            serial_number: serial,
            location: loc,
            status,
            purchase_date: purch_date,
            warranty_expiry: warranty,
            default_daily_rate: daily_rate,
            default_weekly_rate: weekly_rate,
            default_monthly_rate: monthly_rate,
            equipment_specs: specs,
        });
    }

    Ok(results)
}

async fn asset_lookup_by_code_sqlite(
    pool: &SqlitePool,
    tenant_id: &str,
    asset_code: &str,
) -> Result<Vec<AssetDetail>, AssetLookupError> {
    let asset = sqlx::query_as::<_, (String, String, String, String, String, Option<String>, Option<String>, Option<String>, Option<String>, String, Option<String>, Option<String>, f64, f64, f64)>(
        "SELECT id, tenant_id, category_id, asset_code, asset_name, description, make_model, serial_number, location, status, purchase_date, warranty_expiry, default_daily_rate, default_weekly_rate, default_monthly_rate FROM assets WHERE tenant_id = ? AND asset_code = ?"
    )
    .bind(tenant_id)
    .bind(asset_code)
    .fetch_optional(pool)
    .await
    .map_err(|e| AssetLookupError {
        error: format!("Database error: {}", e),
        code: "DB_ERROR".to_string(),
    })?;

    if let Some((id, asset_tenant_id, cat_id, code, name, desc, make_model, serial, loc, status, purch_date, warranty, daily_rate, weekly_rate, monthly_rate)) = asset {
        let specs = fetch_equipment_specs_sqlite(pool, tenant_id, &id).await?;
        Ok(vec![AssetDetail {
            id,
            tenant_id: asset_tenant_id,
            category_id: cat_id,
            asset_code: code,
            asset_name: name,
            description: desc,
            make_model,
            serial_number: serial,
            location: loc,
            status,
            purchase_date: purch_date,
            warranty_expiry: warranty,
            default_daily_rate: daily_rate,
            default_weekly_rate: weekly_rate,
            default_monthly_rate: monthly_rate,
            equipment_specs: specs,
        }])
    } else {
        Ok(vec![])
    }
}

async fn asset_lookup_by_name_sqlite(
    pool: &SqlitePool,
    tenant_id: &str,
    name_search: &str,
) -> Result<Vec<AssetDetail>, AssetLookupError> {
    let search_pattern = format!("%{}%", name_search);
    let assets = sqlx::query_as::<_, (String, String, String, String, String, Option<String>, Option<String>, Option<String>, Option<String>, String, Option<String>, Option<String>, f64, f64, f64)>(
        "SELECT id, tenant_id, category_id, asset_code, asset_name, description, make_model, serial_number, location, status, purchase_date, warranty_expiry, default_daily_rate, default_weekly_rate, default_monthly_rate FROM assets WHERE tenant_id = ? AND asset_name LIKE ?"
    )
    .bind(tenant_id)
    .bind(search_pattern)
    .fetch_all(pool)
    .await
    .map_err(|e| AssetLookupError {
        error: format!("Database error: {}", e),
        code: "DB_ERROR".to_string(),
    })?;

    let mut results = Vec::new();
    for (id, asset_tenant_id, cat_id, code, name, desc, make_model, serial, loc, status, purch_date, warranty, daily_rate, weekly_rate, monthly_rate) in assets {
        let specs = fetch_equipment_specs_sqlite(pool, tenant_id, &id).await?;
        results.push(AssetDetail {
            id,
            tenant_id: asset_tenant_id,
            category_id: cat_id,
            asset_code: code,
            asset_name: name,
            description: desc,
            make_model,
            serial_number: serial,
            location: loc,
            status,
            purchase_date: purch_date,
            warranty_expiry: warranty,
            default_daily_rate: daily_rate,
            default_weekly_rate: weekly_rate,
            default_monthly_rate: monthly_rate,
            equipment_specs: specs,
        });
    }

    Ok(results)
}

async fn asset_lookup_by_status_sqlite(
    pool: &SqlitePool,
    tenant_id: &str,
    status: &str,
) -> Result<Vec<AssetDetail>, AssetLookupError> {
    let assets = sqlx::query_as::<_, (String, String, String, String, String, Option<String>, Option<String>, Option<String>, Option<String>, String, Option<String>, Option<String>, f64, f64, f64)>(
        "SELECT id, tenant_id, category_id, asset_code, asset_name, description, make_model, serial_number, location, status, purchase_date, warranty_expiry, default_daily_rate, default_weekly_rate, default_monthly_rate FROM assets WHERE tenant_id = ? AND status = ?"
    )
    .bind(tenant_id)
    .bind(status)
    .fetch_all(pool)
    .await
    .map_err(|e| AssetLookupError {
        error: format!("Database error: {}", e),
        code: "DB_ERROR".to_string(),
    })?;

    let mut results = Vec::new();
    for (id, asset_tenant_id, cat_id, code, name, desc, make_model, serial, loc, status_val, purch_date, warranty, daily_rate, weekly_rate, monthly_rate) in assets {
        let specs = fetch_equipment_specs_sqlite(pool, tenant_id, &id).await?;
        results.push(AssetDetail {
            id,
            tenant_id: asset_tenant_id,
            category_id: cat_id,
            asset_code: code,
            asset_name: name,
            description: desc,
            make_model,
            serial_number: serial,
            location: loc,
            status: status_val,
            purchase_date: purch_date,
            warranty_expiry: warranty,
            default_daily_rate: daily_rate,
            default_weekly_rate: weekly_rate,
            default_monthly_rate: monthly_rate,
            equipment_specs: specs,
        });
    }

    Ok(results)
}

async fn fetch_equipment_specs_sqlite(
    pool: &SqlitePool,
    tenant_id: &str,
    asset_id: &str,
) -> Result<Vec<EquipmentSpec>, AssetLookupError> {
    let specs = sqlx::query_as::<_, (String, String, String, String, Option<String>)>(
        "SELECT id, asset_id, spec_key, spec_value, unit_of_measure FROM equipment_specs WHERE tenant_id = ? AND asset_id = ?"
    )
    .bind(tenant_id)
    .bind(asset_id)
    .fetch_all(pool)
    .await
    .map_err(|e| AssetLookupError {
        error: format!("Database error fetching specs: {}", e),
        code: "DB_ERROR".to_string(),
    })?;

    Ok(specs
        .into_iter()
        .map(|(id, asset_id, key, value, unit)| EquipmentSpec {
            id,
            asset_id,
            spec_key: key,
            spec_value: value,
            unit_of_measure: unit,
        })
        .collect())
}

// ============================================================
// POSTGRESQL IMPLEMENTATION
// ============================================================

#[tauri::command]
pub async fn asset_lookup_postgres(
    request: AssetSearchRequest,
    pool: State<'_, Arc<PgPool>>,
    tenant_check: State<'_, Arc<dyn TenantCheckFn>>,
) -> Result<Vec<AssetDetail>, AssetLookupError> {
    // Entitlement check (gated behind 'inventory_specs')
    if let Err(_) = tenant_check.check_entitlement(&request.tenant_id, "inventory_specs").await {
        return Err(AssetLookupError {
            error: "Entitlement 'inventory_specs' not enabled for this tenant".to_string(),
            code: "ENTITLEMENT_DENIED".to_string(),
        });
    }

    // Execute query based on search type, always filtering by tenant_id
    match request.search_type {
        AssetSearchType::ByCategory(category_id) => {
            asset_lookup_by_category_postgres(&pool, &request.tenant_id, &category_id).await
        }
        AssetSearchType::ByCode(asset_code) => {
            asset_lookup_by_code_postgres(&pool, &request.tenant_id, &asset_code).await
        }
        AssetSearchType::ByName(name_search) => {
            asset_lookup_by_name_postgres(&pool, &request.tenant_id, &name_search).await
        }
        AssetSearchType::ByStatus(status) => {
            asset_lookup_by_status_postgres(&pool, &request.tenant_id, &status).await
        }
    }
}

async fn asset_lookup_by_category_postgres(
    pool: &PgPool,
    tenant_id: &str,
    category_id: &str,
) -> Result<Vec<AssetDetail>, AssetLookupError> {
    let assets = sqlx::query_as::<_, (String, String, String, String, String, Option<String>, Option<String>, Option<String>, Option<String>, String, Option<String>, Option<String>, f64, f64, f64)>(
        "SELECT id, tenant_id, category_id, asset_code, asset_name, description, make_model, serial_number, location, status, purchase_date, warranty_expiry, default_daily_rate, default_weekly_rate, default_monthly_rate FROM assets WHERE tenant_id = $1 AND category_id = $2"
    )
    .bind(tenant_id)
    .bind(category_id)
    .fetch_all(pool)
    .await
    .map_err(|e| AssetLookupError {
        error: format!("Database error: {}", e),
        code: "DB_ERROR".to_string(),
    })?;

    let mut results = Vec::new();
    for (id, asset_tenant_id, cat_id, code, name, desc, make_model, serial, loc, status, purch_date, warranty, daily_rate, weekly_rate, monthly_rate) in assets {
        let specs = fetch_equipment_specs_postgres(pool, tenant_id, &id).await?;
        results.push(AssetDetail {
            id,
            tenant_id: asset_tenant_id,
            category_id: cat_id,
            asset_code: code,
            asset_name: name,
            description: desc,
            make_model,
            serial_number: serial,
            location: loc,
            status,
            purchase_date: purch_date,
            warranty_expiry: warranty,
            default_daily_rate: daily_rate,
            default_weekly_rate: weekly_rate,
            default_monthly_rate: monthly_rate,
            equipment_specs: specs,
        });
    }

    Ok(results)
}

async fn asset_lookup_by_code_postgres(
    pool: &PgPool,
    tenant_id: &str,
    asset_code: &str,
) -> Result<Vec<AssetDetail>, AssetLookupError> {
    let asset = sqlx::query_as::<_, (String, String, String, String, String, Option<String>, Option<String>, Option<String>, Option<String>, String, Option<String>, Option<String>, f64, f64, f64)>(
        "SELECT id, tenant_id, category_id, asset_code, asset_name, description, make_model, serial_number, location, status, purchase_date, warranty_expiry, default_daily_rate, default_weekly_rate, default_monthly_rate FROM assets WHERE tenant_id = $1 AND asset_code = $2"
    )
    .bind(tenant_id)
    .bind(asset_code)
    .fetch_optional(pool)
    .await
    .map_err(|e| AssetLookupError {
        error: format!("Database error: {}", e),
        code: "DB_ERROR".to_string(),
    })?;

    if let Some((id, asset_tenant_id, cat_id, code, name, desc, make_model, serial, loc, status, purch_date, warranty, daily_rate, weekly_rate, monthly_rate)) = asset {
        let specs = fetch_equipment_specs_postgres(pool, tenant_id, &id).await?;
        Ok(vec![AssetDetail {
            id,
            tenant_id: asset_tenant_id,
            category_id: cat_id,
            asset_code: code,
            asset_name: name,
            description: desc,
            make_model,
            serial_number: serial,
            location: loc,
            status,
            purchase_date: purch_date,
            warranty_expiry: warranty,
            default_daily_rate: daily_rate,
            default_weekly_rate: weekly_rate,
            default_monthly_rate: monthly_rate,
            equipment_specs: specs,
        }])
    } else {
        Ok(vec![])
    }
}

async fn asset_lookup_by_name_postgres(
    pool: &PgPool,
    tenant_id: &str,
    name_search: &str,
) -> Result<Vec<AssetDetail>, AssetLookupError> {
    let search_pattern = format!("%{}%", name_search);
    let assets = sqlx::query_as::<_, (String, String, String, String, String, Option<String>, Option<String>, Option<String>, Option<String>, String, Option<String>, Option<String>, f64, f64, f64)>(
        "SELECT id, tenant_id, category_id, asset_code, asset_name, description, make_model, serial_number, location, status, purchase_date, warranty_expiry, default_daily_rate, default_weekly_rate, default_monthly_rate FROM assets WHERE tenant_id = $1 AND asset_name LIKE $2"
    )
    .bind(tenant_id)
    .bind(search_pattern)
    .fetch_all(pool)
    .await
    .map_err(|e| AssetLookupError {
        error: format!("Database error: {}", e),
        code: "DB_ERROR".to_string(),
    })?;

    let mut results = Vec::new();
    for (id, asset_tenant_id, cat_id, code, name, desc, make_model, serial, loc, status, purch_date, warranty, daily_rate, weekly_rate, monthly_rate) in assets {
        let specs = fetch_equipment_specs_postgres(pool, tenant_id, &id).await?;
        results.push(AssetDetail {
            id,
            tenant_id: asset_tenant_id,
            category_id: cat_id,
            asset_code: code,
            asset_name: name,
            description: desc,
            make_model,
            serial_number: serial,
            location: loc,
            status,
            purchase_date: purch_date,
            warranty_expiry: warranty,
            default_daily_rate: daily_rate,
            default_weekly_rate: weekly_rate,
            default_monthly_rate: monthly_rate,
            equipment_specs: specs,
        });
    }

    Ok(results)
}

async fn asset_lookup_by_status_postgres(
    pool: &PgPool,
    tenant_id: &str,
    status: &str,
) -> Result<Vec<AssetDetail>, AssetLookupError> {
    let assets = sqlx::query_as::<_, (String, String, String, String, String, Option<String>, Option<String>, Option<String>, Option<String>, String, Option<String>, Option<String>, f64, f64, f64)>(
        "SELECT id, tenant_id, category_id, asset_code, asset_name, description, make_model, serial_number, location, status, purchase_date, warranty_expiry, default_daily_rate, default_weekly_rate, default_monthly_rate FROM assets WHERE tenant_id = $1 AND status = $2"
    )
    .bind(tenant_id)
    .bind(status)
    .fetch_all(pool)
    .await
    .map_err(|e| AssetLookupError {
        error: format!("Database error: {}", e),
        code: "DB_ERROR".to_string(),
    })?;

    let mut results = Vec::new();
    for (id, asset_tenant_id, cat_id, code, name, desc, make_model, serial, loc, status_val, purch_date, warranty, daily_rate, weekly_rate, monthly_rate) in assets {
        let specs = fetch_equipment_specs_postgres(pool, tenant_id, &id).await?;
        results.push(AssetDetail {
            id,
            tenant_id: asset_tenant_id,
            category_id: cat_id,
            asset_code: code,
            asset_name: name,
            description: desc,
            make_model,
            serial_number: serial,
            location: loc,
            status: status_val,
            purchase_date: purch_date,
            warranty_expiry: warranty,
            default_daily_rate: daily_rate,
            default_weekly_rate: weekly_rate,
            default_monthly_rate: monthly_rate,
            equipment_specs: specs,
        });
    }

    Ok(results)
}

async fn fetch_equipment_specs_postgres(
    pool: &PgPool,
    tenant_id: &str,
    asset_id: &str,
) -> Result<Vec<EquipmentSpec>, AssetLookupError> {
    let specs = sqlx::query_as::<_, (String, String, String, String, Option<String>)>(
        "SELECT id, asset_id, spec_key, spec_value, unit_of_measure FROM equipment_specs WHERE tenant_id = $1 AND asset_id = $2"
    )
    .bind(tenant_id)
    .bind(asset_id)
    .fetch_all(pool)
    .await
    .map_err(|e| AssetLookupError {
        error: format!("Database error fetching specs: {}", e),
        code: "DB_ERROR".to_string(),
    })?;

    Ok(specs
        .into_iter()
        .map(|(id, asset_id, key, value, unit)| EquipmentSpec {
            id,
            asset_id,
            spec_key: key,
            spec_value: value,
            unit_of_measure: unit,
        })
        .collect())
}

// ============================================================
// TRAIT FOR ENTITLEMENT CHECKING
// ============================================================
//
// AUDIT NOTE (Phase 2.2 Architect audit, Session 9): the version of this file
// as delivered by the Worker shipped a `DefaultTenantCheck` whose
// `check_entitlement()` ignored both arguments and unconditionally returned
// `Ok(())`. That is a hard-coded entitlement bypass, not a stub awaiting
// wiring — every call to `asset_lookup_sqlite` / `asset_lookup_postgres`
// would have passed the entitlement gate for any tenant_id and any
// module_key, including a tenant with zero rows in `tenant_entitlements`.
// This directly violates PROJECT_BASELINE.md Section 1.3 ("a locked
// module's API/IPC calls must actively reject unlicensed tenants").
// Confirmed by inspection (the params are never touched, no DB handle is
// even held) rather than by execution, since compiling/running the full
// Tauri app is outside the scope of this audit window — the logic itself
// is sufficient to prove the defect regardless of runtime behavior.
//
// Fixed below with two concrete, DB-backed implementations that run the
// same `SELECT is_enabled FROM tenant_entitlements WHERE tenant_id = ?
// AND module_key = ?` check the Express route (asset_lookup.js) already
// performs correctly, so both backends enforce the identical rule.

#[async_trait::async_trait]
pub trait TenantCheckFn: Send + Sync {
    async fn check_entitlement(&self, tenant_id: &str, module_key: &str) -> Result<(), String>;
}

/// SQLite-backed entitlement check.
pub struct SqliteTenantCheck {
    pub pool: Arc<SqlitePool>,
}

#[async_trait::async_trait]
impl TenantCheckFn for SqliteTenantCheck {
    async fn check_entitlement(&self, tenant_id: &str, module_key: &str) -> Result<(), String> {
        let row: Option<(i64,)> = sqlx::query_as(
            "SELECT is_enabled FROM tenant_entitlements WHERE tenant_id = ? AND module_key = ?"
        )
        .bind(tenant_id)
        .bind(module_key)
        .fetch_optional(&*self.pool)
        .await
        .map_err(|e| format!("Entitlement check DB error: {}", e))?;

        match row {
            Some((is_enabled,)) if is_enabled != 0 => Ok(()),
            _ => Err(format!(
                "Entitlement '{}' not enabled for tenant '{}'",
                module_key, tenant_id
            )),
        }
    }
}

/// PostgreSQL-backed entitlement check.
pub struct PgTenantCheck {
    pub pool: Arc<PgPool>,
}

#[async_trait::async_trait]
impl TenantCheckFn for PgTenantCheck {
    async fn check_entitlement(&self, tenant_id: &str, module_key: &str) -> Result<(), String> {
        let row: Option<(bool,)> = sqlx::query_as(
            "SELECT is_enabled FROM tenant_entitlements WHERE tenant_id = $1 AND module_key = $2"
        )
        .bind(tenant_id)
        .bind(module_key)
        .fetch_optional(&*self.pool)
        .await
        .map_err(|e| format!("Entitlement check DB error: {}", e))?;

        match row {
            Some((is_enabled,)) if is_enabled => Ok(()),
            _ => Err(format!(
                "Entitlement '{}' not enabled for tenant '{}'",
                module_key, tenant_id
            )),
        }
    }
}

// Wiring note for main.rs / lib.rs app initialization (not part of this
// phase's file set, flagged here so it isn't missed):
//   .manage(Arc::new(SqlitePool::connect(...).await?))
//   .manage(Arc::new(SqliteTenantCheck { pool: <same Arc<SqlitePool> above> }) as Arc<dyn TenantCheckFn>)
// and equivalently for the Postgres pool/PgTenantCheck when that target is active.
// Do NOT register `DefaultTenantCheck` (removed) — it no longer exists in this file.
