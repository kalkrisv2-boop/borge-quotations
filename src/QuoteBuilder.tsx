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
// Phase 5.2: the real native file-dialog plugin attach_lpo's file-path guardrail
// depends on (route-map-v2.docx Section 1.1) -- see lib.rs's `.plugin()` registration
// and capabilities/default.json's "dialog:allow-open" permission, both added alongside
// this import so the three pieces (JS import, Rust registration, capability grant)
// don't end up split across commits the way plugin-dialog previously was (present in
// package.json only, wired nowhere).
import { open as openFileDialog } from '@tauri-apps/plugin-dialog';

// Phase 5.1: mirrors quotes.rs::LOCKED_STATUSES exactly, for the UI's "locked" badge
// only -- the real enforcement is server-side in quotes::save_quote; this constant
// controls nothing except a visual hint, so if it drifts out of sync the worst case is
// a stale badge, not a security gap. Flagged rather than left silently duplicated,
// matching this project's established practice for CANONICAL_MODULE_KEYS-shaped risks.
// Phase 5.2 correction: "Approved" was this project's own guess in Phase 5.1, made
// before the real lifecycle was known -- route-map-v2.docx Phase 5.2 gives the actual,
// authoritative lifecycle (no "Approved" state exists in it). Mirrors
// quotes.rs::LOCKED_STATUSES / STATUS_LIFECYCLE exactly now. Still just a UI hint (see
// original comment below) -- real enforcement is server-side.
const quotes_rs_locked_statuses = ['Issued', 'LPO Confirmed', 'Job Booked'];
const quotes_rs_status_lifecycle = ['Draft', 'Issued', 'LPO Confirmed', 'Job Booked'];

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

// Phase 4.2 — mirrors src-tauri/src/compliance.rs's ComplianceTermRecord/TaxRuleRecord
// exactly (field names, not a re-derived shape).
interface ComplianceTermRecord {
  id: string;
  category: string;
  title: string;
  body_text: string;
  is_default: boolean;
  display_order: number;
  is_active: boolean;
}

interface TaxRuleRecord {
  id: string;
  region_label: string;
  rate: number;
  is_default: boolean;
  is_active: boolean;
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
  // Phase 5.1: was a hardcoded constant -- had to become real state once a quote could
  // branch into a new rev_suffix (Rev.01 -> Rev.02) and the UI needed to actually track
  // which revision is currently being viewed/edited.
  const [revSuffix, setRevSuffix] = useState('Rev.01');
  // Phase 5.2 correction: removed quoteStatusToSave (Draft/Approved selector) -- save_quote
  // no longer accepts anything but Draft. Lifecycle state now lives in currentStatus,
  // advanced only via runAdvanceStatus.
  // Populated from save_quote/fetch_quote responses -- what branch_new_revision and the
  // "currently locked?" UI hint below actually act on.
  const [currentQuoteId, setCurrentQuoteId] = useState<string | null>(null);
  const [currentStatus, setCurrentStatus] = useState<string | null>(null);
  const [newRevSuffix, setNewRevSuffix] = useState('Rev.02');
  // Phase 5.2 — LPO tracking modal state.
  const [lpoNumber, setLpoNumber] = useState('');
  const [lpoFilePath, setLpoFilePath] = useState<string | null>(null);

  const [loggedIn, setLoggedIn] = useState(false);
  const [sessionRegistered, setSessionRegistered] = useState(false);
  const [durationDays, setDurationDays] = useState(30);
  const [rateResult, setRateResult] = useState<RateMatrixResult | null>(null);
  const [lineItems, setLineItems] = useState<LineItemDraft[]>([]);
  const [draft, setDraft] = useState<LineItemDraft>(emptyDraft());
  const [status, setStatus] = useState<string>('');
  const [pdfPath, setPdfPath] = useState<string | null>(null);

