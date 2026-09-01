/**
 * src-shared/rate-matrix.test.ts
 * Phase 3.1 — Comprehensive unit tests for tiered rental rate matrix engine
 *
 * Boundaries verified against the actual Servepower sample quotation PDF and
 * route-map-v2.docx (both read directly this session):
 *   Daily: 1–5 days | Weekly: 6–25 days | Monthly: 26+ days
 *
 * Test Coverage Requirements (route-map-v2.docx Phase 3.1 Completion Check +
 * SESSION_OPERATIONS_KIT.md Section 4):
 *   ✓ Boundary conditions: exactly 5, 6, 25, and 26 days (tier-transition edges)
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

const sampleAssetRates: AssetRates = {
  default_daily_rate: 100,
  default_weekly_rate: 500,
  default_monthly_rate: 1500,
};

const highValueAssetRates: AssetRates = {
  default_daily_rate: 500,
  default_weekly_rate: 2500,
  default_monthly_rate: 7500,
};

describe('getTierByDuration', () => {
  describe('Daily tier (1–5 days)', () => {
    it('should resolve 1 day to Daily tier (minimum valid duration)', () => {
      const tier = getTierByDuration(1);
      expect(tier.rateBasis).toBe('Daily');
      expect(tier.minDays).toBe(1);
      expect(tier.maxDays).toBe(5);
    });

    it('should resolve 3 days to Daily tier (mid-range)', () => {
      const tier = getTierByDuration(3);
      expect(tier.rateBasis).toBe('Daily');
    });

    it('should resolve 5 days to Daily tier (upper boundary)', () => {
      const tier = getTierByDuration(5);
      expect(tier.rateBasis).toBe('Daily');
      expect(tier.maxDays).toBe(5);
    });
  });

  describe('Weekly tier (6–25 days)', () => {
    it('should resolve 6 days to Weekly tier (lower boundary — NOT Daily)', () => {
      const tier = getTierByDuration(6);
      expect(tier.rateBasis).toBe('Weekly');
      expect(tier.minDays).toBe(6);
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
    it('should resolve 26 days to Monthly tier (lower boundary — NOT Weekly)', () => {
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

describe('calculateUnitRate', () => {
  describe('Daily tier rates', () => {
    it('should return default_daily_rate for 1 day', () => {
      expect(calculateUnitRate(1, sampleAssetRates)).toBe(100);
    });

    it('should return default_daily_rate for 5 days', () => {
      expect(calculateUnitRate(5, sampleAssetRates)).toBe(100);
    });
  });

  describe('Weekly tier rates', () => {
    it('should return default_weekly_rate for 6 days', () => {
      expect(calculateUnitRate(6, sampleAssetRates)).toBe(500);
    });

    it('should return default_weekly_rate for 15 days', () => {
      expect(calculateUnitRate(15, sampleAssetRates)).toBe(500);
    });

    it('should return default_weekly_rate for 25 days', () => {
      expect(calculateUnitRate(25, sampleAssetRates)).toBe(500);
    });
  });

  describe('Monthly tier rates', () => {
    it('should return default_monthly_rate for 26 days', () => {
      expect(calculateUnitRate(26, sampleAssetRates)).toBe(1500);
    });

    it('should return default_monthly_rate for 60 days', () => {
      expect(calculateUnitRate(60, sampleAssetRates)).toBe(1500);
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

describe('calculateRateMatrix', () => {
  it('should return complete result for Daily tier', () => {
    const result = calculateRateMatrix(3, sampleAssetRates);
    expect(result).toEqual({
      rateBasis: 'Daily',
      unitRate: 100,
      tier: expect.objectContaining({ rateBasis: 'Daily', minDays: 1, maxDays: 5 }),
    });
  });

  it('should return complete result for Weekly tier', () => {
    const result = calculateRateMatrix(15, sampleAssetRates);
    expect(result).toEqual({
      rateBasis: 'Weekly',
      unitRate: 500,
      tier: expect.objectContaining({ rateBasis: 'Weekly', minDays: 6, maxDays: 25 }),
    });
  });

  it('should return complete result for Monthly tier', () => {
    const result = calculateRateMatrix(60, sampleAssetRates);
    expect(result).toEqual({
      rateBasis: 'Monthly',
      unitRate: 1500,
      tier: expect.objectContaining({ rateBasis: 'Monthly', minDays: 26, maxDays: null }),
    });
  });

  describe('all boundary cases (route-map-v2.docx: exactly 5, 6, 25, 26)', () => {
    it('should correctly resolve 5 days (Daily upper boundary)', () => {
      const result = calculateRateMatrix(5, sampleAssetRates);
      expect(result.rateBasis).toBe('Daily');
      expect(result.unitRate).toBe(100);
    });

    it('should correctly resolve 6 days (Weekly lower boundary)', () => {
      const result = calculateRateMatrix(6, sampleAssetRates);
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

describe('calculateLineTotal', () => {
  describe('Daily tier line totals', () => {
    it('should calculate 1 unit × 1 day = 100 AED', () => {
      expect(calculateLineTotal(1, 1, sampleAssetRates)).toBe(100);
    });

    it('should calculate 3 units × 5 days = 300 AED', () => {
      expect(calculateLineTotal(3, 5, sampleAssetRates)).toBe(300);
    });
  });

  describe('Weekly tier line totals', () => {
    it('should calculate 1 unit × 6 days = 500 AED', () => {
      expect(calculateLineTotal(1, 6, sampleAssetRates)).toBe(500);
    });

    it('should calculate 2 units × 15 days = 1000 AED', () => {
      expect(calculateLineTotal(2, 15, sampleAssetRates)).toBe(1000);
    });

    it('should calculate 1 unit × 25 days = 500 AED', () => {
      expect(calculateLineTotal(1, 25, sampleAssetRates)).toBe(500);
    });
  });

  describe('Monthly tier line totals', () => {
    it('should calculate 1 unit × 26 days = 1500 AED', () => {
      expect(calculateLineTotal(1, 26, sampleAssetRates)).toBe(1500);
    });

    it('should calculate 2 units × 30 days = 3000 AED', () => {
      expect(calculateLineTotal(2, 30, sampleAssetRates)).toBe(3000);
    });

    it('should calculate 5 units × 60 days = 7500 AED', () => {
      expect(calculateLineTotal(5, 60, sampleAssetRates)).toBe(7500);
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

describe('getAllTiers', () => {
  it('should return all three tiers in order', () => {
    const tiers = getAllTiers();
    expect(tiers).toHaveLength(3);
    expect(tiers[0]).toEqual({ rateBasis: 'Daily', minDays: 1, maxDays: 5 });
    expect(tiers[1]).toEqual({ rateBasis: 'Weekly', minDays: 6, maxDays: 25 });
    expect(tiers[2]).toEqual({ rateBasis: 'Monthly', minDays: 26, maxDays: null });
  });

  it('should return a copy, not the internal array', () => {
    const tiers1 = getAllTiers();
    const tiers2 = getAllTiers();
    expect(tiers1).not.toBe(tiers2);
    expect(tiers1).toEqual(tiers2);
  });
});

describe('Rate Matrix — Full Integration', () => {
  it('should handle a complete quote line item workflow', () => {
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
    expect(lineTotal).toBe(36000);
  });

  it('should demonstrate tier transitions across the actual boundaries (5/6 and 25/26)', () => {
    const asset = sampleAssetRates;

    expect(calculateRateMatrix(5, asset).rateBasis).toBe('Daily');
    expect(calculateRateMatrix(5, asset).unitRate).toBe(100);

    expect(calculateRateMatrix(6, asset).rateBasis).toBe('Weekly');
    expect(calculateRateMatrix(6, asset).unitRate).toBe(500);

    expect(calculateRateMatrix(25, asset).rateBasis).toBe('Weekly');
    expect(calculateRateMatrix(25, asset).unitRate).toBe(500);

    expect(calculateRateMatrix(26, asset).rateBasis).toBe('Monthly');
    expect(calculateRateMatrix(26, asset).unitRate).toBe(1500);
  });
});
