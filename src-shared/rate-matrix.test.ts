/**
 * src-shared/rate-matrix.test.ts
 * Phase 3.1 — Comprehensive unit tests for tiered rental rate matrix engine
 * CORRECTED: Tier boundaries per Servepower sample & route-map-v2.docx
 *   Daily: 1–6 days | Weekly: 7–25 days | Monthly: 26+ days
 *
 * Test Coverage Requirements (per Phase 3.1 brief):
 *   ✓ Boundary conditions: exactly 6, 7, 25, 26 days (tier-transition edges)
 *   ✓ 1-day edge case (minimum valid duration)
 *   ✓ Standard mid-tier cases (3, 15, 60 days) for basic sanity
 */

import { describe, it, expect } from 'vitest';
import {
  getTierByDuration,
  calculateUnitRate,
  calculateRateMatrix,
  calculateLineTotal,
  getAllTiers,
  type AssetRates,
  type RateTier,
} from './rate-matrix';

// ============================================================
// TEST DATA & FIXTURES
// ============================================================

/**
 * Sample asset rates for testing (all in AED)
 */
const sampleAssetRates: AssetRates = {
  default_daily_rate: 100,
  default_weekly_rate: 500,
  default_monthly_rate: 1500,
};

/**
 * Different asset rates to test scaling
 */
const highValueAssetRates: AssetRates = {
  default_daily_rate: 500,
  default_weekly_rate: 2500,
  default_monthly_rate: 7500,
};

// ============================================================
// TEST SUITE: getTierByDuration()
// ============================================================

describe('getTierByDuration', () => {
  describe('Daily tier (1–6 days)', () => {
    it('should resolve 1 day to Daily tier (minimum valid duration)', () => {
      const tier = getTierByDuration(1);
      expect(tier.rateBasis).toBe('Daily');
      expect(tier.minDays).toBe(1);
      expect(tier.maxDays).toBe(6);
    });

    it('should resolve 3 days to Daily tier (mid-range)', () => {
      const tier = getTierByDuration(3);
      expect(tier.rateBasis).toBe('Daily');
    });

    it('should resolve 6 days to Daily tier (upper boundary)', () => {
      const tier = getTierByDuration(6);
      expect(tier.rateBasis).toBe('Daily');
      expect(tier.maxDays).toBe(6);
    });
  });

  describe('Weekly tier (7–25 days)', () => {
    it('should resolve 7 days to Weekly tier (lower boundary)', () => {
      const tier = getTierByDuration(7);
      expect(tier.rateBasis).toBe('Weekly');
      expect(tier.minDays).toBe(7);
    });

    it('should resolve 15 days to Weekly tier (mid-range)', () => {
      const tier = getTierByDuration(15);
      expect(tier.rateBasis).toBe('Weekly');
    });

    it('should resolve 25 days to Weekly tier (upper boundary)', () => {
      const tier = getTierByDuration(25);
      expect(tier.rateBasis).toBe('Weekly');
      expect(tier.maxDays).toBe(25);
    });
  });

  describe('Monthly tier (26+ days)', () => {
    it('should resolve 26 days to Monthly tier (lower boundary)', () => {
      const tier = getTierByDuration(26);
      expect(tier.rateBasis).toBe('Monthly');
      expect(tier.minDays).toBe(26);
    });

    it('should resolve 30 days to Monthly tier (typical month)', () => {
      const tier = getTierByDuration(30);
      expect(tier.rateBasis).toBe('Monthly');
    });

    it('should resolve 60 days to Monthly tier (mid-range)', () => {
      const tier = getTierByDuration(60);
      expect(tier.rateBasis).toBe('Monthly');
    });

    it('should resolve 365 days to Monthly tier (full year)', () => {
      const tier = getTierByDuration(365);
      expect(tier.rateBasis).toBe('Monthly');
      expect(tier.maxDays).toBeNull();
    });
  });

  describe('error handling', () => {
    it('should throw error for 0 days (below minimum)', () => {
      expect(() => getTierByDuration(0)).toThrow(/positive integer/i);
    });

    it('should throw error for negative days', () => {
      expect(() => getTierByDuration(-5)).toThrow(/positive integer/i);
    });

    it('should throw error for non-integer days (e.g., 5.5)', () => {
      expect(() => getTierByDuration(5.5)).toThrow(/positive integer/i);
    });

    it('should throw error for non-numeric input (NaN)', () => {
      expect(() => getTierByDuration(NaN)).toThrow(/positive integer/i);
    });
  });
});

// ============================================================
// TEST SUITE: calculateUnitRate()
// ============================================================

