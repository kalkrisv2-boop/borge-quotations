import { verifySessionToken } from './auth.js';
import { guardApiRoute, seedTenantEntitlements } from './entitlements.js';
// Import the legend generator helper. NOTE: RateBasisLegend is a .tsx module —
// this only resolves at runtime if the server bundle is built/transpiled
// (e.g. via the same Vite/tsc pipeline as the frontend) before ipc_handlers.js
// runs. If ipc_handlers.js is ever executed directly under plain Node without
// a build step, this import will fail because Node cannot load .tsx directly.
import { getRateBasisLegendHTML } from '../src/components/RateBasisLegend';
// Phase 3.2 completion: the actual render step. See server/pdf_render.js for
// the architecture note on the WeasyPrint dependency this introduces.
import { renderQuotePdfFile } from './pdf_render.js';

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

    // PDF EXPORT HANDLER (Phase 1.4 / 3.2)
    // AUDIT NOTE: this case did not exist anywhere in the delivered project —
    // grep across src/, server/, and templates/ found no other file that
    // builds this contextPayload or calls getRateBasisLegendHTML() for export.
    // quote_pdf_template.html expects customer_name, offer_reference, po_box,
    // quote_date, location, page_count, contact_person, sales_person,
    // equipment_category, rate_basis, line_items, subtotal, vat_amount,
    // grand_total, terms_line_1..9, rate_basis_legend_html, and
    // authorized_signatory. Gated behind the same 'quotes_core' entitlement
    // as save/fetch, since PDF export is a quotes_core capability, not a
    // separate module key in CANONICAL_MODULE_KEYS.
    case 'generate_quote_pdf': {
      return guardApiRoute(sessionToken, secretKey, 'quotes_core', (contextPayload) => {
        const tenantId = contextPayload.tenant_id;
        const tenantQuotes = quotesDb.get(tenantId) || [];
        const quote = tenantQuotes.find(q => q.offer_ref === contextPayload.offer_ref);

        if (!quote) {
          throw new Error(`Quote not found for offer_ref '${contextPayload.offer_ref}'`);
        }

        const lineItems = quote.line_items || [];
        const subtotal = lineItems.reduce(
          (sum, item) => sum + (Number(item.unit_rate) || 0) * (Number(item.quantity) || 0),
          0
        );
        const vatAmount = subtotal * 0.05;
        const grandTotal = subtotal + vatAmount;

        // Template render is intentionally left to the caller's HTML/print
        // pipeline (Phase 1.3 Paged Media engine) — this handler's job is to
        // assemble the exact data contract templates/quote_pdf_template.html
        // expects, not to perform the print/PDF rasterization itself.
        return {
          customer_name: quote.customer_name,
          offer_reference: quote.offer_ref,
          po_box: quote.po_box || '',
          quote_date: quote.quote_date,
          location: quote.location || '',
          page_count: quote.page_count || '1/1',
          contact_person: quote.contact_person || '',
          sales_person: quote.sales_person || '',
          equipment_category: quote.equipment_category || '',
          rate_basis: quote.rate_basis || 'Monthly',
          line_items: lineItems,
          subtotal,
          vat_amount: vatAmount,
          grand_total: grandTotal,
          terms_line_1: quote.terms_line_1 || '',
          terms_line_2: quote.terms_line_2 || '',
          terms_line_3: quote.terms_line_3 || '',
          terms_line_4: quote.terms_line_4 || '',
          terms_line_5: quote.terms_line_5 || '',
          terms_line_6: quote.terms_line_6 || '',
          terms_line_7: quote.terms_line_7 || '',
          terms_line_8: quote.terms_line_8 || '',
          terms_line_9: quote.terms_line_9 || '',
          rate_basis_legend_html: getRateBasisLegendHTML(),
          authorized_signatory: quote.authorized_signatory || '',
        };
      }, payload);
    }

    default:
      return { status: 404, error: `Unknown IPC command: ${commandName}` };
  }
}

/**
 * Full PDF export: entitlement-checked data assembly (generate_quote_pdf,
 * via the existing synchronous guardApiRoute path) followed by the actual
 * async file conversion (renderQuotePdfFile).
 *
 * This is a separate function, not folded into generate_quote_pdf's handler
 * above, because guardApiRoute calls its handlerFn synchronously and does
 * not await it — PDF conversion shells out to a subprocess and is
 * inherently async, so running it inside that handler would make
 * guardApiRoute return an unresolved Promise as `data` instead of a real
 * result. Composing the two steps here keeps every existing case's
 * synchronous contract intact while still getting a real file out.
 *
 * @returns {Promise<{status: number, data?: {filePath: string}, error?: string}>}
 */
export async function exportQuotePdfToFile(payload, sessionToken, secretKey, outputPath) {
  const dataResult = handleIpcCommand('generate_quote_pdf', payload, sessionToken, secretKey);

  if (dataResult.status !== 200) {
    return dataResult; // entitlement/auth/not-found error — surface as-is, don't attempt render
  }

  try {
    const filePath = await renderQuotePdfFile(dataResult.data, outputPath);
    return { status: 200, data: { filePath } };
  } catch (err) {
    return { status: 500, error: err.message };
  }
}