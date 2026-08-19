//! Post-fill fee re-derivation for the SATURATED regime of `CvammFeeLib`.
//!
//! The fee law is `fee = clamp(out + (mid-out)*g*v, out, mid) * skewMultiplier`, floored
//! by an optional hook floor. Its `v` term carries realized variance, which has no getter
//! on the ALM and so cannot be recomputed off-chain. But when `g*v` saturates the cap the
//! base collapses to `MidFee`, and the whole fee becomes `MidFee * multiplier` — and the
//! multiplier depends only on the inventory coordinate and the reserve split, both of
//! which a fill leaves us holding exactly.
//!
//! So a fill can be repriced exactly whenever the PRE-fill sample is reproduced by that
//! same expression in BOTH directions. When it is not — the book left saturation, the hook
//! floor binds, or the hook was upgraded — [`re_sample_fees`] returns `None` and the caller
//! keeps the conservative fold. [`super::fee_law_exact`] prices the unsaturated regime
//! and is preferred wherever it applies; this bound is what stands behind it.

use num_bigint::BigUint;
use num_traits::Zero;

/// The terms of `CvammFeeLib`'s law, all read from the ALM's `ClammFeeHook`.
///
/// Realized variance is deliberately absent: it has no getter, and it reaches the fee only
/// through a scalar a swap cannot move, so it is solved from the sampled fees rather than
/// read. The reservation price is absent for a different reason — it moves with the
/// anchor, so the state derives it from `anchor_sqrt_curve_x96`.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FeeLawTerms {
    /// `inventoryBalancedFeeWad` — the law's base cap.
    pub mid_fee_wad: BigUint,
    /// `inventoryImbalancedFeeWad` — the law's base floor.
    pub out_fee_wad: BigUint,
    /// `inventoryFeeCurvatureWad`. Zero selects the law's scalar branch, where the fee is
    /// `lp_fee_wad` regardless of the multiplier.
    pub curvature_wad: BigUint,
    pub lp_fee_wad: BigUint,
    pub dir_skew_wad: BigUint,
    pub inv_skew_kappa_wad: BigUint,
    pub inv_skew_band_wad: BigUint,
    pub vol_sigma_ref_wad: BigUint,
    pub vol_beta_wad: BigUint,
    pub vol_min_wad: BigUint,
    pub vol_max_wad: BigUint,
    /// `hotFeeFloorWad(dir, uint128::MAX)`: the FFAD floor at a SATURATED push rate, which
    /// is its ceiling at any live rate — the hook ramps it as `level * smoothstep(t)` and
    /// clamps there. A re-derived fee below it could be overridden upward on-chain, so the
    /// re-derivation declines there rather than over-quoting.
    pub floor_stable_in_wad: BigUint,
    pub floor_volatile_in_wad: BigUint,
}

/// The book the law is evaluated at: the words a fill moves, plus the two it does not.
pub struct Book<'a> {
    pub x_wad: &'a BigUint,
    pub reserve_stable: &'a BigUint,
    pub reserve_volatile: &'a BigUint,
    /// `CvammALM.reservationPriceWad()`, derived from the anchor sqrt price.
    pub reservation_price_wad: &'a BigUint,
    pub anchor_sqrt_x96: &'a BigUint,
    pub a_wad: &'a BigUint,
}

pub fn wad() -> BigUint {
    BigUint::from(10u128.pow(18))
}

pub fn half_wad() -> BigUint {
    wad() / 2u8
}

/// `floor(x * y / d)`, the rounding every multiply in the law uses.
pub fn mul_div_floor(x: &BigUint, y: &BigUint, d: &BigUint) -> BigUint {
    (x * y) / d
}

