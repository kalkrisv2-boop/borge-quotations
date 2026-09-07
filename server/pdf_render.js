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

// server/pdf_render.js
// Phase 3.2 completion — the missing render step.
//
// Until now, generate_quote_pdf (ipc_handlers.js) built a data object but
// nothing in the app ever turned that + templates/quote_pdf_template.html
// into an actual PDF. scripts/render_pdf_test.mjs proved the template CAN
// render correctly (verified: 2 pages, correct dynamic legend, WeasyPrint),
// but it was a standalone script the app itself never called. This module
// is that missing piece, made real and reusable — both the app
// (ipc_handlers.js) and the test script now call THIS, so there is exactly
// one render code path, not two that can drift apart.

import nunjucks from 'nunjucks';
import path from 'path';
import fs from 'fs';
import { fileURLToPath } from 'url';
import { execFile } from 'child_process';
import { promisify } from 'util';
import { getRateBasisLegendHTML } from '../src/components/RateBasisLegend';

const execFileAsync = promisify(execFile);

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const templatesDir = path.join(__dirname, '..', 'templates');

const env = new nunjucks.Environment(new nunjucks.FileSystemLoader(templatesDir), {
  autoescape: true,
});
env.addFilter('format_currency', (value) => {
  const num = Number(value) || 0;
  return num.toLocaleString('en-US', { minimumFractionDigits: 2, maximumFractionDigits: 2 });
});

/**
 * Render a quote contextPayload (the exact shape generate_quote_pdf in
 * ipc_handlers.js already builds) into the final HTML string.
 *
 * rate_basis_legend_html is generated here if the caller didn't already
 * supply it, so callers can't accidentally skip it and get a blank legend.
 */
export function renderQuotePdfHtml(contextPayload) {
  const fullContext = {
    ...contextPayload,
    rate_basis_legend_html:
      contextPayload.rate_basis_legend_html ?? getRateBasisLegendHTML(),
  };
  return env.render('quote_pdf_template.html', fullContext);
}

/**
 * Convert a rendered quote to an actual PDF file on disk.
 *
 * ARCHITECTURE DECISION (flagging explicitly, not silently picking this):
 * this shells out to a system `weasyprint` executable — the same engine
 * used to independently verify the page-break and legend fixes. That is
 * the ONLY option currently wired up. It requires Python + WeasyPrint to
 * be present wherever this runs.
 *
 * That is a real, open question for whoever handles Phase 5's deployment
 * mechanics (bundle Python+WeasyPrint with the Tauri installer, or switch
 * this function's internals to the Tauri webview's native print-to-PDF —
 * PROJECT_BASELINE.md already describes "browser/Tauri print preview" as
 * an alternate engine, and route-map-v2.docx's own Deferred Items section
 * defers "actual installer signing, update channel setup... deployment
 * steps" out of scope until a dedicated phase). This function's job is to
 * prove the render is correct end-to-end for Phase 3.2 — not to make the
 * distribution decision on its own.
 *
 * Throws with a clear message (does not silently fall back to anything)
 * if `weasyprint` isn't on PATH, so this failure is loud, not swallowed.
 */
export async function renderQuotePdfFile(contextPayload, outputPath) {
  const html = renderQuotePdfHtml(contextPayload);
  // Written inside templatesDir (not os.tmpdir()) on purpose: WeasyPrint
  // resolves relative asset paths (e.g. the logo <img src="borge-logo.jpg">)
  // against the HTML file's own location, not the process cwd. A tmpdir
  // location would silently break the logo again.
  const tmpHtmlPath = path.join(templatesDir, `.quote-render-${Date.now()}.html`);
  fs.writeFileSync(tmpHtmlPath, html, 'utf-8');

  try {
    await execFileAsync('weasyprint', [tmpHtmlPath, outputPath]);
  } catch (err) {
    throw new Error(
      `PDF conversion failed — is 'weasyprint' installed and on PATH? ` +
      `(pip install weasyprint --break-system-packages, or see deployment ` +
      `note in server/pdf_render.js). Original error: ${err.message}`
    );
  } finally {
    fs.unlinkSync(tmpHtmlPath);
  }

  return outputPath;
}
