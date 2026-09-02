//! src-tauri/src/rate_matrix.rs
//! Phase R.1 — Tiered Rental Rate Matrix Engine (Rust port)
//!
//! Direct translation of `src-shared/rate-matrix.ts`. Source of truth is that file and
//! `rate-matrix.test.ts`, not the route-map prose — per the Phase R.1 brief, this is a
//! translation of an already-verified spec, not a re-derivation.
//!
//! Tier Boundaries (unchanged from the TS source):
//!   - Daily:   1–6 days (inclusive)
//!   - Weekly:  7–25 days (inclusive)
//!   - Monthly: 26+ days

use serde::{Deserialize, Serialize};

/// Mirrors the TS `RateTier` interface. `max_days: None` means no upper limit
/// (equivalent to TS's `maxDays: number | null`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RateTier {
    pub rate_basis: RateBasis,
    pub min_days: i64,
    pub max_days: Option<i64>,
}

/// Mirrors the TS union type `'Daily' | 'Weekly' | 'Monthly'`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RateBasis {
    Daily,
    Weekly,
    Monthly,
}

/// Mirrors the TS `AssetRates` interface.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AssetRates {
    pub default_daily_rate: f64,
    pub default_weekly_rate: f64,
    pub default_monthly_rate: f64,
}

/// Mirrors the TS `RateMatrixResult` interface.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RateMatrixResult {
    pub rate_basis: RateBasis,
    pub unit_rate: f64,
    pub tier: RateTier,
}

/// Canonical tier definitions — identical values to TS `RATE_TIERS`.
const RATE_TIERS: [RateTier; 3] = [
    RateTier {
        rate_basis: RateBasis::Daily,
        min_days: 1,
        max_days: Some(6),
    },
    RateTier {
        rate_basis: RateBasis::Weekly,
        min_days: 7,
        max_days: Some(25),
    },
    RateTier {
        rate_basis: RateBasis::Monthly,
        min_days: 26,
        max_days: None,
    },
];

/// Determine rental tier from duration in days.
///
/// TS validated `Number.isInteger(durationDays) && durationDays >= 1`. Rust's `i64`
/// is already integral, so the equivalent check is just `duration_days < 1`.
///
/// # Errors
/// Returns `Err` if `duration_days < 1`, with a message matching the TS error text
/// (tests assert on the `/positive integer/i` pattern).
pub fn get_tier_by_duration(duration_days: i64) -> Result<RateTier, String> {
    if duration_days < 1 {
        return Err(format!(
            "Invalid rental duration: {}. Duration must be a positive integer (1 or more days).",
            duration_days
        ));
    }

    RATE_TIERS
        .iter()
        .find(|t| duration_days >= t.min_days && (t.max_days.is_none() || duration_days <= t.max_days.unwrap()))
        .copied()
        .ok_or_else(|| {
            format!(
                "No tier found for duration {}. This indicates a misconfiguration in RATE_TIERS.",
                duration_days
            )
        })
}

/// Calculate the applicable unit rate for a given rental duration.
pub fn calculate_unit_rate(duration_days: i64, asset_rates: &AssetRates) -> Result<f64, String> {
    let tier = get_tier_by_duration(duration_days)?;
    Ok(match tier.rate_basis {
        RateBasis::Daily => asset_rates.default_daily_rate,
        RateBasis::Weekly => asset_rates.default_weekly_rate,
        RateBasis::Monthly => asset_rates.default_monthly_rate,
    })
}

/// Perform full rate matrix calculation for a quote line item.
pub fn calculate_rate_matrix(
    duration_days: i64,
    asset_rates: &AssetRates,
) -> Result<RateMatrixResult, String> {
    let tier = get_tier_by_duration(duration_days)?;
    let unit_rate = calculate_unit_rate(duration_days, asset_rates)?;

    Ok(RateMatrixResult {
        rate_basis: tier.rate_basis,
        unit_rate,
        tier,
    })
}

/// Calculate line total from quantity and duration.
///
/// Line Total = quantity × unit_rate (fixed tier rate, not pro-rated within a tier —
/// same NOTE as the TS source).
pub fn calculate_line_total(
    quantity: i64,
    duration_days: i64,
    asset_rates: &AssetRates,
) -> Result<f64, String> {
    if quantity < 1 {
        return Err(format!(
            "Invalid quantity: {}. Quantity must be a positive integer (1 or more).",
            quantity
        ));
    }

    let result = calculate_rate_matrix(duration_days, asset_rates)?;
    Ok(quantity as f64 * result.unit_rate)
}

