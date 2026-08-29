import { verifySessionToken } from './auth.js';

export const CANONICAL_MODULE_KEYS = Object.freeze([
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
    if (CANONICAL_MODULE_KEYS.includes(item.module_key)) {
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
  if (!CANONICAL_MODULE_KEYS.includes(moduleKey)) return false;

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