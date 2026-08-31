import React, { useState } from 'react';
import { isDesktopMode } from '../src-shared/tauri-bridge';
import { QuoteLineItemEditor, QuoteLineItem } from './components/QuoteLineItemEditor';

// AUDIT NOTE: the old main.js called isTauri() from tauri-bridge.js — but no
// version of that file (original or Session 9's corrected version) exports a
// function by that name; it exports isDesktopMode(). If that status line was
// ever rendering correctly, it was silently failing and falling back to
// static HTML, not actually running the check. Fixed here to call the real
// export.

function App() {
  const [savedItems, setSavedItems] = useState<QuoteLineItem[]>([]);
  const [isEditing, setIsEditing] = useState(true);

  const handleSave = (item: QuoteLineItem) => {
    // Minimal placeholder — no persistence layer wired up yet. This exists
    // to let you click through AssetPicker -> field population -> save and
    // SEE the result on screen, which is the immediate goal. It is not a
    // real quote-builder yet: no line-item list, no IPC/API persistence,
    // no multi-item quote assembly. Treat as a smoke-test harness, not a
    // finished feature.
    setSavedItems((prev) => [...prev, item]);
    setIsEditing(false);
  };

  const handleCancel = () => {
    setIsEditing(false);
  };

  return (
    <div style={{ padding: '24px', fontFamily: 'system-ui, sans-serif' }}>
      <header style={{ display: 'flex', alignItems: 'center', gap: '8px', marginBottom: '24px' }}>
        <h2 style={{ margin: 0 }}>Borge Equipment Rental &amp; Quotation System</h2>
      </header>

      <p style={{ color: '#5a6b75', fontSize: '13px' }}>
        Runtime detected: {isDesktopMode() ? 'Desktop Shell (Tauri v2 native bridge active)' : 'Web Application target (browser fallback active)'}
      </p>

      <h3>Quote Line Item — Click-Through Test</h3>

      {isEditing ? (
        // TODO: tenantId/quoteId are hardcoded placeholders for this smoke
        // test. Replace with real values once auth/session context and a
        // real quote-creation flow exist upstream of this screen.
        <QuoteLineItemEditor
          tenantId="tenant-A"
          quoteId="quote-smoke-test"
          itemOrder={savedItems.length + 1}
          onSave={handleSave}
          onCancel={handleCancel}
        />
      ) : (
        <button onClick={() => setIsEditing(true)}>+ Add another line item</button>
      )}

      {savedItems.length > 0 && (
        <div style={{ marginTop: '24px' }}>
          <h4>Saved line items this session (not persisted anywhere yet):</h4>
          <pre style={{ background: '#f4f6f8', padding: '12px', fontSize: '12px' }}>
            {JSON.stringify(savedItems, null, 2)}
          </pre>
        </div>
      )}
    </div>
  );
}

export default App;