describe('calculateUnitRate', () => {
  describe('Daily tier rates', () => {
    it('should return default_daily_rate for 1 day', () => {
      const rate = calculateUnitRate(1, sampleAssetRates);
      expect(rate).toBe(100);
    });

    it('should return default_daily_rate for 6 days', () => {
      const rate = calculateUnitRate(6, sampleAssetRates);
      expect(rate).toBe(100);
    });
  });

  describe('Weekly tier rates', () => {
    it('should return default_weekly_rate for 7 days', () => {
      const rate = calculateUnitRate(7, sampleAssetRates);
      expect(rate).toBe(500);
    });

    it('should return default_weekly_rate for 15 days', () => {
      const rate = calculateUnitRate(15, sampleAssetRates);
      expect(rate).toBe(500);
    });

    it('should return default_weekly_rate for 25 days', () => {
      const rate = calculateUnitRate(25, sampleAssetRates);
      expect(rate).toBe(500);
    });
  });

  describe('Monthly tier rates', () => {
    it('should return default_monthly_rate for 26 days', () => {
      const rate = calculateUnitRate(26, sampleAssetRates);
      expect(rate).toBe(1500);
    });

    it('should return default_monthly_rate for 60 days', () => {
      const rate = calculateUnitRate(60, sampleAssetRates);
      expect(rate).toBe(1500);
    });
  });

  describe('with different asset rates', () => {
    it('should scale correctly for high-value asset', () => {
      expect(calculateUnitRate(1, highValueAssetRates)).toBe(500);
      expect(calculateUnitRate(15, highValueAssetRates)).toBe(2500);
      expect(calculateUnitRate(60, highValueAssetRates)).toBe(7500);
    });

    it('should handle zero rates (pricing edge case)', () => {
      const zeroRates: AssetRates = {
        default_daily_rate: 0,
        default_weekly_rate: 0,
        default_monthly_rate: 0,
      };
      expect(calculateUnitRate(1, zeroRates)).toBe(0);
      expect(calculateUnitRate(15, zeroRates)).toBe(0);
    });

    it('should handle decimal rates (fractions of AED)', () => {
      const fractionalRates: AssetRates = {
        default_daily_rate: 99.5,
        default_weekly_rate: 495.75,
        default_monthly_rate: 1499.99,
      };
      expect(calculateUnitRate(1, fractionalRates)).toBe(99.5);
      expect(calculateUnitRate(15, fractionalRates)).toBe(495.75);
      expect(calculateUnitRate(60, fractionalRates)).toBe(1499.99);
    });
  });
});

// ============================================================
// TEST SUITE: calculateRateMatrix()
// ============================================================

describe('calculateRateMatrix', () => {
  it('should return complete result for Daily tier', () => {
    const result = calculateRateMatrix(3, sampleAssetRates);
    expect(result).toEqual({
      rateBasis: 'Daily',
      unitRate: 100,
      tier: expect.objectContaining({
        rateBasis: 'Daily',
        minDays: 1,
        maxDays: 6,
      }),
    });
  });

  it('should return complete result for Weekly tier', () => {
    const result = calculateRateMatrix(15, sampleAssetRates);
    expect(result).toEqual({
      rateBasis: 'Weekly',
      unitRate: 500,
      tier: expect.objectContaining({
        rateBasis: 'Weekly',
        minDays: 7,
        maxDays: 25,
      }),
    });
  });

  it('should return complete result for Monthly tier', () => {
    const result = calculateRateMatrix(60, sampleAssetRates);
    expect(result).toEqual({
      rateBasis: 'Monthly',
      unitRate: 1500,
      tier: expect.objectContaining({
        rateBasis: 'Monthly',
        minDays: 26,
        maxDays: null,
      }),
    });
  });

  describe('all boundary cases (CORRECTED)', () => {
    it('should correctly resolve 6 days (Daily upper boundary)', () => {
      const result = calculateRateMatrix(6, sampleAssetRates);
      expect(result.rateBasis).toBe('Daily');
      expect(result.unitRate).toBe(100);
    });

    it('should correctly resolve 7 days (Weekly lower boundary)', () => {
      const result = calculateRateMatrix(7, sampleAssetRates);
      expect(result.rateBasis).toBe('Weekly');
      expect(result.unitRate).toBe(500);
    });

    it('should correctly resolve 25 days (Weekly upper boundary)', () => {
      const result = calculateRateMatrix(25, sampleAssetRates);
      expect(result.rateBasis).toBe('Weekly');
      expect(result.unitRate).toBe(500);
    });

    it('should correctly resolve 26 days (Monthly lower boundary)', () => {
      const result = calculateRateMatrix(26, sampleAssetRates);
      expect(result.rateBasis).toBe('Monthly');
      expect(result.unitRate).toBe(1500);
    });
  });
});

// ============================================================
// TEST SUITE: calculateLineTotal()
// ============================================================

