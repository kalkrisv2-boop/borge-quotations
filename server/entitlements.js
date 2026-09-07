// ============================================================================
// DEPRECATED — Phase R.5 (Full Click-Through & Stitch Point)
//
// This file is NO LONGER CALLED by the running app. Every command it used to
// handle (auth, entitlements, save_quote/fetch_quote, calculate_rate_matrix,
// generate_quote_pdf) is now served natively by src-tauri/src/{auth,quotes,
// rate_matrix,pdf_render}.rs, dispatched from src-tauri/src/lib.rs's
// handle_guarded_ipc. See route-map-verR-addendum.md, Phase R.5, deliverable 3
// ("Retire server/*.js from the shipped build... may be retained in the repo
// as historical reference / test oracle -- the constraint is no runtime
// callers, not delete the files").
//
// Retained here ONLY as:
//   (a) the original spec/oracle every R-phase Rust port was verified against
//       (R.2 auth, R.3 quotes, R.4 pdf_render all cite this file directly as
//       their behavioral reference), and
//   (b) historical record of the Phase 1.2-era architecture gap (see
//       PROJECT_BASELINE.md Session 15 entry) this whole Phase R arc exists to
//       close.
//
// Do not wire this back into any build target. Do not add new callers.
// ============================================================================

import { verifySessionToken } from './auth.js';

// FIX (Architect audit, Phase R.2): this was previously CANONICAL_MODULE_KEYS, defined
// here AND (independently, by hand) in src-tauri/src/lib.rs -- the exact
// dual-implementation drift risk Phase R exists to eliminate. The canonical list now
// has exactly one definition, in Rust (auth::CANONICAL_MODULE_KEYS). This is that same
// literal list, renamed and commented so it reads as the historical Node-side copy, not
// a second source of truth, kept only because server/*.js remains runtime-reachable
// until Phase R.5 retires it. Do not rename this back to CANONICAL_MODULE_KEYS, and do
// not treat this as authoritative if it ever disagrees with auth.rs's list -- auth.rs
// wins.
const NODE_MODULE_KEYS = Object.freeze([
  'quotes_core',
  'inventory_specs',
  'rate_matrix',
  'compliance_terms',
  'revision_lpo'
]);

// In-memory backing store for entitlements (replaces DB backend in standalone runtime)
const entitlementStore = new Map();

/**
 * Seed or update entitlements for a specific tenant
 */
export function seedTenantEntitlements(tenantId, entitlementsList) {
  if (!tenantId) throw new Error('tenant_id is required for entitlement seeding');
  
  const tenantMap = entitlementStore.get(tenantId) || new Map();
  for (const item of entitlementsList) {
    if (NODE_MODULE_KEYS.includes(item.module_key)) {
      tenantMap.set(item.module_key, Boolean(item.is_enabled));
    }
  }
  entitlementStore.set(tenantId, tenantMap);
}

/**
 * Actively evaluate entitlement for a given tenant and module key.
 * Enforces strict backend-level isolation.
 */
export function checkTenantEntitlement(tenantId, moduleKey) {
  if (!tenantId || !moduleKey) return false;
  if (!NODE_MODULE_KEYS.includes(moduleKey)) return false;

  const tenantMap = entitlementStore.get(tenantId);
  if (!tenantMap) return false;

  return tenantMap.get(moduleKey) === true;
}

/**
 * High-order guard wrapper that validates session tokens, checks module entitlements,
 * and traps unhandled business logic errors inside handlerFn.
 */
export function guardApiRoute(sessionToken, secretKey, requiredModuleKey, handlerFn, payload = {}) {
  const decodedSession = verifySessionToken(sessionToken, secretKey);
  if (!decodedSession) {
    return { status: 401, error: 'Unauthorized: Invalid or expired session token' };
  }

  const tenantId = decodedSession.tenant_id;
  if (!tenantId) {
    return { status: 403, error: 'Forbidden: Missing tenant scope in session' };
  }

  const isAllowed = checkTenantEntitlement(tenantId, requiredModuleKey);
  if (!isAllowed) {
    return { 
      status: 403, 
      error: `Access Denied: Tenant '${tenantId}' does not hold active entitlement for module '${requiredModuleKey}'` 
    };
  }

  // Safely execute handler with mandatory tenant isolation and exception trapping
  try {
    const result = handlerFn({ ...payload, tenant_id: tenantId, user_id: decodedSession.sub });
    return {
      status: 200,
      data: result
    };
  } catch (err) {
    console.error(`[Guard Error] Unhandled exception executing handler for module '${requiredModuleKey}':`, err);
    return {
      status: 500,
      error: `Internal Error: ${err.message || 'An unhandled error occurred while processing the request.'}`
    };
  }
}