// src/components/DateRangePicker.tsx
// Phase 3.2 — Date-range listener component
//
// Replaces raw rental_duration_days input with a date-based approach:
// User selects hire start date and end date → component calculates the
// duration in days and feeds it into the rate matrix engine.
//
// Day counting: inclusive of both start and end dates (standard rental
// industry practice: if you rent from Jan 1 to Jan 5, that's 5 days, not 4).
// No timezone offset applied — uses browser local date values only.

import React, { useState, useCallback, useEffect } from 'react';
import styles from './DateRangePicker.module.css';

export interface DateRangePickerProps {
  onDurationChange: (durationDays: number) => void;
  initialStartDate?: string; // YYYY-MM-DD
  initialEndDate?: string;   // YYYY-MM-DD
  minDate?: string; // Optional minimum selectable date
}

/**
 * Calculate rental duration in days from start and end dates (inclusive)
 *
 * @param startDate - ISO string (YYYY-MM-DD) or Date object
 * @param endDate   - ISO string (YYYY-MM-DD) or Date object
 * @returns Duration in days (inclusive: Jan 1 → Jan 5 = 5 days)
 * @throws Error if end date is before start date
 */
export function calculateDurationDays(startDate: string | Date, endDate: string | Date): number {
  let start: Date;
  let end: Date;

  if (typeof startDate === 'string') {
    start = new Date(startDate + 'T00:00:00'); // parse as local midnight
  } else {
    start = new Date(startDate);
    start.setHours(0, 0, 0, 0); // normalize to midnight
  }

  if (typeof endDate === 'string') {
    end = new Date(endDate + 'T00:00:00');
  } else {
    end = new Date(endDate);
    end.setHours(0, 0, 0, 0);
  }

  if (end < start) {
    throw new Error(
      `Invalid date range: end date (${endDate}) is before start date (${startDate})`
    );
  }

  // Calculate days: (end - start) / ms_per_day + 1 (inclusive)
  const msPerDay = 24 * 60 * 60 * 1000;
  const durationDays = Math.floor((end.getTime() - start.getTime()) / msPerDay) + 1;

  if (durationDays < 1) {
    throw new Error('Calculated duration must be at least 1 day');
  }

  return durationDays;
}

/**
 * Convert a Date object to YYYY-MM-DD string (local time)
 */
function dateToISOString(date: Date): string {
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, '0');
  const day = String(date.getDate()).padStart(2, '0');
  return `${year}-${month}-${day}`;
}

/**
 * Get today's date as YYYY-MM-DD string
 */
function getTodayString(): string {
  return dateToISOString(new Date());
}

export const DateRangePicker: React.FC<DateRangePickerProps> = ({
  onDurationChange,
  initialStartDate,
  initialEndDate,
  minDate,
}) => {
  const today = getTodayString();
  const defaultStart = initialStartDate || today;
  const defaultEnd = initialEndDate || today;

  const [startDate, setStartDate] = useState<string>(defaultStart);
  const [endDate, setEndDate] = useState<string>(defaultEnd);
  const [durationDays, setDurationDays] = useState<number>(0);
  const [error, setError] = useState<string | null>(null);

  /**
   * Recalculate duration whenever dates change
   */
  const recalculateDuration = useCallback(
    (start: string, end: string) => {
      try {
        const duration = calculateDurationDays(start, end);
        setDurationDays(duration);
        setError(null);
        onDurationChange(duration);
      } catch (err) {
        const message = err instanceof Error ? err.message : 'Invalid date range';
        setError(message);
        setDurationDays(0);
        onDurationChange(0); // Signal invalid state upstream
      }
    },
    [onDurationChange]
  );

  /**
   * Initialize duration on first mount
   */
  useEffect(() => {
    recalculateDuration(startDate, endDate);
  }, []); // Run only once on mount

  /**
   * Handle start date change
   */
  const handleStartDateChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const newStart = e.target.value;
    setStartDate(newStart);
    recalculateDuration(newStart, endDate);
  };

  /**
   * Handle end date change
   */
  const handleEndDateChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const newEnd = e.target.value;
    setEndDate(newEnd);
    recalculateDuration(startDate, newEnd);
  };

  return (
    <div className={styles.dateRangeContainer}>
      <h4 className={styles.title}>Rental Period</h4>

      <div className={styles.formRow}>
        <div className={styles.formGroup}>
          <label htmlFor="startDate">Start Date *</label>
          <input
            id="startDate"
            type="date"
            value={startDate}
            onChange={handleStartDateChange}
            min={minDate}
            required
          />
        </div>

        <div className={styles.formGroup}>
          <label htmlFor="endDate">End Date *</label>
          <input
            id="endDate"
            type="date"
            value={endDate}
            onChange={handleEndDateChange}
            min={startDate} // End date must be >= start date
            required
          />
        </div>

        <div className={styles.formGroup}>
          <label htmlFor="durationDisplay">Duration (days)</label>
          <div className={styles.readOnlyDisplay}>
            {durationDays > 0 ? durationDays : '—'}
          </div>
        </div>
      </div>

      {error && <div className={styles.errorText}>{error}</div>}

      <div className={styles.helpText}>
        {durationDays > 0 && (
          <>
            {durationDays} day{durationDays !== 1 ? 's' : ''} inclusive
          </>
        )}
      </div>
    </div>
  );
};

export default DateRangePicker;
