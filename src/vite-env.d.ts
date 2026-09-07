/// <reference types="vite/client" />

// This single file gives TypeScript the ambient module declaration for
// *.module.css imports (and *.svg, *.png, etc.) project-wide, via Vite's
// own bundled client types. AssetPicker.module.css.d.ts and
// QuoteLineItemEditor.module.css.d.ts (hand-written per-component sidecar
// declarations) are not wrong and can stay — but with this file in place
// they're redundant: every *.module.css import anywhere in the project is
// now typed automatically, including any added in future phases, without
// needing a matching hand-written .d.ts for each new component. Delete the
// two sidecar files if you'd rather not maintain both patterns side by
// side, or leave them — TypeScript resolves the more specific declaration
// first with no conflict either way.
