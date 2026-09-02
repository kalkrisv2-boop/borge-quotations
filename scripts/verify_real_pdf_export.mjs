// scripts/verify_real_pdf_export.mjs
// Architect verification: exercises the ACTUAL app code path — auth,
// tenant entitlement enforcement, save_quote, then the new
// exportQuotePdfToFile — rather than re-testing the render logic in
// isolation. If this script's PDF comes out wrong, the real app is wrong.

import { generateSessionToken } from '../server/auth.js';
import { seedTenantEntitlements } from '../server/entitlements.js';
import { handleIpcCommand, exportQuotePdfToFile } from '../server/ipc_handlers.js';
import path from 'path';
import { fileURLToPath } from 'url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const secretKey = 'test-secret-key-for-verification-only';

const tenantId = 'tenant-borge-uae';
const user = { id: 'user-1', tenant_id: tenantId, role: 'staff' };

// 1. Real entitlement seeding (same path admin_seed_entitlements uses)
seedTenantEntitlements(tenantId, [{ module_key: 'quotes_core', is_enabled: true }]);

// 2. Real session token
const sessionToken = generateSessionToken(user, secretKey);

// 3. Real save_quote call — this is what a genuine quote-builder flow does
//    before export, not fabricated data injected straight into export.
const quotePayload = {
  quote: {
    offer_ref: 'QN-EH/211/2026-Rev.02',
    customer_name: 'Borge',
    po_box: '',
    quote_date: '02-09-2026',
    location: 'Abu Dhabi, UAE',
    page_count: '1/1',
    contact_person: 'Rajesh',
    sales_person: 'Sreejith-0565465803',
    equipment_category: 'SF6 Gas Test Equipment & Heavy Machinery',
    rate_basis: 'Monthly',
    line_items: [
      { item_order: 1, item_description: 'SF6 Economic Gas Service Cart with Storage Tank (600Ltr)', equipment_spec: 'Connecting Hose 10mtr, Connecting Hose 5mtr, Filling Adaptor Box', make_model: 'DILO/L057R2BEA0B0', quantity: 1, unit_rate: 35000 },
      { item_order: 2, item_description: 'SF6 Gas Leak Detector', equipment_spec: '', make_model: 'Dilo / 3-033-R002', quantity: 1, unit_rate: 2000 },
      { item_order: 3, item_description: 'SF6 Gas Multi Analyzer', equipment_spec: '', make_model: 'Dilo /3-038R-R303', quantity: 1, unit_rate: 14000 },
      { item_order: 4, item_description: 'Cable Test Plug Size-2', equipment_spec: '', make_model: 'Pfisterer /Size II', quantity: 1, unit_rate: 3250 },
      { item_order: 5, item_description: 'Cable Test Plug Size-2', equipment_spec: '', make_model: 'Pfisterer /SizeIII', quantity: 1, unit_rate: 3250 },
      { item_order: 6, item_description: 'Voltage Test Plug Size-2', equipment_spec: '', make_model: 'Pfisterer', quantity: 1, unit_rate: 4750 },
      { item_order: 7, item_description: 'Voltage Test Plug Size-3', equipment_spec: '', make_model: 'Pfisterer', quantity: 1, unit_rate: 4750 },
      { item_order: 8, item_description: 'CAT 320D Excavator / Dredger', equipment_spec: 'Includes standard bucket, hydraulic thumb', make_model: 'CAT 320D', quantity: 1, unit_rate: 18500 },
      { item_order: 9, item_description: 'Volvo EC480E Excavator', equipment_spec: 'Long-reach boom configuration', make_model: 'Volvo EC480E', quantity: 1, unit_rate: 22000 },
      { item_order: 10, item_description: 'Mobile Generator Set 150kVA', equipment_spec: 'Diesel, sound-attenuated enclosure', make_model: 'Cummins C150D5', quantity: 2, unit_rate: 6800 },
      { item_order: 11, item_description: 'Liebherr Mobile Crane 50T', equipment_spec: 'Operator not included per Terms & Conditions', make_model: 'Liebherr LTM 1050', quantity: 1, unit_rate: 27500 },
    ],
    terms_line_1: 'Valid calibration certificate shall be provided by Borge Equipment Rental.',
    terms_line_2: 'Mobilization and Demobilization is not in our scope.',
    terms_line_3: 'The charges mentioned above is only for equipment rental and does not include training/engineering service.',
    terms_line_4: "Any damage of equipment's or missing accessories will be chargeable.",
    terms_line_5: 'Payment terms: 100% equipment return time Cash/CDC.',
    terms_line_6: 'Formal LPO / email confirmation specifying your requirement schedule only shall be considered for booking the equipment.',
    terms_line_7: 'The availability of the equipment should be checked at least 1 week before your requirement schedule.',
    terms_line_8: 'The above price does not include VAT. VAT charges shall be applicable over the total invoice value as per UAE law.',
    terms_line_9: 'Above offer validity for 30 days.',
    authorized_signatory: 'Sreejith',
  },
};

const saveResult = handleIpcCommand('save_quote', quotePayload, sessionToken, secretKey);
console.log('save_quote result:', saveResult.status, saveResult.data || saveResult.error);
if (saveResult.status !== 200) process.exit(1);

// 4. TENANT ISOLATION CHECK: a different tenant, with no entitlement seeded,
//    must be rejected — proves the guard is actually enforced on this path,
//    not just on the ones tested in earlier sessions.
const otherTenantToken = generateSessionToken({ id: 'user-2', tenant_id: 'tenant-other', role: 'staff' }, secretKey);
const isolationCheck = handleIpcCommand('fetch_quote', { offer_ref: 'QN-EH/211/2026-Rev.02' }, otherTenantToken, secretKey);
console.log('Cross-tenant isolation check (expect 403):', isolationCheck.status, isolationCheck.error);
if (isolationCheck.status !== 403) {
  console.error('FAIL: cross-tenant access was not rejected!');
  process.exit(1);
}

// 5. The actual thing being verified: real PDF export through the real app path.
const outputPath = path.join(__dirname, '..', 'templates', 'real_app_export_test.pdf');
const exportResult = await exportQuotePdfToFile(
  { offer_ref: 'QN-EH/211/2026-Rev.02' },
  sessionToken,
  secretKey,
  outputPath
);
console.log('exportQuotePdfToFile result:', exportResult.status, exportResult.data || exportResult.error);
if (exportResult.status !== 200) process.exit(1);
console.log('PDF written to:', exportResult.data.filePath);
