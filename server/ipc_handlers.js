import { guardApiRoute, seedTenantEntitlements } from './entitlements.js';

const quotesDb = new Map();

export function handleIpcCommand(commandName, payload, sessionToken, secretKey) {
  switch (commandName) {
    case 'admin_seed_entitlements': {
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