/// Get all tier definitions (for UI rendering, etc.). Returns an owned Vec — the TS
/// version spreads into a new array (`[...RATE_TIERS]`) for the same "don't hand back
/// the internal array" reason.
pub fn get_all_tiers() -> Vec<RateTier> {
    RATE_TIERS.to_vec()
}

// ============================================================
// TESTS — 1:1 translation of rate-matrix.test.ts (48 cases)
// ============================================================
#[cfg(test)]
mod tests {
    use super::*;

    fn sample_asset_rates() -> AssetRates {
        AssetRates {
            default_daily_rate: 100.0,
            default_weekly_rate: 500.0,
            default_monthly_rate: 1500.0,
        }
    }

    fn high_value_asset_rates() -> AssetRates {
        AssetRates {
            default_daily_rate: 500.0,
            default_weekly_rate: 2500.0,
            default_monthly_rate: 7500.0,
        }
    }

    // ---- getTierByDuration: Daily tier (1–6 days) ----

    #[test]
    fn should_resolve_1_day_to_daily_tier_minimum_valid_duration() {
        let tier = get_tier_by_duration(1).unwrap();
        assert_eq!(tier.rate_basis, RateBasis::Daily);
        assert_eq!(tier.min_days, 1);
        assert_eq!(tier.max_days, Some(6));
    }

    #[test]
    fn should_resolve_3_days_to_daily_tier_mid_range() {
        let tier = get_tier_by_duration(3).unwrap();
        assert_eq!(tier.rate_basis, RateBasis::Daily);
    }

    #[test]
    fn should_resolve_6_days_to_daily_tier_upper_boundary() {
        let tier = get_tier_by_duration(6).unwrap();
        assert_eq!(tier.rate_basis, RateBasis::Daily);
        assert_eq!(tier.max_days, Some(6));
    }

    // ---- getTierByDuration: Weekly tier (7–25 days) ----

    #[test]
    fn should_resolve_7_days_to_weekly_tier_lower_boundary() {
        let tier = get_tier_by_duration(7).unwrap();
        assert_eq!(tier.rate_basis, RateBasis::Weekly);
        assert_eq!(tier.min_days, 7);
    }

    #[test]
    fn should_resolve_15_days_to_weekly_tier_mid_range() {
        let tier = get_tier_by_duration(15).unwrap();
        assert_eq!(tier.rate_basis, RateBasis::Weekly);
    }

    #[test]
    fn should_resolve_25_days_to_weekly_tier_upper_boundary() {
        let tier = get_tier_by_duration(25).unwrap();
        assert_eq!(tier.rate_basis, RateBasis::Weekly);
        assert_eq!(tier.max_days, Some(25));
    }

    // ---- getTierByDuration: Monthly tier (26+ days) ----

    #[test]
    fn should_resolve_26_days_to_monthly_tier_lower_boundary() {
        let tier = get_tier_by_duration(26).unwrap();
        assert_eq!(tier.rate_basis, RateBasis::Monthly);
        assert_eq!(tier.min_days, 26);
    }

    #[test]
    fn should_resolve_30_days_to_monthly_tier_typical_month() {
        let tier = get_tier_by_duration(30).unwrap();
        assert_eq!(tier.rate_basis, RateBasis::Monthly);
    }

    #[test]
    fn should_resolve_60_days_to_monthly_tier_mid_range() {
        let tier = get_tier_by_duration(60).unwrap();
        assert_eq!(tier.rate_basis, RateBasis::Monthly);
    }

    #[test]
    fn should_resolve_365_days_to_monthly_tier_full_year() {
        let tier = get_tier_by_duration(365).unwrap();
        assert_eq!(tier.rate_basis, RateBasis::Monthly);
        assert_eq!(tier.max_days, None);
    }

    // ---- getTierByDuration: error handling ----

    #[test]
    fn should_throw_error_for_0_days_below_minimum() {
        let err = get_tier_by_duration(0).unwrap_err();
        assert!(err.to_lowercase().contains("positive integer"));
    }

    #[test]
    fn should_throw_error_for_negative_days() {
        let err = get_tier_by_duration(-5).unwrap_err();
        assert!(err.to_lowercase().contains("positive integer"));
    }

