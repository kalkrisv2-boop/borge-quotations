/**
 * src-shared/rate-matrix.ts
 * Phase 3.1 — Tiered Rental Rate Matrix Engine
 *
 * Determines the applicable rental tier (Daily/Weekly/Monthly) based on rental
 * duration in days, and returns the corresponding unit_rate from the asset's
 * default rates.
 *
 * Tier Boundaries — verified against the authoritative Servepower sample
 * quotation PDF and route-map-v2.docx Phase 3.1 (both actually read this
 * session, not relayed from a prior session's restatement):
 *   - Daily:   1–5 days (inclusive) — source: "less than 6 days"
 *   - Weekly:  6–25 days (inclusive)
 *   - Monthly: 26+ days — source: "26 days or more"
 *
 * This corrects a regression: an earlier session moved these boundaries to
 * 1–6 / 7–25 / 26+ citing "the Servepower sample," but the source document
 * was never actually attached to that session. route-map-v2.docx's own
 * Completion Check ("exactly 6 days, exactly 25 days, exactly 26 days") and
 * SESSION_OPERATIONS_KIT.md Section 4 ("boundary unit tests (5, 6, 25, 26,
 * 1-day edge case)") both independently confirm the critical Daily/Weekly
 * boundary sits between 5 and 6, not 6 and 7.
 */

/**
 * Represents a single rental rate tier
 */
export interface RateTier {
  rateBasis: 'Daily' | 'Weekly' | 'Monthly';
  minDays: number;
  maxDays: number | null; // null = no upper limit
}

/**
 * Asset rate data structure (subset of full Asset)
 */
export interface AssetRates {
  default_daily_rate: number;
  default_weekly_rate: number;
  default_monthly_rate: number;
}

/**
 * Result of rate matrix calculation
 */
export interface RateMatrixResult {
  rateBasis: 'Daily' | 'Weekly' | 'Monthly';
  unitRate: number;
  tier: RateTier;
}

/**
 * Canonical tier definitions (Phase 3.1)
 * Per Servepower sample quotation and route-map-v2.docx:
 *   - Daily: "For continues rental period of less than 6 days" = 1–5 days
 *   - Weekly: "For continues rental period of 7 days or less than 25 days"
 *     (source phrasing is loose; route-map-v2.docx's own Completion Check and
 *     SESSION_OPERATIONS_KIT.md Section 4 both test the boundary at exactly
 *     6, confirming Weekly starts at 6, not 7) = 6–25 days
 *   - Monthly: "For continues rental period of 26 days or more" = 26+ days
 */
const RATE_TIERS: RateTier[] = [
  { rateBasis: 'Daily', minDays: 1, maxDays: 5 },
  { rateBasis: 'Weekly', minDays: 6, maxDays: 25 },
  { rateBasis: 'Monthly', minDays: 26, maxDays: null },
];

/**
 * Determine rental tier from duration in days
 *
 * @param durationDays - Rental duration in days (must be >= 1)
 * @returns The applicable RateTier
 * @throws Error if durationDays < 1 or is not a positive integer
 */
export function getTierByDuration(durationDays: number): RateTier {
  if (!Number.isInteger(durationDays) || durationDays < 1) {
    throw new Error(
      `Invalid rental duration: ${durationDays}. Duration must be a positive integer (1 or more days).`
    );
  }

  const tier = RATE_TIERS.find(
    (t) => durationDays >= t.minDays && (t.maxDays === null || durationDays <= t.maxDays)
  );

  if (!tier) {
    // This should never happen if RATE_TIERS covers all positive integers
    throw new Error(
      `No tier found for duration ${durationDays}. This indicates a misconfiguration in RATE_TIERS.`
    );
  }

  return tier;
}

/**
 * Calculate the applicable unit rate for a given rental duration
 *
 * @param durationDays - Rental duration in days (must be >= 1)
 * @param assetRates - Asset's default daily/weekly/monthly rates
 * @returns The applicable unit_rate (in AED) based on the determined tier
 * @throws Error if durationDays is invalid
 */
export function calculateUnitRate(durationDays: number, assetRates: AssetRates): number {
  const tier = getTierByDuration(durationDays);

  switch (tier.rateBasis) {
    case 'Daily':
      return assetRates.default_daily_rate;
    case 'Weekly':
      return assetRates.default_weekly_rate;
    case 'Monthly':
      return assetRates.default_monthly_rate;
    default:
      const _exhaustive: never = tier.rateBasis;
      return _exhaustive;
  }
}

/**
 * Perform full rate matrix calculation for a quote line item
 *
 * Given a rental duration and asset rates, determine:
 *   1. The applicable rate tier (Daily/Weekly/Monthly)
 *   2. The unit_rate from that tier
 *
 * @param durationDays - Rental duration in days (must be >= 1)
 * @param assetRates - Asset's default daily/weekly/monthly rates
 * @returns RateMatrixResult containing rateBasis, unitRate, and tier details
 * @throws Error if durationDays is invalid
 */
export function calculateRateMatrix(
  durationDays: number,
  assetRates: AssetRates
): RateMatrixResult {
  const tier = getTierByDuration(durationDays);
  const unitRate = calculateUnitRate(durationDays, assetRates);

  return {
    rateBasis: tier.rateBasis,
    unitRate,
    tier,
  };
}

/**
 * Calculate line total from quantity and duration
 *
 * Line Total = quantity × unit_rate
 *
 * NOTE: This implementation does NOT pro-rate within a tier (e.g., charging
 * a per-day rate for the exact 15 days). Instead, it applies the tier's
 * fixed unit_rate (e.g., weekly rate) to the quantity. If pro-rating within
 * tiers is required, check route-map-v2.docx Phase 3.1 section and update
 * this function accordingly.
 *
 * @param quantity - Number of units/instances to rent
 * @param durationDays - Rental duration in days
 * @param assetRates - Asset's default daily/weekly/monthly rates
 * @returns Line total in AED
 * @throws Error if quantity is invalid or durationDays is invalid
 */
export function calculateLineTotal(
  quantity: number,
  durationDays: number,
  assetRates: AssetRates
): number {
  if (!Number.isInteger(quantity) || quantity < 1) {
    throw new Error(
      `Invalid quantity: ${quantity}. Quantity must be a positive integer (1 or more).`
    );
  }

  const { unitRate } = calculateRateMatrix(durationDays, assetRates);
  return quantity * unitRate;
}

/**
 * Get all tier definitions (for UI rendering, etc.)
 */
export function getAllTiers(): RateTier[] {
  return [...RATE_TIERS];
}

export default {
  getTierByDuration,
  calculateUnitRate,
  calculateRateMatrix,
  calculateLineTotal,
  getAllTiers,
};