describe('calculateLineTotal', () => {
  describe('Daily tier line totals', () => {
    it('should calculate 1 unit × 1 day = 100 AED', () => {
      const total = calculateLineTotal(1, 1, sampleAssetRates);
      expect(total).toBe(100);
    });

    it('should calculate 3 units × 6 days = 300 AED', () => {
      const total = calculateLineTotal(3, 6, sampleAssetRates);
      expect(total).toBe(300); // 3 × 100 (daily rate)
    });
  });

  describe('Weekly tier line totals', () => {
    it('should calculate 1 unit × 7 days = 500 AED', () => {
      const total = calculateLineTotal(1, 7, sampleAssetRates);
      expect(total).toBe(500);
    });

    it('should calculate 2 units × 15 days = 1000 AED', () => {
      const total = calculateLineTotal(2, 15, sampleAssetRates);
      expect(total).toBe(1000); // 2 × 500 (weekly rate)
    });

    it('should calculate 1 unit × 25 days = 500 AED', () => {
      const total = calculateLineTotal(1, 25, sampleAssetRates);
      expect(total).toBe(500);
    });
  });

  describe('Monthly tier line totals', () => {
    it('should calculate 1 unit × 26 days = 1500 AED', () => {
      const total = calculateLineTotal(1, 26, sampleAssetRates);
      expect(total).toBe(1500);
    });

    it('should calculate 2 units × 30 days = 3000 AED', () => {
      const total = calculateLineTotal(2, 30, sampleAssetRates);
      expect(total).toBe(3000); // 2 × 1500 (monthly rate)
    });

    it('should calculate 5 units × 60 days = 7500 AED', () => {
      const total = calculateLineTotal(5, 60, sampleAssetRates);
      expect(total).toBe(7500); // 5 × 1500 (monthly rate)
    });
  });

  describe('error handling', () => {
    it('should throw error for 0 quantity', () => {
      expect(() => calculateLineTotal(0, 10, sampleAssetRates)).toThrow(/positive integer/i);
    });

    it('should throw error for negative quantity', () => {
      expect(() => calculateLineTotal(-2, 10, sampleAssetRates)).toThrow(/positive integer/i);
    });

    it('should throw error for non-integer quantity', () => {
      expect(() => calculateLineTotal(2.5, 10, sampleAssetRates)).toThrow(/positive integer/i);
    });

    it('should throw error for invalid duration (0 days)', () => {
      expect(() => calculateLineTotal(1, 0, sampleAssetRates)).toThrow(/positive integer/i);
    });

    it('should throw error for invalid duration (negative)', () => {
      expect(() => calculateLineTotal(1, -5, sampleAssetRates)).toThrow(/positive integer/i);
    });
  });
});

// ============================================================
// TEST SUITE: getAllTiers()
// ============================================================

describe('getAllTiers', () => {
  it('should return all three tiers in order', () => {
    const tiers = getAllTiers();
    expect(tiers).toHaveLength(3);
    expect(tiers[0].rateBasis).toBe('Daily');
    expect(tiers[1].rateBasis).toBe('Weekly');
    expect(tiers[2].rateBasis).toBe('Monthly');
  });

  it('should return a copy, not the internal array', () => {
    const tiers1 = getAllTiers();
    const tiers2 = getAllTiers();
    expect(tiers1).not.toBe(tiers2);
    expect(tiers1).toEqual(tiers2);
  });
});

// ============================================================
// INTEGRATION TEST SUITE
// ============================================================

describe('Rate Matrix — Full Integration', () => {
  it('should handle a complete quote line item workflow', () => {
    // Scenario: Customer rents 2 excavators for 45 days (Monthly tier)
    const quantity = 2;
    const durationDays = 45;
    const excavatorRates: AssetRates = {
      default_daily_rate: 1200,
      default_weekly_rate: 5500,
      default_monthly_rate: 18000,
    };

    const matrix = calculateRateMatrix(durationDays, excavatorRates);
    const lineTotal = calculateLineTotal(quantity, durationDays, excavatorRates);

    expect(matrix.rateBasis).toBe('Monthly');
    expect(matrix.unitRate).toBe(18000);
    expect(lineTotal).toBe(36000); // 2 × 18000
  });

  it('should demonstrate tier transitions across boundaries', () => {
    const asset = sampleAssetRates;

    // 6 days → Daily tier
    expect(calculateRateMatrix(6, asset).rateBasis).toBe('Daily');
    expect(calculateRateMatrix(6, asset).unitRate).toBe(100);

    // 7 days → Weekly tier (step up from 6)
    expect(calculateRateMatrix(7, asset).rateBasis).toBe('Weekly');
    expect(calculateRateMatrix(7, asset).unitRate).toBe(500);

    // 25 days → Weekly tier
    expect(calculateRateMatrix(25, asset).rateBasis).toBe('Weekly');
    expect(calculateRateMatrix(25, asset).unitRate).toBe(500);

    // 26 days → Monthly tier (step up from 25)
    expect(calculateRateMatrix(26, asset).rateBasis).toBe('Monthly');
    expect(calculateRateMatrix(26, asset).unitRate).toBe(1500);
  });
});
