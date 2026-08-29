import { verifySessionToken } from './auth.js';
import { guardApiRoute, seedTenantEntitlements } from './entitlements.js';

const quotesDb = new Map();

export function handleIpcCommand(commandName, payload, sessionToken, secretKey) {
  switch (commandName) {
    case 'admin_seed_entitlements': {
      // SECURITY FIX (Architect audit, Phase 1.2): this command previously had
      // NO auth check at all — any caller, authenticated or not, could grant
      // itself (or any arbitrary tenant_id) any entitlement. Confirmed
      // exploitable by direct test (see audit notes), not just read by eye.
      // Now requires a valid, verified session token belonging to an
      // 'admin'-role user before any entitlement write is accepted.
      const decodedSession = verifySessionToken(sessionToken, secretKey);
      if (!decodedSession) {
        return { status: 401, error: 'Unauthorized: Invalid or expired session token' };
      }
      if (decodedSession.role !== 'admin') {
        return { status: 403, error: 'Forbidden: Only admin-role sessions may seed entitlements' };
      }
      if (!payload || !payload.tenant_id || !Array.isArray(payload.entitlements)) {
        return { status: 400, error: 'Invalid payload for entitlement seed' };
      }
      seedTenantEntitlements(payload.tenant_id, payload.entitlements);
      return { status: 200, data: { success: true } };
    }

    case 'save_quote': {
      return guardApiRoute(sessionToken, secretKey, 'quotes_core', (contextPayload) => {
        const tenantId = contextPayload.tenant_id;
        const tenantQuotes = quotesDb.get(tenantId) || [];

        const quoteData = contextPayload.quote;
        if (!quoteData || !quoteData.offer_ref) {
          throw new Error('Invalid quote payload');
        }

        const existingIndex = tenantQuotes.findIndex(q => q.offer_ref === quoteData.offer_ref);
        if (existingIndex >= 0) {
          tenantQuotes[existingIndex] = { ...quoteData, tenant_id: tenantId, updated_at: new Date().toISOString() };
        } else {
          tenantQuotes.push({ ...quoteData, tenant_id: tenantId, created_at: new Date().toISOString(), updated_at: new Date().toISOString() });
        }

        quotesDb.set(tenantId, tenantQuotes);
        return { success: true, offer_ref: quoteData.offer_ref };
      }, payload);
    }

    case 'fetch_quote': {
      return guardApiRoute(sessionToken, secretKey, 'quotes_core', (contextPayload) => {
        const tenantId = contextPayload.tenant_id;
        const tenantQuotes = quotesDb.get(tenantId) || [];
        const found = tenantQuotes.find(q => q.offer_ref === contextPayload.offer_ref);

        if (!found) {
          return { error: 'Quote not found' };
        }
        return found;
      }, payload);
    }

    case 'fetch_inventory_specs': {
      return guardApiRoute(sessionToken, secretKey, 'inventory_specs', (contextPayload) => {
        return { tenant_id: contextPayload.tenant_id, items: [] };
      }, payload);
    }

    case 'calculate_rate_matrix': {
      return guardApiRoute(sessionToken, secretKey, 'rate_matrix', (contextPayload) => {
        return { tenant_id: contextPayload.tenant_id, calculated: true };
      }, payload);
    }

    default:
      return { status: 404, error: `Unknown IPC command: ${commandName}` };
  }
}