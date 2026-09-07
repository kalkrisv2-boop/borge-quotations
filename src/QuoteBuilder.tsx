// src/QuoteBuilder.tsx
// Phase R.5 — Full Click-Through & Stitch Point.
//
// Replaces the smoke-test-only App.tsx (which never called tauriInvoke at all — it
// only rendered the AssetPicker/QuoteLineItemEditor click-through and kept saved items
// in local React state, per its own "no persistence layer wired up yet" comment) with
// real calls into the now-wired Rust commands:
//   register_session -> calculate_rate_matrix -> save_quote -> fetch_quote (after a
//   restart) -> generate_quote_pdf.
//
// STITCH-SESSION UPDATE: `DateRangePicker` and `RateBasisLegend` (Phase 3.2) are now
// integrated below -- they were present in the repo but never wired into this
// component. `AssetPicker` (Phase 2.2) is deliberately NOT integrated here, and this
// is a real, separate gap worth stating plainly rather than silently working around a
// second time: `AssetPicker` calls `tauriIpcCall('asset_lookup_sqlite'/'_postgres', ...)`,
// but those commands live in `src-tauri/commands/asset_lookup.rs`, which is not
// declared as a `mod` anywhere `lib.rs` reaches, and `Cargo.toml` has no `sqlx`/
// `async-trait` dependency for it to even compile against. This is the same shape of
// gap Phase R was created to close for auth/quotes/rate_matrix/pdf (business logic
// written and tested in isolation, never actually reachable from the compiled app) --
// it just hasn't been noticed for Phase 2 yet, because Phase 2 predates Phase R and
// was never re-audited under the same lens. This component keeps its own minimal
// inline line-item form (matching `quotes.rs`'s real `QuoteItemInput` shape exactly)
// rather than wire in a picker component that would compile-fail or silently do
// nothing. Porting `asset_lookup` into a real `rusqlite`-backed Rust command (mirroring
// how R.0-R.5 ported everything else off Node/sqlx) is real, scoped follow-up work --
// not attempted in this pass.
//
// Every call below goes through src-shared/tauri-bridge.js's tauriInvoke — the single
// canonical IPC entry point (per PROJECT_BASELINE.md Section 1.3) — not a parallel
// bridge.

import React, { useState } from 'react';
import { isDesktopMode, tauriInvoke } from '../src-shared/tauri-bridge';
import DateRangePicker from './components/DateRangePicker';
import { RateBasisLegend } from './components/RateBasisLegend';

interface LineItemDraft {
  item_description: string;
  make_model: string;
  quantity: number;
  unit_rate: number;
  rate_basis: 'Daily' | 'Weekly' | 'Monthly';
  equipment_spec: string;
}

interface RateMatrixResult {
  rate_basis: 'Daily' | 'Weekly' | 'Monthly';
  unit_rate: number;
  tier: { rate_basis: string; min_days: number; max_days: number | null };
  quantity: number;
  line_total: number;
}

const emptyDraft = (): LineItemDraft => ({
  item_description: '',
  make_model: '',
  quantity: 1,
  unit_rate: 0,
  rate_basis: 'Monthly',
  equipment_spec: '',
});

/**
 * IpcResponse shape mirrors src-tauri/src/lib.rs's `IpcResponse` struct exactly
 * (status, message, data) — every handle_guarded_ipc call returns this shape.
 */
interface IpcResponse<T = unknown> {
  status: number;
  message: string;
  data: T | null;
}

