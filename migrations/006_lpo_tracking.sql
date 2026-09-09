-- ============================================================
-- 006_lpo_tracking.sql — Phase 5.2: Client LPO Tracking & Booking Status
--
-- route-map-v2.docx Phase 5.2 deliverables: "Purchase Order tracking modal, status
-- lifecycle (Draft → Issued → LPO Confirmed → Job Booked), and LPO file attachment
-- handler (via @tauri-apps/plugin-dialog, per Section 1.1 file-path guardrail)."
-- Completion Check: "Uploaded/linked LPO numbers seamlessly attach to confirmed quotes
-- and appear on audit views."
--
-- Design decision, flagged explicitly: an LPO is modeled as columns directly on
-- `quotes` (one active LPO per quote), not a separate join table. Unlike
-- `compliance_terms` (genuinely many-to-many — many quotes can reuse the same clause,
-- one quote can select many clauses), a quote's Purchase Order is inherently 1:1 —
-- route-map-v2.docx describes "a" Purchase Order tracking modal per quote, not a list.
-- A separate `lpo_documents` table would be the right call if this project later needs
-- to track LPO revisions or multiple attachments per quote; not built speculatively
-- here.
--
-- File-path guardrail (route-map-v2.docx Section 1.1, referenced directly by Phase
-- 5.2's own deliverable text): `lpo_file_path` stores only a path string the frontend
-- obtained from `@tauri-apps/plugin-dialog`'s real native file picker — the backend
-- never accepts a raw path typed by a caller and never constructs one itself. This is
-- enforced in `quotes::attach_lpo` (application code), the same non-DB-level guardrail
-- pattern this project already uses for `tax_rule_id` tenant ownership (migration 004).
-- ============================================================

ALTER TABLE quotes ADD COLUMN lpo_number VARCHAR(100);
ALTER TABLE quotes ADD COLUMN lpo_file_path TEXT;
ALTER TABLE quotes ADD COLUMN lpo_attached_at VARCHAR(30);
ALTER TABLE quotes ADD COLUMN lpo_attached_by VARCHAR(36) REFERENCES users(id) ON DELETE SET NULL;
