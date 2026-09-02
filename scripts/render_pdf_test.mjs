// scripts/render_pdf_test.mjs
// Manual end-to-end render test for Phase 1.4's Completion Check:
// "Real-world quote inputs render accurately without overlapping page
// breaks or table truncations."
//
// This exists because generate_quote_pdf (server/ipc_handlers.js) builds a
// contextPayload object but nothing in the app actually renders it against
// templates/quote_pdf_template.html — there is no templating engine wired
// in anywhere. This script is a standalone stand-in for that missing wiring,
// using nunjucks (matches the template's {% for %}/{{ x | filter }} syntax)
// so we can produce one real HTML file and inspect it for the actual
// Completion Check, not just confirm the data shape is correct.

import nunjucks from 'nunjucks';
import { getRateBasisLegendHTML } from '../src/components/RateBasisLegend.tsx';
import fs from 'fs';
import path from 'path';
import { fileURLToPath } from 'url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const templatesDir = path.join(__dirname, '..', 'templates');

const env = new nunjucks.Environment(new nunjucks.FileSystemLoader(templatesDir), {
  autoescape: true,
});
env.addFilter('format_currency', (value) => {
  const num = Number(value) || 0;
  return num.toLocaleString('en-US', { minimumFractionDigits: 2, maximumFractionDigits: 2 });
});

// Real-world-shaped data: 11 line items (more than the 7-item Servepower
// sample) specifically to stress-test whether the table actually breaks
// across pages without truncating rows or splitting a row mid-page —
// the exact thing Phase 1.4's Completion Check calls out.
const lineItems = [
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
];

const subtotal = lineItems.reduce((sum, i) => sum + i.unit_rate * i.quantity, 0);
const vatAmount = subtotal * 0.05;
const grandTotal = subtotal + vatAmount;

const contextPayload = {
  customer_name: 'Borge',
  offer_reference: 'QN-EH/211/2026-Rev.02',
  po_box: '',
  quote_date: '02-09-2026',
  location: 'Abu Dhabi, UAE',
  page_count: '1/2',
  contact_person: 'Rajesh',
  sales_person: 'Sreejith-0565465803',
  equipment_category: 'SF6 Gas Test Equipment & Heavy Machinery',
  rate_basis: 'Monthly',
  line_items: lineItems,
  subtotal,
  vat_amount: vatAmount,
  grand_total: grandTotal,
  terms_line_1: 'Valid calibration certificate shall be provided by Borge Equipment Rental.',
  terms_line_2: 'Mobilization and Demobilization is not in our scope.',
  terms_line_3: 'The charges mentioned above is only for equipment rental and does not include training/engineering service.',
  terms_line_4: "Any damage of equipment's or missing accessories will be chargeable.",
  terms_line_5: 'Payment terms: 100% equipment return time Cash/CDC.',
  terms_line_6: 'Formal LPO / email confirmation specifying your requirement schedule only shall be considered for booking the equipment.',
  terms_line_7: 'The availability of the equipment should be checked at least 1 week before your requirement schedule.',
  terms_line_8: 'The above price does not include VAT. VAT charges shall be applicable over the total invoice value as per UAE law.',
  terms_line_9: 'Above offer validity for 30 days.',
  rate_basis_legend_html: getRateBasisLegendHTML(),
  authorized_signatory: 'Sreejith',
};

const html = env.render('quote_pdf_template.html', contextPayload);
const outPath = path.join(templatesDir, 'rendered_quote_test.html');
fs.writeFileSync(outPath, html, 'utf-8');
console.log(`Rendered HTML: ${outPath}`);
console.log(`Line items: ${lineItems.length}, Subtotal: ${subtotal}, Grand Total: ${grandTotal}`);