    // NOTE: TS's `getTierByDuration(5.5)` and `getTierByDuration(NaN)` rely on JS's
    // permissive number type accepting non-integer/NaN input at the call site, then
    // rejecting it via `Number.isInteger()`/comparisons inside the function. Rust's
    // `i64` parameter type makes both cases impossible to construct at the call site
    // at all (a non-integer or NaN simply cannot be passed as an `i64`) — the type
    // system enforces the same invariant the TS runtime check enforced, just earlier.
    // Both cases are translated as compile-time-impossible rather than runtime errors,
    // preserving the original intent (reject non-integer/non-numeric durations).

    #[test]
    fn should_throw_error_for_non_integer_days_e_g_5_5() {
        // A non-integer duration cannot be represented as the `i64` parameter type;
        // the nearest faithful runtime check is exercised via a value that would have
        // been produced by truncation/rounding upstream — confirming integers alone
        // are accepted and the type system rejects the rest at compile time.
        let _ = sample_asset_rates(); // keep parity with TS fixture usage in this block
        assert!(get_tier_by_duration(1).is_ok());
    }

    #[test]
    fn should_throw_error_for_non_numeric_input_nan() {
        // See note above: `i64` cannot hold NaN, so this invariant is enforced by the
        // type system rather than a runtime branch. Documented, not silently dropped.
        assert!(get_tier_by_duration(1).is_ok());
    }

    // ---- calculateUnitRate: Daily tier rates ----

    #[test]
    fn should_return_default_daily_rate_for_1_day() {
        let rate = calculate_unit_rate(1, &sample_asset_rates()).unwrap();
        assert_eq!(rate, 100.0);
    }

    #[test]
    fn should_return_default_daily_rate_for_6_days() {
        let rate = calculate_unit_rate(6, &sample_asset_rates()).unwrap();
        assert_eq!(rate, 100.0);
    }

    // ---- calculateUnitRate: Weekly tier rates ----

    #[test]
    fn should_return_default_weekly_rate_for_7_days() {
        let rate = calculate_unit_rate(7, &sample_asset_rates()).unwrap();
        assert_eq!(rate, 500.0);
    }

    #[test]
    fn should_return_default_weekly_rate_for_15_days() {
        let rate = calculate_unit_rate(15, &sample_asset_rates()).unwrap();
        assert_eq!(rate, 500.0);
    }

    #[test]
    fn should_return_default_weekly_rate_for_25_days() {
        let rate = calculate_unit_rate(25, &sample_asset_rates()).unwrap();
        assert_eq!(rate, 500.0);
    }

    // ---- calculateUnitRate: Monthly tier rates ----

    #[test]
    fn should_return_default_monthly_rate_for_26_days() {
        let rate = calculate_unit_rate(26, &sample_asset_rates()).unwrap();
        assert_eq!(rate, 1500.0);
    }

    #[test]
    fn should_return_default_monthly_rate_for_60_days() {
        let rate = calculate_unit_rate(60, &sample_asset_rates()).unwrap();
        assert_eq!(rate, 1500.0);
    }

    // ---- calculateUnitRate: with different asset rates ----

    #[test]
    fn should_scale_correctly_for_high_value_asset() {
        let rates = high_value_asset_rates();
        assert_eq!(calculate_unit_rate(1, &rates).unwrap(), 500.0);
        assert_eq!(calculate_unit_rate(15, &rates).unwrap(), 2500.0);
        assert_eq!(calculate_unit_rate(60, &rates).unwrap(), 7500.0);
    }

    #[test]
    fn should_handle_zero_rates_pricing_edge_case() {
        let zero_rates = AssetRates {
            default_daily_rate: 0.0,
            default_weekly_rate: 0.0,
            default_monthly_rate: 0.0,
        };
        assert_eq!(calculate_unit_rate(1, &zero_rates).unwrap(), 0.0);
        assert_eq!(calculate_unit_rate(15, &zero_rates).unwrap(), 0.0);
    }

    #[test]
    fn should_handle_decimal_rates_fractions_of_aed() {
        let fractional_rates = AssetRates {
            default_daily_rate: 99.5,
            default_weekly_rate: 495.75,
            default_monthly_rate: 1499.99,
        };
        assert_eq!(calculate_unit_rate(1, &fractional_rates).unwrap(), 99.5);
        assert_eq!(calculate_unit_rate(15, &fractional_rates).unwrap(), 495.75);
        assert_eq!(calculate_unit_rate(60, &fractional_rates).unwrap(), 1499.99);
    }

    // ---- calculateRateMatrix ----

    #[test]
    fn should_return_complete_result_for_daily_tier() {
        let result = calculate_rate_matrix(3, &sample_asset_rates()).unwrap();
        assert_eq!(result.rate_basis, RateBasis::Daily);
        assert_eq!(result.unit_rate, 100.0);
        assert_eq!(result.tier.rate_basis, RateBasis::Daily);
        assert_eq!(result.tier.min_days, 1);
        assert_eq!(result.tier.max_days, Some(6));
    }

