// src/components/RateBasisLegend.tsx
// Phase 3.2 — Rate Basis Legend PDF Block
//
// FIXED (Architect Session 13 audit): the previous version called getAllTiers()
// but then rendered per-tier description text from a hardcoded if/else ladder
// keyed on tier.rateBasis name, not from the tier's own minDays/maxDays. That is
// the same drift risk as a fully hardcoded copy — if the engine's boundaries
// change, this component's rendered text would NOT change, silently reproducing
// the Session 11 class of bug. getTierDurationDescription() existed for this
// exact purpose and was never called; it's now the only source of the numbers.

import React from 'react';
import { getAllTiers } from '../../src-shared/rate-matrix';

export interface RateBasisLegendProps {
  className?: string;
  showHeader?: boolean;
}

/**
 * Render human-readable description of a tier's duration range, derived
 * ONLY from the tier's own minDays/maxDays — never from tier name.
 */
function getTierDurationDescription(minDays: number, maxDays: number | null): string {
  if (maxDays === null) {
    return `${minDays}+ days`;
  }
  if (minDays === maxDays) {
    return `${minDays} day`;
  }
  return `${minDays}–${maxDays} days`;
}

/**
 * Render the complete Rental Rate Basis legend
 * Matches the Servepower sample PDF's legend section (page 2, "Rental Rate Basis")
 */
export const RateBasisLegend: React.FC<RateBasisLegendProps> = ({
  className = '',
  showHeader = true,
}) => {
  const tiers = getAllTiers();

  return (
    <div className={className}>
      {showHeader && (
        <h3 style={{ margin: '12px 0 8px 0', fontSize: '13px', fontWeight: '600' }}>
          Rental Rate Basis
        </h3>
      )}

      <ul style={{ margin: '8px 0', paddingLeft: '20px', fontSize: '12px', lineHeight: '1.6' }}>
        {tiers.map((tier) => (
          <li key={tier.rateBasis} style={{ marginBottom: '4px' }}>
            <strong>{tier.rateBasis} rate</strong>: {getTierDurationDescription(tier.minDays, tier.maxDays)}
          </li>
        ))}
      </ul>
    </div>
  );
};

/**
 * Render legend for PDF paged media context (inline HTML string)
 * Returns raw HTML that can be injected into a PDF template.
 *
 * This is the ONLY function that should ever produce the legend markup used
 * in the PDF. quote_pdf_template.html must inject its output via a template
 * variable (see corrected template) rather than containing its own copy of
 * this markup — a second copy anywhere is the defect this phase exists to fix.
 */
export function getRateBasisLegendHTML(): string {
  const tiers = getAllTiers();

  const listItems = tiers
    .map((tier) => {
      const description = getTierDurationDescription(tier.minDays, tier.maxDays);
      return `<li><strong>${tier.rateBasis} rate</strong> :- For continuous rental period of ${description}</li>`;
    })
    .join('\n');

  return `
    <div class="rate-basis-legend-block">
      <h3 class="legend-title">Rental Rate Basis</h3>
      <ul class="legend-list">
        ${listItems}
      </ul>
      <p class="legend-note">
        <em>The applicable rate tier is automatically determined based on the selected rental period. All rates are in AED.</em>
      </p>
    </div>
  `;
}

export default RateBasisLegend;