function QuoteBuilder() {
  // R.5-FIX: `register_session` no longer trusts a caller-supplied token/tenant --
  // it independently verifies a real HMAC-signed token via `auth::verify_session_token`
  // (see lib.rs's `HmacSecret` doc comment for why the old smoke-test flow was a real,
  // exploitable gap, not just a convenience shortcut). That token now has to come from
  // a real `login` call. SECURITY: no real credential is pre-filled here (this file is
  // committed, public source). Enter whatever you configured as SEED_USER_EMAIL/
  // SEED_USER_PASSWORD_HASH in your local .env -- see .env.example -- or leave blank
  // and no seeded user will exist to log in as (see seed.rs's safe-by-default behavior).
  const [email, setEmail] = useState('');
  const [password, setPassword] = useState('');
  const [sessionToken, setSessionToken] = useState<string | null>(null);
  const offerRef = 'QN-SAMPLE/001/2026';
  const revSuffix = 'Rev.01';

  const [loggedIn, setLoggedIn] = useState(false);
  const [sessionRegistered, setSessionRegistered] = useState(false);
  const [durationDays, setDurationDays] = useState(30);
  const [rateResult, setRateResult] = useState<RateMatrixResult | null>(null);
  const [lineItems, setLineItems] = useState<LineItemDraft[]>([]);
  const [draft, setDraft] = useState<LineItemDraft>(emptyDraft());
  const [status, setStatus] = useState<string>('');
  const [pdfPath, setPdfPath] = useState<string | null>(null);

  const runLogin = async () => {
    try {
      const res = await tauriInvoke<IpcResponse<{ session_token: string }>>('login', {
        email,
        password,
      });
      if (res.status === 200 && res.data?.session_token) {
        setSessionToken(res.data.session_token);
        setLoggedIn(true);
        setStatus(`login: ${res.status} ${res.message}`);
      } else {
        setLoggedIn(false);
        setStatus(`login: ${res.status} ${res.message}`);
      }
    } catch (err) {
      setLoggedIn(false);
      setStatus(`login failed: ${(err as Error).message}`);
    }
  };

  const runRegisterSession = async () => {
    if (!sessionToken) {
      setStatus('register_session: skipped -- log in first to obtain a real session token');
      return;
    }
    try {
      const res = await tauriInvoke<IpcResponse>('register_session', {
        session_token: sessionToken,
      });
      setSessionRegistered(res.status === 200);
      setStatus(`register_session: ${res.status} ${res.message}`);
    } catch (err) {
      setStatus(`register_session failed: ${(err as Error).message}`);
    }
  };

  const runCalculateRateMatrix = async () => {
    try {
      const res = await tauriInvoke<IpcResponse<RateMatrixResult>>(
        'handle_guarded_ipc',
        {
          command_name: 'calculate_rate_matrix',
          session_token: sessionToken,
          payload: {
            duration_days: durationDays,
            default_daily_rate: draft.unit_rate || 100,
            default_weekly_rate: (draft.unit_rate || 100) * 5,
            default_monthly_rate: (draft.unit_rate || 100) * 15,
            quantity: draft.quantity,
          },
        }
      );
      if (res.status === 200 && res.data) {
        setRateResult(res.data);
        setDraft((d) => ({
          ...d,
          rate_basis: res.data!.rate_basis,
          unit_rate: res.data!.unit_rate,
        }));
      }
      setStatus(`calculate_rate_matrix: ${res.status} ${res.message}`);
    } catch (err) {
      setStatus(`calculate_rate_matrix failed: ${(err as Error).message}`);
    }
  };

  const addLineItem = () => {
    if (!draft.item_description.trim()) return;
    setLineItems((prev) => [...prev, draft]);
    setDraft(emptyDraft());
  };

  const runSaveQuote = async () => {
    try {
      const res = await tauriInvoke<IpcResponse>('handle_guarded_ipc', {
        command_name: 'save_quote',
        session_token: sessionToken,
        payload: {
          quote: {
            offer_ref: offerRef,
            rev_suffix: revSuffix,
            quote_date: new Date().toISOString().slice(0, 10),
            // SECURITY: genericized -- this used to be real customer/contact/
            // salesperson data (including a real phone number) hardcoded in
            // committed, now-public source. Replace with real values via the UI
            // fields once a proper quote form exists; these are placeholders only.
            customer_name: 'Sample Customer LLC',
            customer_po_box: '',
            customer_city: 'Example City, UAE',
            contact_person: 'Sample Contact',
            salesperson_name: 'Sample Salesperson',
            subject_text: 'Sample Equipment Rental',
            terms_conditions: 'Sample terms and conditions text.',
            rate_basis_text: draft.rate_basis || 'Monthly',
            vat_rate: 5.0,
            status: 'Draft',
            line_items: lineItems.map((li) => ({
              item_description: li.item_description,
              make_model: li.make_model || null,
              quantity: li.quantity,
              unit_rate: li.unit_rate,
              rate_basis: li.rate_basis,
              line_total: li.unit_rate * li.quantity,
              equipment_spec: li.equipment_spec || null,
            })),
          },
        },
      });
      setStatus(`save_quote: ${res.status} ${res.message}`);
    } catch (err) {
      setStatus(`save_quote failed: ${(err as Error).message}`);
    }
  };

  const runFetchQuote = async () => {
    try {
      const res = await tauriInvoke<IpcResponse>('handle_guarded_ipc', {
        command_name: 'fetch_quote',
        session_token: sessionToken,
        payload: { offer_ref: offerRef, rev_suffix: revSuffix },
      });
      setStatus(`fetch_quote: ${res.status} ${res.message}`);
    } catch (err) {
      setStatus(`fetch_quote failed: ${(err as Error).message}`);
    }
  };

  const runGeneratePdf = async () => {
    try {
      const res = await tauriInvoke<IpcResponse<{ pdf_path: string }>>(
        'handle_guarded_ipc',
        {
          command_name: 'generate_quote_pdf',
          session_token: sessionToken,
          payload: { offer_ref: offerRef, rev_suffix: revSuffix },
        }
      );
      if (res.status === 200 && res.data) {
        setPdfPath(res.data.pdf_path);
      }
      setStatus(`generate_quote_pdf: ${res.status} ${res.message}`);
    } catch (err) {
      setStatus(`generate_quote_pdf failed: ${(err as Error).message}`);
    }
  };

  return (
    <div style={{ padding: '24px', fontFamily: 'system-ui, sans-serif' }}>
      <header style={{ marginBottom: '16px' }}>
        <h2 style={{ margin: 0 }}>Borge Equipment Rental — R.5 Click-Through</h2>
        {!isDesktopMode() && (
          <div
            style={{
              background: '#fff3cd',
              border: '2px solid #d97706',
              borderRadius: '6px',
              padding: '12px 16px',
              marginBottom: '16px',
              fontSize: '14px',
              fontWeight: 600,
              color: '#7c2d12',
            }}
          >
            ⚠️ You are viewing this in a plain browser tab, not the real desktop app.
            <br />
            Every button below WILL fail — there is no Rust backend to talk to here.
            <br />
            Run <code>npm run dev</code> (which should launch a separate native window
            via <code>tauri dev</code>, not just open this page in your browser) and use
            that window instead.
          </div>
        )}
        <p style={{ color: '#5a6b75', fontSize: '13px' }}>
          Runtime:{' '}
          {isDesktopMode()
            ? 'Desktop Shell (Tauri v2 native bridge active) ✅'
            : 'Web target (browser fallback active — IPC calls below will fail here by design) ❌'}
        </p>
      </header>

      <ol style={{ fontSize: '13px', lineHeight: 1.8 }}>
        <li>
          Email:{' '}
          <input
            type="email"
            value={email}
            onChange={(e) => setEmail(e.target.value)}
            style={{ width: '220px' }}
          />{' '}
          Password:{' '}
          <input
            type="password"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            style={{ width: '140px' }}
          />{' '}
          <button onClick={runLogin}>1. Log in</button> {loggedIn ? '✅' : ''}
        </li>
        <li>
          <button onClick={runRegisterSession} disabled={!loggedIn}>
            2. Register session
          </button>{' '}
          {sessionRegistered ? '✅' : ''}
        </li>
        <li>
          {/* STITCH FIX: was a plain number input; the real DateRangePicker (Phase
              3.2) existed in the repo but was never wired into the R.5 click-through
              flow. Matches the literal Completion Check's wording ("set a rental date
              range") more directly than a raw day-count field. */}
          <DateRangePicker onDurationChange={setDurationDays} />
          Resolved duration: {durationDays} day(s){' '}
          <button onClick={runCalculateRateMatrix}>3. Calculate rate tier</button>
          {rateResult && (
            <span>
              {' '}
              → {rateResult.rate_basis} @ {rateResult.unit_rate} AED (line total{' '}
              {rateResult.line_total} AED)
            </span>
          )}
        </li>
      </ol>

      {/* STITCH FIX: real RateBasisLegend (Phase 3.2), previously only ever ported
          statically into the Rust PDF context (lib.rs's RATE_BASIS_LEGEND_HTML
          constant) -- never rendered in the app UI itself. Shown here so the on-screen
          tier boundaries and the exported PDF's legend are visibly the same rule,
          sourced from the same rate-matrix.ts engine this component reads from. */}
      <RateBasisLegend />

      <h4>Line item</h4>
      <div style={{ display: 'flex', gap: '8px', flexWrap: 'wrap', marginBottom: '8px' }}>
        <input
          placeholder="Item description"
          value={draft.item_description}
          onChange={(e) => setDraft({ ...draft, item_description: e.target.value })}
        />
        <input
          placeholder="Make/Model"
          value={draft.make_model}
          onChange={(e) => setDraft({ ...draft, make_model: e.target.value })}
        />
        <input
          type="number"
          placeholder="Qty"
          value={draft.quantity}
          onChange={(e) => setDraft({ ...draft, quantity: Number(e.target.value) })}
          style={{ width: '60px' }}
        />
        <button onClick={addLineItem}>+ Add line item</button>
      </div>

      {lineItems.length > 0 && (
        <ul style={{ fontSize: '13px' }}>
          {lineItems.map((li, i) => (
            <li key={i}>
              {li.item_description} — {li.make_model} × {li.quantity} @ {li.unit_rate}{' '}
              AED ({li.rate_basis})
            </li>
          ))}
        </ul>
      )}

      <div style={{ marginTop: '16px', display: 'flex', gap: '8px' }}>
        <button onClick={runSaveQuote}>4. Save quote</button>
        <button onClick={runFetchQuote}>
          5. Fetch quote (re-run after an app restart to verify persistence)
        </button>
        <button onClick={runGeneratePdf}>6. Generate PDF</button>
      </div>

      {pdfPath && (
        <p style={{ marginTop: '12px' }}>
          PDF written to: <code>{pdfPath}</code>
        </p>
      )}

      <pre
        style={{
          marginTop: '16px',
          background: '#f4f6f8',
          padding: '12px',
          fontSize: '12px',
        }}
      >
        {status}
      </pre>
    </div>
  );
}

export default QuoteBuilder;
