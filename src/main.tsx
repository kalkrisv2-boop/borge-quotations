import React from 'react';
import ReactDOM from 'react-dom/client';
import QuoteBuilder from './QuoteBuilder';

// STITCH FIX: this previously rendered `App`, the old Phase-1.2-era smoke-test
// harness (single QuoteLineItemEditor click-through, no persistence, no IPC calls at
// all -- see App.tsx's own comments). `QuoteBuilder.tsx` is the real R.5 click-through
// component (login -> register_session -> calculate_rate_matrix -> save_quote ->
// fetch_quote -> generate_quote_pdf, all through real Rust commands) and was never
// actually rendered by anything -- it existed in the repo but was unreachable from the
// app's own entry point. `App.tsx`/`QuoteLineItemEditor` are left in the repo as
// reference (the real AssetPicker/DateRangePicker/RateBasisLegend components they
// depend on are being progressively folded into QuoteBuilder -- see QuoteBuilder.tsx's
// own comments for what's integrated vs. still flagged as open).

const rootEl = document.getElementById('root');
if (!rootEl) {
  throw new Error(
    "No <div id=\"root\"> found in index.html — see MIGRATION_STEPS.md step 3."
  );
}

ReactDOM.createRoot(rootEl).render(
  <React.StrictMode>
    <QuoteBuilder />
  </React.StrictMode>
);
