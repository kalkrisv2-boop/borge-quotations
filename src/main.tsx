import React from 'react';
import ReactDOM from 'react-dom/client';
import App from './App';

// Replaces the old vanilla main.js entry point. index.html's <script> tag
// must point here instead (see MIGRATION_STEPS.md, step 3) — main.js and
// its DOMContentLoaded/isTauri() status-line logic are now dead code and
// should be deleted once this is wired in, not left alongside it.

const rootEl = document.getElementById('root');
if (!rootEl) {
  throw new Error(
    "No <div id=\"root\"> found in index.html — see MIGRATION_STEPS.md step 3."
  );
}

ReactDOM.createRoot(rootEl).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>
);