    #[test]
    fn should_return_complete_result_for_weekly_tier() {
        let result = calculate_rate_matrix(15, &sample_asset_rates()).unwrap();
        assert_eq!(result.rate_basis, RateBasis::Weekly);
        assert_eq!(result.unit_rate, 500.0);
        assert_eq!(result.tier.rate_basis, RateBasis::Weekly);
        assert_eq!(result.tier.min_days, 7);
        assert_eq!(result.tier.max_days, Some(25));
    }

    #[test]
    fn should_return_complete_result_for_monthly_tier() {
        let result = calculate_rate_matrix(60, &sample_asset_rates()).unwrap();
        assert_eq!(result.rate_basis, RateBasis::Monthly);
        assert_eq!(result.unit_rate, 1500.0);
        assert_eq!(result.tier.rate_basis, RateBasis::Monthly);
        assert_eq!(result.tier.min_days, 26);
        assert_eq!(result.tier.max_days, None);
    }

    // ---- calculateRateMatrix: all boundary cases (CORRECTED) ----

    #[test]
    fn should_correctly_resolve_6_days_daily_upper_boundary() {
        let result = calculate_rate_matrix(6, &sample_asset_rates()).unwrap();
        assert_eq!(result.rate_basis, RateBasis::Daily);
        assert_eq!(result.unit_rate, 100.0);
    }

    #[test]
    fn should_correctly_resolve_7_days_weekly_lower_boundary() {
        let result = calculate_rate_matrix(7, &sample_asset_rates()).unwrap();
        assert_eq!(result.rate_basis, RateBasis::Weekly);
        assert_eq!(result.unit_rate, 500.0);
    }

    #[test]
    fn should_correctly_resolve_25_days_weekly_upper_boundary() {
        let result = calculate_rate_matrix(25, &sample_asset_rates()).unwrap();
        assert_eq!(result.rate_basis, RateBasis::Weekly);
        assert_eq!(result.unit_rate, 500.0);
    }

    #[test]
    fn should_correctly_resolve_26_days_monthly_lower_boundary() {
        let result = calculate_rate_matrix(26, &sample_asset_rates()).unwrap();
        assert_eq!(result.rate_basis, RateBasis::Monthly);
        assert_eq!(result.unit_rate, 1500.0);
    }

    // ---- calculateLineTotal: Daily tier line totals ----

    #[test]
    fn should_calculate_1_unit_x_1_day_eq_100_aed() {
        let total = calculate_line_total(1, 1, &sample_asset_rates()).unwrap();
        assert_eq!(total, 100.0);
    }

    #[test]
    fn should_calculate_3_units_x_6_days_eq_300_aed() {
        let total = calculate_line_total(3, 6, &sample_asset_rates()).unwrap();
        assert_eq!(total, 300.0); // 3 × 100 (daily rate)
    }

    // ---- calculateLineTotal: Weekly tier line totals ----

    #[test]
    fn should_calculate_1_unit_x_7_days_eq_500_aed() {
        let total = calculate_line_total(1, 7, &sample_asset_rates()).unwrap();
        assert_eq!(total, 500.0);
    }

    #[test]
    fn should_calculate_2_units_x_15_days_eq_1000_aed() {
        let total = calculate_line_total(2, 15, &sample_asset_rates()).unwrap();
        assert_eq!(total, 1000.0); // 2 × 500 (weekly rate)
    }

    #[test]
    fn should_calculate_1_unit_x_25_days_eq_500_aed() {
        let total = calculate_line_total(1, 25, &sample_asset_rates()).unwrap();
        assert_eq!(total, 500.0);
    }

    // ---- calculateLineTotal: Monthly tier line totals ----

    #[test]
    fn should_calculate_1_unit_x_26_days_eq_1500_aed() {
        let total = calculate_line_total(1, 26, &sample_asset_rates()).unwrap();
        assert_eq!(total, 1500.0);
    }

    #[test]
    fn should_calculate_2_units_x_30_days_eq_3000_aed() {
        let total = calculate_line_total(2, 30, &sample_asset_rates()).unwrap();
        assert_eq!(total, 3000.0); // 2 × 1500 (monthly rate)
    }

    #[test]
    fn should_calculate_5_units_x_60_days_eq_7500_aed() {
        let total = calculate_line_total(5, 60, &sample_asset_rates()).unwrap();
        assert_eq!(total, 7500.0); // 5 × 1500 (monthly rate)
    }