/// The directional and inventory-displacement product, given the volatile value weight and
/// whether the fill restores the price toward the anchor.
pub fn skew_multiplier(
    terms: &FeeLawTerms,
    volatile_weight: &BigUint,
    restoring: bool,
    stable_in: bool,
) -> BigUint {
    let half = half_wad();
    let mut multiplier =
        if restoring { wad() - &terms.dir_skew_wad } else { wad() + &terms.dir_skew_wad };

    // Paying stable removes volatile from the book, so it increases displacement only
    // below a 50% volatile value weight; paying volatile does the converse. This is
    // deliberately independent of the spot/anchor restoring test.
    let inventory_increasing =
        if stable_in { *volatile_weight < half } else { *volatile_weight > half };
    if inventory_increasing && !terms.inv_skew_kappa_wad.is_zero() {
        let abs_deviation =
            if *volatile_weight > half { volatile_weight - &half } else { &half - volatile_weight };
        if abs_deviation > terms.inv_skew_band_wad {
            multiplier += mul_div_floor(
                &terms.inv_skew_kappa_wad,
                &(abs_deviation - &terms.inv_skew_band_wad),
                &wad(),
            );
        }
    }
    multiplier
}

/// The volatile value weight and the multiplier at `book`, or `None` on a degenerate or
/// one-sided book — where the law returns the imbalanced fee rather than the capped base.
///
/// `restoring` is the price test `(spot < anchor) == stable_in`. Marginal price decreases
/// globally in the coordinate and the anchor sits at exactly `WAD/2`, so `spot < anchor` is
/// simply `x > WAD/2` — no curve evaluation needed.
fn skew_multiplier_wad(terms: &FeeLawTerms, book: &Book<'_>, stable_in: bool) -> Option<BigUint> {
    let volatile_value = mul_div_floor(book.reserve_volatile, book.reservation_price_wad, &wad());
    if book.reserve_stable.is_zero() || volatile_value.is_zero() {
        return None;
    }
    let total = book.reserve_stable + &volatile_value;
    let volatile_weight = mul_div_floor(&volatile_value, &wad(), &total);
    let restoring = (*book.x_wad > half_wad()) == stable_in;
    Some(skew_multiplier(terms, &volatile_weight, restoring, stable_in))
}

pub fn floor_for(terms: &FeeLawTerms, stable_in: bool) -> &BigUint {
    if stable_in {
        &terms.floor_stable_in_wad
    } else {
        &terms.floor_volatile_in_wad
    }
}

/// Bounds the fee at `book` from ABOVE, exactly.
///
/// The law is `fee = clamp(out + (mid-out)*g*v, out, mid) * multiplier`, then raised to the
/// hook floor. `g*v` carries realized variance and is unknowable off-chain — but the clamp
/// caps the base at `MidFee` whatever it does, so the fee can never exceed
/// `max(MidFee * multiplier, floor)`. When the base actually sits at that cap the bound IS
/// the fee, so the saturated regime prices exactly and the rest prices safely.
///
/// Returns `None` on the degenerate book the multiplier does not describe.
pub fn fee_upper_bound_wad(
    terms: &FeeLawTerms,
    book: &Book<'_>,
    stable_in: bool,
) -> Option<BigUint> {
    let floor = floor_for(terms, stable_in);
    if terms.curvature_wad.is_zero() {
        let fee = terms
            .lp_fee_wad
            .clone()
            .max(floor.clone());
        return Some(fee.min(wad()));
    }
    let multiplier = skew_multiplier_wad(terms, book, stable_in)?;
    let fee = mul_div_floor(&terms.mid_fee_wad, &multiplier, &wad()).min(wad());
    // The cap is applied AFTER the floor: on-chain a floor above WAD is discarded outright
    // (`floorWad > WAD` returns the base fee), so a floor that escaped the cap here would
    // price a fee the venue never charges — and a fee at or above WAD underflows the
    // caller's `gross - fee`.
    Some(fee.max(floor.clone()).min(wad()))
}

/// Reprices both legs at the POST-fill book through the bound.
///
/// Returns `None` when the bound fails to hold at the PRE-fill book, which means the
/// modelled law no longer describes the venue — a hook upgrade, a term we do not read — and
/// the caller must fall back rather than trust it.
pub fn re_sample_fees(
    terms: &FeeLawTerms,
    sampled_stable_in: &BigUint,
    sampled_volatile_in: &BigUint,
    pre: &Book<'_>,
    post: &Book<'_>,
) -> Option<(BigUint, BigUint)> {
    for (stable_in, sampled) in [(true, sampled_stable_in), (false, sampled_volatile_in)] {
        if fee_upper_bound_wad(terms, pre, stable_in)? < *sampled {
            return None;
        }
    }
    Some((fee_upper_bound_wad(terms, post, true)?, fee_upper_bound_wad(terms, post, false)?))
}
