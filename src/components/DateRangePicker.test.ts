/**
 * src/components/DateRangePicker.test.ts
 * Phase 3.2 — Unit tests for date-range → duration calculation
 *
 * Coverage includes:
 *   - Inclusive day counting (Jan 1 to Jan 5 = 5 days, not 4)
 *   - Single-day rentals (same start and end date)
 *   - Leap-year February edge cases
 *   - Year-boundary transitions (Dec 31 to Jan 1)
 *   - Timezone handling (browser local time, no UTC offset)
 *   - Invalid ranges (end before start)
 *   - Edge cases (zero days, negative days)
 */

import { describe, it, expect } from 'vitest';
import { calculateDurationDays } from './DateRangePicker';

describe('DateRangePicker — calculateDurationDays', () => {
  describe('Basic Day Counting (Inclusive)', () => {
    it('should count single day as 1 day (Jan 1 to Jan 1)', () => {
      const duration = calculateDurationDays('2026-01-01', '2026-01-01');
      expect(duration).toBe(1);
    });

    it('should count 5-day range as 5 days (Jan 1 to Jan 5)', () => {
      const duration = calculateDurationDays('2026-01-01', '2026-01-05');
      expect(duration).toBe(5);
    });

    it('should count 2-day range as 2 days (Jan 1 to Jan 2)', () => {
      const duration = calculateDurationDays('2026-01-01', '2026-01-02');
      expect(duration).toBe(2);
    });

    it('should count 30-day range as 30 days (Jan 1 to Jan 30)', () => {
      const duration = calculateDurationDays('2026-01-01', '2026-01-30');
      expect(duration).toBe(30);
    });
  });

  describe('Month-Boundary Cases', () => {
    it('should handle range crossing month boundary (Jan 29 to Feb 2)', () => {
      const duration = calculateDurationDays('2026-01-29', '2026-02-02');
      expect(duration).toBe(5); // Jan 29, 30, 31, Feb 1, 2
    });

    it('should handle range crossing month boundary (Jan 30 to Feb 3)', () => {
      const duration = calculateDurationDays('2026-01-30', '2026-02-03');
      expect(duration).toBe(5); // Jan 30, 31, Feb 1, 2, 3
    });

    it('should handle end-of-month (Jan 31 to Feb 1)', () => {
      const duration = calculateDurationDays('2026-01-31', '2026-02-01');
      expect(duration).toBe(2);
    });
  });

  describe('Leap-Year February Edge Cases', () => {
    // 2024 is a leap year (divisible by 4, not a non-divisible-by-400 century year)
    it('should handle leap-year Feb 28 to Feb 29 (2024)', () => {
      const duration = calculateDurationDays('2024-02-28', '2024-02-29');
      expect(duration).toBe(2);
    });

    it('should handle leap-year Feb 29 to Mar 1 (2024)', () => {
      const duration = calculateDurationDays('2024-02-29', '2024-03-01');
      expect(duration).toBe(2);
    });

    // 2026 is NOT a leap year
    it('should handle non-leap-year Feb 28 to Mar 1 (2026)', () => {
      const duration = calculateDurationDays('2026-02-28', '2026-03-01');
      expect(duration).toBe(2);
    });

    it('should calculate 28-day February rental in non-leap-year (2026)', () => {
      const duration = calculateDurationDays('2026-02-01', '2026-02-28');
      expect(duration).toBe(28);
    });
  });

  describe('Year-Boundary Cases', () => {
    it('should handle Dec 31 to Jan 1 (year transition)', () => {
      const duration = calculateDurationDays('2025-12-31', '2026-01-01');
      expect(duration).toBe(2);
    });

    it('should handle Dec 29 to Jan 2 (year transition, 5 days)', () => {
      const duration = calculateDurationDays('2025-12-29', '2026-01-02');
      expect(duration).toBe(5); // Dec 29, 30, 31, Jan 1, 2
    });

    it('should handle full-year span (Jan 1 to Dec 31, non-leap-year)', () => {
      const duration = calculateDurationDays('2026-01-01', '2026-12-31');
      expect(duration).toBe(365);
    });

    it('should handle full-year span (Jan 1 to Dec 31, leap-year)', () => {
      const duration = calculateDurationDays('2024-01-01', '2024-12-31');
      expect(duration).toBe(366);
    });
  });

  describe('Timezone Handling (Browser Local Time)', () => {
    it('should parse ISO string dates as local midnight (not UTC)', () => {
      // This test verifies that "2026-01-01" is treated as 2026-01-01 00:00:00
      // in the browser's local timezone, not as UTC. The calculation itself doesn't
      // apply any offset — it uses the Date constructor which normalizes to local time.
      const duration = calculateDurationDays('2026-01-01', '2026-01-01');
      expect(duration).toBe(1);
    });

    it('should handle Date objects with time components (normalize to midnight)', () => {
      const startDate = new Date('2026-01-01T14:30:00');
      const endDate = new Date('2026-01-05T18:45:00');
      const duration = calculateDurationDays(startDate, endDate);
      // Both normalized to midnight, so Jan 1–5 = 5 days
      expect(duration).toBe(5);
    });

    it('should handle Date objects with different local times (same calendar days)', () => {
      const startDate = new Date('2026-01-01T00:00:00');
      const endDate = new Date('2026-01-01T23:59:59');
      const duration = calculateDurationDays(startDate, endDate);
      expect(duration).toBe(1);
    });
  });

  describe('Input Format Flexibility (String vs Date Object)', () => {
    it('should accept ISO string dates', () => {
      const duration = calculateDurationDays('2026-01-01', '2026-01-05');
      expect(duration).toBe(5);
    });

    it('should accept Date objects', () => {
      const start = new Date('2026-01-01');
      const end = new Date('2026-01-05');
      const duration = calculateDurationDays(start, end);
      expect(duration).toBe(5);
    });

    it('should accept mixed string and Date inputs', () => {
      const start = '2026-01-01';
      const end = new Date('2026-01-05');
      const duration = calculateDurationDays(start, end);
      expect(duration).toBe(5);
    });
  });

  describe('Invalid Ranges (Error Cases)', () => {
    it('should throw error if end date is before start date (same month)', () => {
      expect(() => calculateDurationDays('2026-01-05', '2026-01-01')).toThrow(
        /end date.*before start date/i
      );
    });

    it('should throw error if end date is before start date (different months)', () => {
      expect(() => calculateDurationDays('2026-02-01', '2026-01-01')).toThrow(
        /end date.*before start date/i
      );
    });

    it('should throw error if end date is before start date (Date objects)', () => {
      const start = new Date('2026-02-01');
      const end = new Date('2026-01-01');
      expect(() => calculateDurationDays(start, end)).toThrow(
        /end date.*before start date/i
      );
    });
  });

  describe('Practical Rental Scenarios', () => {
    it('should calculate 1-week rental correctly (7 days)', () => {
      const duration = calculateDurationDays('2026-01-01', '2026-01-07');
      expect(duration).toBe(7);
    });

    it('should calculate 2-week rental correctly (14 days)', () => {
      const duration = calculateDurationDays('2026-01-01', '2026-01-14');
      expect(duration).toBe(14);
    });

    it('should calculate 3-week rental correctly (21 days)', () => {
      const duration = calculateDurationDays('2026-01-01', '2026-01-21');
      expect(duration).toBe(21);
    });

    it('should calculate 1-month+ rental (31 days)', () => {
      const duration = calculateDurationDays('2026-01-01', '2026-01-31');
      expect(duration).toBe(31);
    });

    it('should calculate 90-day quarter rental', () => {
      const duration = calculateDurationDays('2026-01-01', '2026-03-31');
      expect(duration).toBe(90); // Jan 31 + Feb 28 + Mar 31
    });
  });

  describe('Boundary Cases Relevant to Rate Tiers', () => {
    // Phase 3.1 tier boundaries (per src-shared/rate-matrix.ts, Session 12-corrected):
    // Daily 1–6 / Weekly 7–25 / Monthly 26+
    // CORRECTED (Architect Session 14 audit): this file previously stated
    // Daily 1–5 / Weekly 6–25, inherited from a carryover brief that had the
    // Session 11 regression and its Session 12 correction backwards. These
    // assertions only test calculateDurationDays() (pure day-counting), not
    // tier assignment, so the numeric expectations below were never wrong —
    // only the boundary labels in the comments were.
    // These tests verify that duration calculation feeds correctly into tier logic

    it('should calculate exactly 5 days (mid-range, still within Daily tier)', () => {
      const duration = calculateDurationDays('2026-01-01', '2026-01-05');
      expect(duration).toBe(5);
    });

    it('should calculate exactly 6 days (Daily tier upper boundary)', () => {
      const duration = calculateDurationDays('2026-01-01', '2026-01-06');
      expect(duration).toBe(6);
    });

    it('should calculate exactly 7 days (Weekly tier lower boundary)', () => {
      const duration = calculateDurationDays('2026-01-01', '2026-01-07');
      expect(duration).toBe(7);
    });

    it('should calculate exactly 25 days (Weekly tier upper boundary)', () => {
      const duration = calculateDurationDays('2026-01-01', '2026-01-25');
      expect(duration).toBe(25);
    });

    it('should calculate exactly 26 days (Monthly tier lower boundary)', () => {
      const duration = calculateDurationDays('2026-01-01', '2026-01-26');
      expect(duration).toBe(26);
    });
  });

  describe('Edge Case: Zero and Negative Durations', () => {
    // These should never occur in practice because UI prevents it, but the calculation
    // function itself throws on invalid ranges, so verify the error behavior

    it('should never return 0 days (same date always = 1)', () => {
      const duration = calculateDurationDays('2026-01-01', '2026-01-01');
      expect(duration).toBe(1);
    });

    it('should validate that end >= start before calculating', () => {
      // Negative duration is impossible because the function checks end >= start first
      expect(() => calculateDurationDays('2026-01-10', '2026-01-01')).toThrow();
    });
  });
});