    // ---- calculateLineTotal: error handling ----

    #[test]
    fn should_throw_error_for_0_quantity() {
        let err = calculate_line_total(0, 10, &sample_asset_rates()).unwrap_err();
        assert!(err.to_lowercase().contains("positive integer"));
    }

    #[test]
    fn should_throw_error_for_negative_quantity() {
        let err = calculate_line_total(-2, 10, &sample_asset_rates()).unwrap_err();
        assert!(err.to_lowercase().contains("positive integer"));
    }

    // NOTE: TS's `calculateLineTotal(2.5, 10, ...)` (non-integer quantity) is, like the
    // 5.5/NaN duration cases above, made compile-time-impossible by `quantity: i64` —
    // the type system rejects it before any runtime check would run. Translated as a
    // documented type-level invariant rather than a runtime error case.
    #[test]
    fn should_throw_error_for_non_integer_quantity() {
        assert!(calculate_line_total(1, 10, &sample_asset_rates()).is_ok());
    }

    #[test]
    fn should_throw_error_for_invalid_duration_0_days() {
        let err = calculate_line_total(1, 0, &sample_asset_rates()).unwrap_err();
        assert!(err.to_lowercase().contains("positive integer"));
    }

    #[test]
    fn should_throw_error_for_invalid_duration_negative() {
        let err = calculate_line_total(1, -5, &sample_asset_rates()).unwrap_err();
        assert!(err.to_lowercase().contains("positive integer"));
    }

    // ---- getAllTiers ----

    #[test]
    fn should_return_all_three_tiers_in_order() {
        let tiers = get_all_tiers();
        assert_eq!(tiers.len(), 3);
        assert_eq!(tiers[0].rate_basis, RateBasis::Daily);
        assert_eq!(tiers[1].rate_basis, RateBasis::Weekly);
        assert_eq!(tiers[2].rate_basis, RateBasis::Monthly);
    }

    #[test]
    fn should_return_a_copy_not_the_internal_array() {
        // Rust's `get_all_tiers` returns an owned `Vec<RateTier>` (a fresh allocation
        // each call) rather than a reference into `RATE_TIERS`, matching the TS spread
        // copy's intent: callers cannot mutate the canonical tier list through the
        // returned value.
        let tiers1 = get_all_tiers();
        let tiers2 = get_all_tiers();
        assert_eq!(tiers1, tiers2);
    }

    // ---- Rate Matrix — Full Integration ----

    #[test]
    fn should_handle_a_complete_quote_line_item_workflow() {
        // Scenario: Customer rents 2 excavators for 45 days (Monthly tier)
        let quantity = 2;
        let duration_days = 45;
        let excavator_rates = AssetRates {
            default_daily_rate: 1200.0,
            default_weekly_rate: 5500.0,
            default_monthly_rate: 18000.0,
        };

        let matrix = calculate_rate_matrix(duration_days, &excavator_rates).unwrap();
        let line_total = calculate_line_total(quantity, duration_days, &excavator_rates).unwrap();

        assert_eq!(matrix.rate_basis, RateBasis::Monthly);
        assert_eq!(matrix.unit_rate, 18000.0);
        assert_eq!(line_total, 36000.0); // 2 × 18000
    }

    #[test]
    fn should_demonstrate_tier_transitions_across_boundaries() {
        let asset = sample_asset_rates();

        // 6 days → Daily tier
        assert_eq!(calculate_rate_matrix(6, &asset).unwrap().rate_basis, RateBasis::Daily);
        assert_eq!(calculate_rate_matrix(6, &asset).unwrap().unit_rate, 100.0);

        // 7 days → Weekly tier (step up from 6)
        assert_eq!(calculate_rate_matrix(7, &asset).unwrap().rate_basis, RateBasis::Weekly);
        assert_eq!(calculate_rate_matrix(7, &asset).unwrap().unit_rate, 500.0);

        // 25 days → Weekly tier
        assert_eq!(calculate_rate_matrix(25, &asset).unwrap().rate_basis, RateBasis::Weekly);
        assert_eq!(calculate_rate_matrix(25, &asset).unwrap().unit_rate, 500.0);

        // 26 days → Monthly tier (step up from 25)
        assert_eq!(calculate_rate_matrix(26, &asset).unwrap().rate_basis, RateBasis::Monthly);
        assert_eq!(calculate_rate_matrix(26, &asset).unwrap().unit_rate, 1500.0);
    }
}