  // Phase 4.2 — Terms Library & Regional VAT Schema, selected per quote.
  const [complianceTerms, setComplianceTerms] = useState<ComplianceTermRecord[]>([]);
  const [selectedTermIds, setSelectedTermIds] = useState<string[]>([]);
  const [taxRules, setTaxRules] = useState<TaxRuleRecord[]>([]);
  const [selectedTaxRuleId, setSelectedTaxRuleId] = useState<string>('');
  const [newTermTitle, setNewTermTitle] = useState('');
  const [newTermCategory, setNewTermCategory] = useState('General');
  const [newTermBody, setNewTermBody] = useState('');
  const [newRuleLabel, setNewRuleLabel] = useState('');
  const [newRuleRate, setNewRuleRate] = useState(5);

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
      if (res.status === 200) {
        // Load the compliance/tax library as soon as the session is usable -- both are
        // gated behind the "compliance_terms" entitlement, same as every other call
        // below, so a tenant without that module simply sees empty lists.
        void loadComplianceLibrary();
      }
    } catch (err) {
      setStatus(`register_session failed: ${(err as Error).message}`);
    }
  };

  // Phase 4.2 -- list_compliance_terms / list_tax_rules. Loaded together since both
  // populate the same "compliance library" section of the form.
  const loadComplianceLibrary = async () => {
    try {
      const [termsRes, rulesRes] = await Promise.all([
        tauriInvoke<IpcResponse<ComplianceTermRecord[]>>('handle_guarded_ipc', {
          command_name: 'list_compliance_terms',
          session_token: sessionToken,
          payload: { active_only: true },
        }),
        tauriInvoke<IpcResponse<TaxRuleRecord[]>>('handle_guarded_ipc', {
          command_name: 'list_tax_rules',
          session_token: sessionToken,
          payload: { active_only: true },
        }),
      ]);
      if (termsRes.status === 200 && termsRes.data) {
        setComplianceTerms(termsRes.data);
        // Pre-select whichever terms the tenant marked is_default, so a fresh quote
        // starts with the standard clause set instead of empty -- matches
        // compliance_terms.is_default's stated purpose.
        setSelectedTermIds(termsRes.data.filter((t) => t.is_default).map((t) => t.id));
      }
      if (rulesRes.status === 200 && rulesRes.data) {
        setTaxRules(rulesRes.data);
        const defaultRule = rulesRes.data.find((r) => r.is_default);
        if (defaultRule) setSelectedTaxRuleId(defaultRule.id);
      }
      setStatus(
        `compliance library loaded: ${termsRes.data?.length ?? 0} term(s), ${
          rulesRes.data?.length ?? 0
        } tax rule(s)`
      );
    } catch (err) {
      setStatus(`compliance library load failed: ${(err as Error).message}`);
    }
  };

  const toggleTermSelected = (id: string) => {
    setSelectedTermIds((prev) =>
      prev.includes(id) ? prev.filter((t) => t !== id) : [...prev, id]
    );
  };

  const runCreateComplianceTerm = async () => {
    if (!newTermTitle.trim() || !newTermBody.trim()) {
      setStatus('save_compliance_term: title and body text are both required');
      return;
    }
    try {
      const res = await tauriInvoke<IpcResponse<{ id: string }>>('handle_guarded_ipc', {
        command_name: 'save_compliance_term',
        session_token: sessionToken,
        payload: {
          term: {
            category: newTermCategory,
            title: newTermTitle,
            body_text: newTermBody,
            is_default: false,
            display_order: 0,
          },
        },
      });
      setStatus(`save_compliance_term: ${res.status} ${res.message}`);
      if (res.status === 200) {
        setNewTermTitle('');
        setNewTermBody('');
        await loadComplianceLibrary();
      }
    } catch (err) {
      setStatus(`save_compliance_term failed: ${(err as Error).message}`);
    }
  };

  const runCreateTaxRule = async () => {
    if (!newRuleLabel.trim()) {
      setStatus('save_tax_rule: region_label is required');
      return;
    }
    try {
      const res = await tauriInvoke<IpcResponse<{ id: string }>>('handle_guarded_ipc', {
        command_name: 'save_tax_rule',
        session_token: sessionToken,
        payload: {
          rule: { region_label: newRuleLabel, rate: newRuleRate, is_default: taxRules.length === 0 },
        },
      });
      setStatus(`save_tax_rule: ${res.status} ${res.message}`);
      if (res.status === 200) {
        setNewRuleLabel('');
        await loadComplianceLibrary();
      }
    } catch (err) {
      setStatus(`save_tax_rule failed: ${(err as Error).message}`);
    }
  };

  // FIX: this used to feed draft.unit_rate (whatever the user typed as their real
  // rate) into the backend as "default_daily_rate", then multiply it by hardcoded
  // 5x/15x to synthesize Weekly/Monthly, then overwrite the draft's rate field with
  // whichever of those three synthetic numbers matched the resolved tier. That meant:
  // typing a real weekly rate and clicking this button would silently replace it with
  // (that number x 5), mislabeled as "Weekly". There was no safe order to use it in.
  //
  // What duration alone actually determines is WHICH TIER applies (Daily/Weekly/
  // Monthly) -- it says nothing about what the rate for that tier should be. This
  // button now only resolves and displays the tier. It sends fixed placeholder
  // numbers (never draft.unit_rate) purely so the existing backend command has
  // something to compute with, and reads back only `tier`/`rate_basis` -- the
  // synthetic `unit_rate` in the response is discarded, not written into the draft.
  // The Rate field is always the user's own typed number now, for either sequence:
  // set dates -> see the tier -> type the real rate for that tier -> add the item, or
  // type the rate first -> check the tier afterward -- neither order overwrites
  // anything.
  const runCalculateRateMatrix = async () => {
    try {
      const res = await tauriInvoke<IpcResponse<RateMatrixResult>>(
        'handle_guarded_ipc',
        {
          command_name: 'calculate_rate_matrix',
          session_token: sessionToken,
          payload: {
            duration_days: durationDays,
            default_daily_rate: 1,
            default_weekly_rate: 1,
            default_monthly_rate: 1,
            quantity: 1,
          },
        }
      );
      if (res.status === 200 && res.data) {
        setRateResult(res.data);
        // Pre-fill the rate_basis dropdown to match the resolved tier as a
        // convenience only -- the numeric rate is left exactly as the user typed it.
        setDraft((d) => ({ ...d, rate_basis: res.data!.rate_basis }));
      }
      setStatus(`calculate_rate_matrix: ${res.status} ${res.message}`);
    } catch (err) {
      setStatus(`calculate_rate_matrix failed: ${(err as Error).message}`);
    }
  };

  // editingIndex tracks whether "Add line item" is adding a new row or committing an
  // edit back into an existing one. null = adding; a number = replacing that index.
  const [editingIndex, setEditingIndex] = useState<number | null>(null);

  const addLineItem = () => {
    if (!draft.item_description.trim()) return;
    if (editingIndex !== null) {
      setLineItems((prev) => prev.map((li, i) => (i === editingIndex ? draft : li)));
      setEditingIndex(null);
    } else {
      setLineItems((prev) => [...prev, draft]);
    }
    setDraft(emptyDraft());
  };

  const editLineItem = (index: number) => {
    setDraft(lineItems[index]);
    setEditingIndex(index);
  };

  const cancelEdit = () => {
    setDraft(emptyDraft());
    setEditingIndex(null);
  };

  const removeLineItem = (index: number) => {
    setLineItems((prev) => prev.filter((_, i) => i !== index));
    // If the row being edited gets removed out from under the form, drop back to
    // "adding" mode instead of silently overwriting a different row on next Add.
    if (editingIndex === index) {
      setDraft(emptyDraft());
      setEditingIndex(null);
    } else if (editingIndex !== null && index < editingIndex) {
      setEditingIndex(editingIndex - 1);
    }
  };

  const runSaveQuote = async () => {
    try {
      const res = await tauriInvoke<IpcResponse<{ quote_id: string }>>('handle_guarded_ipc', {
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
            // Phase 4.2: which tax_rules row this quote's VAT should be resolved
            // against. Empty selection -> null -> build_pdf_context falls back to the
            // tenant default (or the legacy flat 5%) exactly as before Phase 4.2.
            tax_rule_id: selectedTaxRuleId || null,
            // Phase 5.2 correction: save_quote now only ever accepts "Draft" (see
            // quotes.rs's own doc comment on why) -- the quoteStatusToSave selector
            // this used to read from is gone; lifecycle progression happens
            // exclusively through runAdvanceStatus below.
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
      if (res.status === 200 && res.data?.quote_id) {
        setCurrentQuoteId(res.data.quote_id);
        setCurrentStatus('Draft');
      }

      // Phase 4.2: attach_terms_to_quote is a separate call (compliance.rs's own
      // wholesale-replace function, not folded into save_quote's transaction -- see
      // compliance.rs module doc) so it needs the real quote_id save_quote just
      // returned. Only attempted after a real save succeeded, and only reported as a
      // save_quote failure in the status line if it fails -- the quote row itself did
      // save correctly either way, so this is deliberately a distinct, later status
      // message rather than silently folded into the line above.
      if (res.status === 200 && res.data?.quote_id) {
        const attachRes = await tauriInvoke<IpcResponse>('handle_guarded_ipc', {
          command_name: 'attach_terms_to_quote',
          session_token: sessionToken,
          payload: { quote_id: res.data.quote_id, term_ids: selectedTermIds },
        });
        setStatus(
          `save_quote: ${res.status} ${res.message} | attach_terms_to_quote: ${attachRes.status} ${attachRes.message}`
        );
      }
    } catch (err) {
      setStatus(`save_quote failed: ${(err as Error).message}`);
    }
  };

  const runFetchQuote = async () => {
    try {
      const res = await tauriInvoke<IpcResponse<{ id: string; status: string }>>(
        'handle_guarded_ipc',
        {
          command_name: 'fetch_quote',
          session_token: sessionToken,
          payload: { offer_ref: offerRef, rev_suffix: revSuffix },
        }
      );
      setStatus(`fetch_quote: ${res.status} ${res.message}`);
      if (res.status === 200 && res.data?.id) {
        setCurrentQuoteId(res.data.id);
        setCurrentStatus(res.data.status);
      }
    } catch (err) {
      setStatus(`fetch_quote failed: ${(err as Error).message}`);
    }
  };

  // Phase 5.1: locking + audit-logged branching. Only meaningful once a quote is
  // actually locked (see quotes.rs::LOCKED_STATUSES) -- branch_new_revision itself
  // rejects an attempt against a still-Draft quote, this is just the matching frontend
  // affordance so that rejection is the expected/discoverable path, not a dead end.
  const runBranchNewRevision = async () => {
    if (!currentQuoteId) {
      setStatus('branch_new_revision: save or fetch a quote first to get a quote_id');
      return;
    }
    try {
      const res = await tauriInvoke<IpcResponse<{ new_quote_id: string; revision_id: string }>>(
        'handle_guarded_ipc',
        {
          command_name: 'branch_new_revision',
          session_token: sessionToken,
          payload: { quote_id: currentQuoteId, new_rev_suffix: newRevSuffix },
        }
      );
      setStatus(`branch_new_revision: ${res.status} ${res.message}`);
      if (res.status === 200 && res.data) {
        // Switch the working revision to the new Draft copy branch_new_revision just
        // created, so "Fetch quote" / "Generate PDF" below immediately act on it.
        setRevSuffix(newRevSuffix);
        setCurrentQuoteId(res.data.new_quote_id);
        setCurrentStatus('Draft');
      }
    } catch (err) {
      setStatus(`branch_new_revision failed: ${(err as Error).message}`);
    }
  };

  // Phase 5.2: derives the single valid next lifecycle step from currentStatus, or
  // null once at the end ("Job Booked") -- mirrors quotes.rs::update_quote_status's own
  // "exactly one step forward" rule so the button never even offers an invalid move.
  const nextStatus = (() => {
    if (!currentStatus) return null;
    const idx = quotes_rs_status_lifecycle.indexOf(currentStatus);
    if (idx === -1 || idx === quotes_rs_status_lifecycle.length - 1) return null;
    return quotes_rs_status_lifecycle[idx + 1];
  })();

  const runAdvanceStatus = async () => {
    if (!currentQuoteId || !nextStatus) {
      setStatus('update_quote_status: no quote_id/current status loaded, or already at the final status');
      return;
    }
    try {
      const res = await tauriInvoke<IpcResponse>('handle_guarded_ipc', {
        command_name: 'update_quote_status',
        session_token: sessionToken,
        payload: { quote_id: currentQuoteId, new_status: nextStatus },
      });
      setStatus(`update_quote_status: ${res.status} ${res.message}`);
      if (res.status === 200) {
        setCurrentStatus(nextStatus);
      }
    } catch (err) {
      setStatus(`update_quote_status failed: ${(err as Error).message}`);
    }
  };

  // Phase 5.2: real native file picker (route-map-v2.docx Section 1.1 guardrail) --
  // the path stored is whatever the OS's own dialog returns, never typed/constructed
  // free text. Requires lib.rs's `.plugin(tauri_plugin_dialog::init())` registration
  // and the "dialog:allow-open" capability permission (both added alongside this).
  const runPickLpoFile = async () => {
    try {
      const selected = await openFileDialog({
        multiple: false,
        title: 'Select LPO document',
      });
      if (typeof selected === 'string') {
        setLpoFilePath(selected);
        setStatus(`LPO file selected: ${selected}`);
      } else {
        setStatus('LPO file selection cancelled');
      }
    } catch (err) {
      setStatus(`File dialog failed: ${(err as Error).message}`);
    }
  };

  const runAttachLpo = async () => {
    if (!currentQuoteId) {
      setStatus('attach_lpo: save or fetch a quote first to get a quote_id');
      return;
    }
    if (!lpoNumber.trim()) {
      setStatus('attach_lpo: LPO number is required');
      return;
    }
    try {
      const res = await tauriInvoke<IpcResponse>('handle_guarded_ipc', {
        command_name: 'attach_lpo',
        session_token: sessionToken,
        payload: {
          quote_id: currentQuoteId,
          lpo_number: lpoNumber,
          lpo_file_path: lpoFilePath || null,
        },
      });
      setStatus(`attach_lpo: ${res.status} ${res.message}`);
    } catch (err) {
      setStatus(`attach_lpo failed: ${(err as Error).message}`);
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
          <button onClick={runCalculateRateMatrix}>3. Check rate tier</button>
          {/* FIX: previously showed a synthetic "@ X AED" derived by multiplying
              whatever was in the rate field by 5 or 15 -- looked like a real quoted
              rate, wasn't. This now only states which tier the duration falls into;
              it never implies a rate. Type the real rate in the line-item form below
              for whichever tier this shows. */}
          {rateResult && (
            <span>
              {' '}
              → {rateResult.rate_basis} rate applies for {durationDays} day(s) (tier:{' '}
              {rateResult.tier.min_days}
              {rateResult.tier.max_days ? `–${rateResult.tier.max_days}` : '+'} days).
              Enter the actual {rateResult.rate_basis.toLowerCase()} rate below.
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
        {/* Manual rate entry -- see runCalculateRateMatrix's comment above for why
            "3. Check rate tier" no longer writes into this field. This is always the
            user's own number, in either order: check the tier first and type the
            matching rate, or type a rate and check the tier afterward. */}
        <input
          type="number"
          placeholder="Rate (AED)"
          value={draft.unit_rate}
          onChange={(e) => setDraft({ ...draft, unit_rate: Number(e.target.value) })}
          style={{ width: '90px' }}
        />
        <select
          value={draft.rate_basis}
          onChange={(e) =>
            setDraft({ ...draft, rate_basis: e.target.value as LineItemDraft['rate_basis'] })
          }
        >
          <option value="Daily">Daily</option>
          <option value="Weekly">Weekly</option>
          <option value="Monthly">Monthly</option>
        </select>
        <button onClick={addLineItem}>
          {editingIndex !== null ? 'Save changes' : '+ Add line item'}
        </button>
        {editingIndex !== null && <button onClick={cancelEdit}>Cancel</button>}
      </div>

      {lineItems.length > 0 && (
        <ul style={{ fontSize: '13px', listStyle: 'none', paddingLeft: 0 }}>
          {lineItems.map((li, i) => (
            <li
              key={i}
              style={{
                marginBottom: '4px',
                background: editingIndex === i ? '#fff8e1' : 'transparent',
                padding: '2px 4px',
              }}
            >
              {li.item_description} — {li.make_model} × {li.quantity} @ {li.unit_rate}{' '}
              AED ({li.rate_basis}){' '}
              <button onClick={() => editLineItem(i)} style={{ fontSize: '11px' }}>
                Edit
              </button>{' '}
              <button onClick={() => removeLineItem(i)} style={{ fontSize: '11px' }}>
                Remove
              </button>
            </li>
          ))}
        </ul>
      )}

      {/* Phase 4.2 — Terms Library & Regional VAT Schema, selected per quote. Loaded
          via loadComplianceLibrary (called once register_session succeeds). Selections
          here are what runSaveQuote sends as tax_rule_id / attach_terms_to_quote's
          term_ids. */}
      <h4>Compliance Terms &amp; Tax</h4>
      <div style={{ display: 'flex', gap: '24px', flexWrap: 'wrap', marginBottom: '12px' }}>
        <div style={{ minWidth: '260px' }}>
          <strong style={{ fontSize: '13px' }}>Terms &amp; conditions to include</strong>
          {complianceTerms.length === 0 && (
            <p style={{ fontSize: '12px', color: '#5a6b75' }}>
              None saved yet for this tenant — add one below.
            </p>
          )}
          <ul style={{ fontSize: '13px', listStyle: 'none', paddingLeft: 0 }}>
            {complianceTerms.map((t) => (
              <li key={t.id}>
                <label>
                  <input
                    type="checkbox"
                    checked={selectedTermIds.includes(t.id)}
                    onChange={() => toggleTermSelected(t.id)}
                  />{' '}
                  <strong>[{t.category}]</strong> {t.title}
                </label>
              </li>
            ))}
          </ul>
          <div style={{ display: 'flex', gap: '4px', flexWrap: 'wrap', marginTop: '4px' }}>
            <select value={newTermCategory} onChange={(e) => setNewTermCategory(e.target.value)}>
              <option value="Payment">Payment</option>
              <option value="Liability">Liability</option>
              <option value="Warranty">Warranty</option>
              <option value="Cancellation">Cancellation</option>
              <option value="General">General</option>
            </select>
            <input
              placeholder="Title"
              value={newTermTitle}
              onChange={(e) => setNewTermTitle(e.target.value)}
              style={{ width: '120px' }}
            />
            <input
              placeholder="Clause text"
              value={newTermBody}
              onChange={(e) => setNewTermBody(e.target.value)}
              style={{ width: '220px' }}
            />
            <button onClick={runCreateComplianceTerm}>+ Add term</button>
          </div>
        </div>

        <div style={{ minWidth: '220px' }}>
          <strong style={{ fontSize: '13px' }}>Regional VAT / tax rule</strong>
          {taxRules.length === 0 && (
            <p style={{ fontSize: '12px', color: '#5a6b75' }}>
              None saved yet for this tenant — add one below (falls back to a flat 5% if
              none selected).
            </p>
          )}
          <div>
            <select
              value={selectedTaxRuleId}
              onChange={(e) => setSelectedTaxRuleId(e.target.value)}
              style={{ marginTop: '4px' }}
            >
              <option value="">-- none selected (fallback) --</option>
              {taxRules.map((r) => (
                <option key={r.id} value={r.id}>
                  {r.region_label} ({r.rate}%){r.is_default ? ' [default]' : ''}
                </option>
              ))}
            </select>
          </div>
          <div style={{ display: 'flex', gap: '4px', flexWrap: 'wrap', marginTop: '4px' }}>
            <input
              placeholder="Region label"
              value={newRuleLabel}
              onChange={(e) => setNewRuleLabel(e.target.value)}
              style={{ width: '140px' }}
            />
            <input
              type="number"
              placeholder="Rate %"
              value={newRuleRate}
              onChange={(e) => setNewRuleRate(Number(e.target.value))}
              style={{ width: '70px' }}
            />
            <button onClick={runCreateTaxRule}>+ Add tax rule</button>
          </div>
        </div>
      </div>

      <div style={{ marginTop: '16px', display: 'flex', gap: '8px', alignItems: 'center' }}>
        Rev: <strong>{revSuffix}</strong>
        {currentStatus && (
          <span
            style={{
              fontSize: '12px',
              padding: '2px 8px',
              borderRadius: '4px',
              background: quotes_rs_locked_statuses.includes(currentStatus) ? '#fde68a' : '#d1fae5',
            }}
          >
            {currentStatus}
            {quotes_rs_locked_statuses.includes(currentStatus) ? ' (locked)' : ''}
          </span>
        )}
        <button onClick={runSaveQuote}>4. Save quote (Draft only)</button>
        <button onClick={runFetchQuote}>
          5. Fetch quote (re-run after an app restart to verify persistence)
        </button>
        <button onClick={runGeneratePdf}>6. Generate PDF</button>
      </div>

      {/* Phase 5.1 — once a quote is locked, further edits go through here instead of
          runSaveQuote, which will now reject them (see quotes.rs's Locked variant). */}
      <div style={{ marginTop: '12px', display: 'flex', gap: '8px', alignItems: 'center' }}>
        <span style={{ fontSize: '13px' }}>Branch new revision from current quote_id:</span>
        <input
          placeholder="New rev suffix, e.g. Rev.02"
          value={newRevSuffix}
          onChange={(e) => setNewRevSuffix(e.target.value)}
          style={{ width: '140px' }}
        />
        <button onClick={runBranchNewRevision}>7. Branch new revision</button>
      </div>

      {/* Phase 5.2 — status lifecycle progression (Draft -> Issued -> LPO Confirmed ->
          Job Booked), one step at a time, and the Purchase Order tracking modal. */}
      <div style={{ marginTop: '12px', display: 'flex', gap: '8px', alignItems: 'center' }}>
        <span style={{ fontSize: '13px' }}>Advance status:</span>
        <button onClick={runAdvanceStatus} disabled={!nextStatus}>
          8. {nextStatus ? `Advance to "${nextStatus}"` : 'No further status (Job Booked)'}
        </button>
      </div>

      <div
        style={{
          marginTop: '12px',
          padding: '12px',
          border: '1px solid #d0d7de',
          borderRadius: '6px',
          maxWidth: '480px',
        }}
      >
        <strong style={{ fontSize: '13px' }}>
          9. Purchase Order (LPO) tracking
          {currentStatus && quotes_rs_status_lifecycle.indexOf(currentStatus) < 1 && (
            <span style={{ fontWeight: 400, color: '#5a6b75' }}>
              {' '}
              — advance past Draft first
            </span>
          )}
        </strong>
        <div style={{ display: 'flex', gap: '4px', flexWrap: 'wrap', marginTop: '6px' }}>
          <input
            placeholder="LPO number"
            value={lpoNumber}
            onChange={(e) => setLpoNumber(e.target.value)}
            style={{ width: '160px' }}
          />
          <button onClick={runPickLpoFile}>
            {lpoFilePath ? 'File selected ✓' : 'Choose LPO file...'}
          </button>
          <button onClick={runAttachLpo}>Attach LPO</button>
        </div>
        {lpoFilePath && (
          <p style={{ fontSize: '11px', color: '#5a6b75', marginTop: '4px', wordBreak: 'break-all' }}>
            {lpoFilePath}
          </p>
        )}
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
