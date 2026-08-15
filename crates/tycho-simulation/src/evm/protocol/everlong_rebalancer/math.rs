//! Pure-integer port of the deployed CollateralRebalancer curve ("CR math") plus the
//! CollateralRebalancerSwapper's token-leg preview:
//! - `CollRebalancerMath.leverageQuote` / `deleverageQuote` (strict branch AND the conservative
//!   missed-wall recovery continuation for deleverage),
//! - `isStateSafe` / `anchorBestEffort` (the fill-acceptance predicate applied on top of every
//!   quote),
//! - `Swapper._previewTokenAmounts` (CollVault ERC-4626 -> ALM proportional split),
//! - the composed `quoteLeverageAt` / `deleverageLegsAt` the swap legs settle by.
//!
//! Every function replicates Solidity semantics exactly: floor division, `Math.mulDiv`
//! with `Rounding.Up` = ceil, `Math.sqrt` = floor, and `Mul512.productGt` as
//! full-precision `a*b > c*d`. Arbitrary-precision integers are used because
//! intermediates exceed 256 bits (the contract uses a 512-bit library for them).
//! Validated wei-exact against the shipped library bytecode over the fixture grid in
//! `testdata/collvault_fixtures.csv` and fills settled by the deployed Berachain venue.
//!
//! The curve constants (wall anchors, Bézier controls, rescue spread) are STRATEGY
//! PARAMS frozen into each deployment — they arrive as component attributes, never as
//! code constants.

use num_bigint::BigUint;
use serde::{Deserialize, Serialize};

pub fn wad() -> BigUint {
    BigUint::from(10u128.pow(18))
}

fn wad_sq() -> BigUint {
    wad() * wad()
}

fn ppm() -> BigUint {
    BigUint::from(1_000_000u32)
}

fn bp() -> BigUint {
    BigUint::from(10_000u32)
}

fn max_input() -> BigUint {
    BigUint::from(10u8).pow(38)
}

fn icr_floor_factor() -> BigUint {
    BigUint::from(12u8) * BigUint::from(10u8).pow(17)
}

pub fn interest_ray() -> BigUint {
    BigUint::from(10u8).pow(27)
}

fn one() -> BigUint {
    BigUint::from(1u8)
}

fn big(n: u64) -> BigUint {
    BigUint::from(n)
}

/// The deployed CollateralRebalancer's frozen curve constants.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CurveParams {
    pub leverage_ratio_wad: BigUint,
    pub h_zero: BigUint,
    pub h_join: BigUint,
    pub h_wall: BigUint,
    pub width: BigUint,
    pub d_join: BigUint,
    pub d_wall: BigUint,
    pub rescue_spread_ppm: BigUint,
    /// P0..P3: phi = dD/dh cubic controls.
    pub bezier_phi: [BigUint; 4],
    /// Q0..Q4: exact quartic integral controls.
    pub bezier_integral: [BigUint; 5],
    /// PRE-fill floor capping the max leverage lot.
    pub physical_cr_floor_wad: BigUint,
}

/// The per-refresh rebalancer + vault snapshot. Zero-valued optional words mean
/// "not tracked" and degrade exactly as the venue's own legacy paths do.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct VaultState {
    pub collateral: BigUint,
    pub debt: BigUint,
    pub price_wad: BigUint,
    pub spread_ppm: BigUint,

    pub alm_stable_reserve: BigUint,
    pub alm_volatile_reserve: BigUint,
    pub alm_supply: BigUint,
    pub cv_total_assets: BigUint,
    pub cv_total_supply: BigUint,
    pub cv_decimals_offset: u8,
    pub withdraw_fee_bp: BigUint,

    /// Non-zero disables debt origination outright (every leverage fill reverts).
    pub interest_rate: BigUint,

    /// CDP repayment bound words; zero `min_net_debt` disables the bound.
    pub min_net_debt: BigUint,
    pub debt_gas_compensation: BigUint,

    /// Reference-marked physical reserves for the PHYSICAL_CR_FLOOR leverage cap.
    pub ref_stable_reserve: BigUint,
    pub ref_asset_reserve: BigUint,
    pub ref_raw_reference_wad: BigUint,

    /// Per-ALM-share reservation mark; zero falls back to the pre-fill reservation
    /// in the post-fill acceptance.
    pub rvps_wad: BigUint,

    /// CDP-health leg of the acceptance; zero skips the ICR check.
    pub mcr_wad: BigUint,
    pub icr_price_wad: BigUint,
}

// ---------- Solidity primitive equivalents ----------

/// `Math.mulDiv(x, y, d)`: floor(x*y/d).
pub fn mul_div(x: &BigUint, y: &BigUint, d: &BigUint) -> BigUint {
    (x * y) / d
}

/// `Math.mulDiv(x, y, d, Rounding.Up)`: ceil(x*y/d).
pub fn mul_div_up(x: &BigUint, y: &BigUint, d: &BigUint) -> BigUint {
    let p = x * y;
    let (q, r) = (&p / d, &p % d);
    if r == BigUint::ZERO {
        q
    } else {
        q + one()
    }
}

/// `Mul512.productGt`: `a*b > c*d` at full precision.
fn product_gt(a: &BigUint, b: &BigUint, c: &BigUint, d: &BigUint) -> bool {
    a * b > c * d
}

/// De Casteljau lerp step: floor toward increases, ceil-subtract on decreases —
/// exactly `CollRebalancerMath._lerpFloor`.
fn lerp_floor(a: &BigUint, b: &BigUint, x: &BigUint) -> BigUint {
    if b >= a {
        a + mul_div(&(b - a), x, &wad())
    } else {
        a - mul_div_up(&(a - b), x, &wad())
    }
}

fn checked_sub(a: &BigUint, b: &BigUint) -> Option<BigUint> {
    if a >= b {
        Some(a - b)
    } else {
        None
    }
}

// ---------- Marked value & anchor ----------

/// `cv = floor(collateral*price/WAD)`; `None` on overflow/out-of-bounds.
fn marked_value(collateral: &BigUint, price: &BigUint) -> Option<BigUint> {
    if *price == BigUint::ZERO {
        return None;
    }
    // The contract rejects when the 512-bit product's high word reaches WAD<<256.
    let limit = wad() << 256;
    if collateral * price >= limit {
        return None;
    }
    let cv = mul_div(collateral, price, &wad());
    (cv <= max_input()).then_some(cv)
}

fn root_interval_contains(cv: &BigUint, debt: &BigUint, anchor: &BigUint) -> bool {
    let s = anchor + debt;
    let s2 = &s * &s;
    let a8 = big(8) * anchor;
    !product_gt(&big(3), &s2, &a8, cv)
}

fn half_law_anchor(cv: &BigUint, debt: &BigUint) -> BigUint {
    let two_cv = big(2) * cv;
    let three_debt = big(3) * debt;
    if three_debt > two_cv {
        return BigUint::ZERO;
    }
    let root_arg = &two_cv * (&two_cv - &three_debt);
    let root = root_arg.sqrt();
    let anchor = (big(8) * cv - big(6) * debt + big(4) * root) / big(6);
    let plus_one = &anchor + one();
    if root_interval_contains(cv, debt, &plus_one) {
        plus_one
    } else {
        anchor
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Region {
    Strict,
    /// Past the honest CR wall: only the conservative deleverage continuation
    /// quotes here; leverage is strict-only by construction.
    Recovery,
    Out,
}

impl CurveParams {
    /// Whether every word the quote path divides by or interpolates through is sane.
    ///
    /// A zero `width` divides by zero on the first quote, and the wall ordering is what
    /// keeps the recovery continuation's square root non-negative. The Rust decode cannot
    /// produce a missing word the way a nil pointer can, so this is the value bar alone —
    /// but it is the same bar, and it is checked before anything is quoted rather than
    /// discovered inside the curve.
    pub fn usable(&self) -> bool {
        self.width > BigUint::ZERO &&
            self.h_join > BigUint::ZERO &&
            self.h_wall > BigUint::ZERO &&
            self.d_wall <= self.h_wall
    }

    fn debt_norm_wad(&self, h: &BigUint) -> BigUint {
        if h <= &self.h_join {
            let root = (h * wad()).sqrt();
            let two_root = big(2) * root;
            return two_root - BigUint::from(1_500_000_000_000_000_000u64);
        }
        let x = h - &self.h_join;
        let x_norm = mul_div(&x, &wad(), &self.width);
        &self.d_join + mul_div(&self.width, &self.bezier4(&x_norm), &wad())
    }

    /// `strictAnchor` -> (anchor, h, half_law); `None` = infeasible.
    #[allow(clippy::type_complexity)]
    fn strict_anchor(
        &self,
        cv: &BigUint,
        debt: &BigUint,
        r_wad: &BigUint,
    ) -> Option<(BigUint, BigUint, bool)> {
        if *r_wad != self.leverage_ratio_wad || *cv > max_input() || *debt > max_input() {
            return None;
        }
        if *cv == BigUint::ZERO {
            return (*debt == BigUint::ZERO).then_some((BigUint::ZERO, BigUint::ZERO, true));
        }
        if product_gt(debt, &self.h_wall, cv, &self.d_wall) {
            return None;
        }
        if !product_gt(debt, &self.h_join, cv, &self.d_join) {
            let anchor = half_law_anchor(cv, debt);
            return (anchor != BigUint::ZERO).then_some((anchor, BigUint::ZERO, true));
        }
        let mut lo = self.h_join.clone();
        let mut hi = self.h_wall.clone();
        while lo < hi {
            let mid = (&lo + &hi) >> 1;
            if product_gt(debt, &mid, cv, &self.debt_norm_wad(&mid)) {
                lo = &mid + one();
            } else {
                hi = mid;
            }
        }
        let three_cv = big(3) * cv;
        let two_h = big(2) * &lo;
        let anchor = mul_div(&three_cv, &wad(), &two_h);
        (anchor != BigUint::ZERO).then_some((anchor, lo, false))
    }

    // ---------- Normalized curve (Bézier) ----------

    fn bezier4(&self, x: &BigUint) -> BigUint {
        let q = &self.bezier_integral;
        let a0 = lerp_floor(&q[0], &q[1], x);
        let a1 = lerp_floor(&q[1], &q[2], x);
        let a2 = lerp_floor(&q[2], &q[3], x);
        let a3 = lerp_floor(&q[3], &q[4], x);
        let b0 = lerp_floor(&a0, &a1, x);
        let b1 = lerp_floor(&a1, &a2, x);
        let b2 = lerp_floor(&a2, &a3, x);
        let c0 = lerp_floor(&b0, &b1, x);
        let c1 = lerp_floor(&b1, &b2, x);
        lerp_floor(&c0, &c1, x)
    }

    fn bezier3(&self, x: &BigUint) -> BigUint {
        let p = &self.bezier_phi;
        let a0 = lerp_floor(&p[0], &p[1], x);
        let a1 = lerp_floor(&p[1], &p[2], x);
        let a2 = lerp_floor(&p[2], &p[3], x);
        let b0 = lerp_floor(&a0, &a1, x);
        let b1 = lerp_floor(&a1, &a2, x);
        lerp_floor(&b0, &b1, x)
    }

    fn phi_wad(&self, h: &BigUint) -> BigUint {
        if h <= &self.h_join {
            let root = (h * wad()).sqrt();
            return wad_sq() / root;
        }
        let x = h - &self.h_join;
        self.bezier3(&mul_div(&x, &wad(), &self.width))
    }

    /// `anchorAndBase` -> (xAnchor, baseX): the value anchor and the stable-value
    /// marginal numerator M the post-fill acceptance is measured on.
    fn anchor_and_base(
        &self,
        collateral: &BigUint,
        debt: &BigUint,
        price: &BigUint,
    ) -> (BigUint, BigUint) {
        let zero = (BigUint::ZERO, BigUint::ZERO);
        let Some(cv) = marked_value(collateral, price) else {
            return zero;
        };
        match self.strict_anchor(&cv, debt, &self.leverage_ratio_wad.clone()) {
            Some((anchor, h, half_law)) => {
                if anchor == BigUint::ZERO {
                    return zero;
                }
                if half_law {
                    let base_x = (&anchor + debt) / big(2);
                    return (anchor, base_x);
                }
                let base_x = mul_div(&cv, &self.phi_wad(&h), &wad());
                (anchor, base_x)
            }
            None => {
                let rec = self.recovery_state_for(&cv, debt);
                match rec {
                    Some(rec) => (rec.anchor, rec.base_x),
                    None => zero,
                }
            }
        }
    }

    // ---------- Debt-cap / cv-required on an anchor ----------

    fn debt_cap_on_anchor(&self, anchor: &BigUint, cv: &BigUint) -> Option<BigUint> {
        let three_cv = big(3) * cv;
        let two_anchor = big(2) * anchor;
        let h = mul_div(&three_cv, &wad(), &two_anchor);
        if h < self.h_zero || h > self.h_wall {
            return None;
        }
        if h <= self.h_join {
            let eight_anchor = big(8) * anchor;
            let root = mul_div(&eight_anchor, cv, &big(3)).sqrt();
            if root <= *anchor {
                return None;
            }
            return Some(root - anchor);
        }
        Some(mul_div(cv, &self.debt_norm_wad(&h), &h))
    }

    fn cv_required_on_anchor(&self, anchor: &BigUint, debt: &BigUint) -> Option<BigUint> {
        let three_debt = big(3) * debt;
        let two_anchor = big(2) * anchor;
        if !product_gt(&three_debt, &wad(), &two_anchor, &self.d_join) {
            let s = anchor + debt;
            let s2 = &s * &s;
            let eight_anchor = big(8) * anchor;
            return Some(mul_div_up(&big(3), &s2, &eight_anchor));
        }
        if product_gt(&three_debt, &wad(), &two_anchor, &self.d_wall) {
            return None;
        }
        let mut lo = self.h_join.clone();
        let mut hi = self.h_wall.clone();
        while lo < hi {
            let mid = (&lo + &hi) >> 1;
            if product_gt(&three_debt, &wad(), &two_anchor, &self.debt_norm_wad(&mid)) {
                lo = &mid + one();
            } else {
                hi = mid;
            }
        }
        let three_wad = big(3) * wad();
        Some(mul_div_up(&two_anchor, &lo, &three_wad))
    }

    fn deleverage_spread(&self, cv: &BigUint, debt: &BigUint, posted: &BigUint) -> BigUint {
        let half = (cv + one()) / big(2);
        if *debt >= half && *posted > self.rescue_spread_ppm {
            return self.rescue_spread_ppm.clone();
        }
        posted.clone()
    }

    #[allow(clippy::too_many_arguments)]
    fn post_strict_anchor_accepted(
        &self,
        pre_anchor: &BigUint,
        collateral: &BigUint,
        debt: &BigUint,
        price: &BigUint,
        r_wad: &BigUint,
        spread: &BigUint,
    ) -> bool {
        let Some(cv) = marked_value(collateral, price) else {
            return false;
        };
        let Some((post_anchor, _, _)) = self.strict_anchor(&cv, debt, r_wad) else {
            return false;
        };
        if cv != BigUint::ZERO && post_anchor == BigUint::ZERO {
            return false;
        }
        if *spread == BigUint::ZERO {
            post_anchor >= *pre_anchor
        } else {
            post_anchor > *pre_anchor
        }
    }

    // ---------- Region tracking ----------

    pub fn state_region(&self, collateral: &BigUint, debt: &BigUint, price: &BigUint) -> Region {
        let Some(cv) = marked_value(collateral, price) else {
            return Region::Out;
        };
        if cv == BigUint::ZERO {
            return Region::Out;
        }
        if product_gt(debt, &self.h_wall, &cv, &self.d_wall) {
            return Region::Recovery;
        }
        match self.strict_anchor(&cv, debt, &self.leverage_ratio_wad.clone()) {
            Some((anchor, _, _)) if anchor != BigUint::ZERO => Region::Strict,
            _ => Region::Out,
        }
    }

    // ---------- Public quotes (strict branch) ----------

    /// `leverageQuote` -> (stableOut, newCollateral, newDebt); stableOut == 0 means
    /// the fill is rejected.
    pub fn leverage_quote(
        &self,
        collateral: &BigUint,
        debt: &BigUint,
        price: &BigUint,
        r_wad: &BigUint,
        spread: &BigUint,
        collateral_in: &BigUint,
    ) -> (BigUint, BigUint, BigUint) {
        let reject = || (BigUint::ZERO, collateral.clone(), debt.clone());
        if *collateral == BigUint::ZERO ||
            *price == BigUint::ZERO ||
            *r_wad != self.leverage_ratio_wad ||
            *spread >= ppm() ||
            *collateral_in == BigUint::ZERO ||
            *collateral_in > max_input() ||
            *debt > max_input()
        {
            return reject();
        }
        let Some(cv) = marked_value(collateral, price) else {
            return reject();
        };
        if cv == BigUint::ZERO {
            return reject();
        }
        let Some((anchor, _, _)) = self.strict_anchor(&cv, debt, r_wad) else {
            return reject();
        };
        if anchor == BigUint::ZERO {
            return reject();
        }
        let new_collateral = collateral + collateral_in;
        let Some(new_cv) = marked_value(&new_collateral, price) else {
            return reject();
        };
        if new_cv <= cv {
            return reject();
        }
        let Some(debt_cap) = self.debt_cap_on_anchor(&anchor, &new_cv) else {
            return reject();
        };
        if debt_cap <= *debt {
            return reject();
        }
        let gross_out = &debt_cap - debt;
        let keep = ppm() - spread;
        let stable_out = mul_div(&gross_out, &keep, &ppm());
        if stable_out == BigUint::ZERO {
            return reject();
        }
        let new_debt = debt + &stable_out;
        if !self.post_strict_anchor_accepted(
            &anchor,
            &new_collateral,
            &new_debt,
            price,
            r_wad,
            spread,
        ) {
            return reject();
        }
        (stable_out, new_collateral, new_debt)
    }

    /// `deleverageQuote` -> (collateralOut, newCollateral, newDebt); collateralOut
    /// == 0 means the fill is rejected. A solvent state beyond the honest wall
    /// quotes along its conservative recovery continuation (deleverage only).
    pub fn deleverage_quote(
        &self,
        collateral: &BigUint,
        debt: &BigUint,
        price: &BigUint,
        r_wad: &BigUint,
        spread: &BigUint,
        stable_in: &BigUint,
    ) -> (BigUint, BigUint, BigUint) {
        let reject = || (BigUint::ZERO, collateral.clone(), debt.clone());
        if *collateral == BigUint::ZERO ||
            *price == BigUint::ZERO ||
            *r_wad != self.leverage_ratio_wad ||
            *spread >= ppm() ||
            *stable_in == BigUint::ZERO ||
            stable_in > debt ||
            *stable_in > max_input() ||
            *debt > max_input()
        {
            return reject();
        }
        let Some(cv) = marked_value(collateral, price) else {
            return reject();
        };
        if cv == BigUint::ZERO {
            return reject();
        }
        let strict = self.strict_anchor(&cv, debt, r_wad);
        let Some((anchor, _, _)) = strict else {
            return self.recovery_deleverage(collateral, debt, &cv, price, spread, stable_in);
        };
        if anchor == BigUint::ZERO {
            return self.recovery_deleverage(collateral, debt, &cv, price, spread, stable_in);
        }
        let new_debt = debt - stable_in;
        let Some(cv_required) = self.cv_required_on_anchor(&anchor, &new_debt) else {
            return reject();
        };
        let collateral_required = mul_div_up(&cv_required, &wad(), price);
        if collateral_required >= *collateral {
            return reject();
        }
        let out_gross = collateral - &collateral_required;
        let eff_spread = self.deleverage_spread(&cv, debt, spread);
        let keep = ppm() - &eff_spread;
        let collateral_out = mul_div(&out_gross, &keep, &ppm());
        if collateral_out == BigUint::ZERO {
            return reject();
        }
        let new_collateral = collateral - &collateral_out;
        if !self.post_strict_anchor_accepted(
            &anchor,
            &new_collateral,
            &new_debt,
            price,
            r_wad,
            &eff_spread,
        ) {
            return reject();
        }
        (collateral_out, new_collateral, new_debt)
    }

    // ---------- Conservative recovery continuation (missed-wall deleverage) ----------

    fn recovery_state_for(&self, cv: &BigUint, debt: &BigUint) -> Option<RecoveryState> {
        if *cv == BigUint::ZERO ||
            *cv > max_input() ||
            *debt > max_input() ||
            debt >= cv ||
            !product_gt(debt, &self.h_wall, cv, &self.d_wall)
        {
            return None;
        }
        let numerator = debt * &self.h_wall - cv * &self.d_wall;
        let hw_minus_dw = &self.h_wall - &self.d_wall;
        let denominator = cv * &hw_minus_dw;
        let y = mul_div(&numerator, &wad_sq(), &denominator).sqrt();
        if y == BigUint::ZERO || y >= wad() {
            return None;
        }
        let wad_minus_y = wad() - &y;
        let wad_plus_y = wad() + &y;
        let wall_cv = mul_div(cv, &wad_minus_y, &wad_plus_y);
        if wall_cv == BigUint::ZERO {
            return None;
        }
        let wall_debt = mul_div(&wall_cv, &self.d_wall, &self.h_wall);
        if wall_debt >= *debt {
            return None;
        }
        let (wall_anchor, _, _) =
            self.strict_anchor(&wall_cv, &wall_debt, &self.leverage_ratio_wad.clone())?;
        if wall_anchor == BigUint::ZERO {
            return None;
        }
        let cv_minus_debt = cv - debt;
        let base_x = debt + mul_div(&cv_minus_debt, &y, &wad());
        let stable_to_wall = debt - &wall_debt;
        if base_x == BigUint::ZERO || stable_to_wall == BigUint::ZERO {
            return None;
        }
        Some(RecoveryState { anchor: wall_anchor, base_x, y, wall_cv, stable_to_wall })
    }

    fn recovery_debt_at_y(&self, wall_cv: &BigUint, y: &BigUint) -> BigUint {
        let z = if *y == BigUint::ZERO {
            wall_cv.clone()
        } else {
            let wad_plus_y = wad() + y;
            let wad_minus_y = wad() - y;
            mul_div_up(wall_cv, &wad_plus_y, &wad_minus_y)
        };
        // rhoNumerator = D_WALL*WAD^2 + (H_WALL - D_WALL)*y^2
        let rho_num = &self.d_wall * wad_sq() + (&self.h_wall - &self.d_wall) * (y * y);
        let den = &self.h_wall * wad_sq();
        mul_div(&z, &rho_num, &den)
    }

    fn recovery_deleverage(
        &self,
        collateral: &BigUint,
        debt: &BigUint,
        cv: &BigUint,
        price: &BigUint,
        spread: &BigUint,
        stable_in: &BigUint,
    ) -> (BigUint, BigUint, BigUint) {
        let reject = || (BigUint::ZERO, collateral.clone(), debt.clone());
        let Some(rec) = self.recovery_state_for(cv, debt) else {
            return reject();
        };
        if stable_in > &rec.stable_to_wall {
            return reject();
        }
        let new_debt = debt - stable_in;
        let mut y_new = BigUint::ZERO;
        if *stable_in != rec.stable_to_wall {
            let mut lo = BigUint::ZERO;
            let wad_minus_one = wad() - one();
            let mut hi = if rec.y < wad_minus_one { &rec.y + one() } else { rec.y.clone() };
            if self.recovery_debt_at_y(&rec.wall_cv, &hi) < new_debt {
                return reject();
            }
            while lo < hi {
                let mid = (&lo + &hi) >> 1;
                if self.recovery_debt_at_y(&rec.wall_cv, &mid) < new_debt {
                    lo = &mid + one();
                } else {
                    hi = mid;
                }
            }
            y_new = lo;
        }
        let invariant_cv = if y_new == BigUint::ZERO {
            rec.wall_cv.clone()
        } else {
            let wad_plus_y = wad() + &y_new;
            let wad_minus_y = wad() - &y_new;
            mul_div_up(&rec.wall_cv, &wad_plus_y, &wad_minus_y)
        };
        let collateral_required = mul_div_up(&invariant_cv, &wad(), price);
        if collateral_required >= *collateral {
            return reject();
        }
        let out_gross = collateral - &collateral_required;
        let eff_spread = self.deleverage_spread(cv, debt, spread);
        let keep = ppm() - &eff_spread;
        let collateral_out = mul_div(&out_gross, &keep, &ppm());
        if collateral_out == BigUint::ZERO {
            return reject();
        }
        let new_collateral = collateral - &collateral_out;
        if !self.post_any_anchor_accepted(
            &rec.anchor,
            &new_collateral,
            &new_debt,
            price,
            &eff_spread,
        ) {
            return reject();
        }
        (collateral_out, new_collateral, new_debt)
    }

    fn anchor_best_effort(&self, cv: &BigUint, debt: &BigUint) -> BigUint {
        if let Some((anchor, _, _)) = self.strict_anchor(cv, debt, &self.leverage_ratio_wad.clone())
        {
            return anchor;
        }
        match self.recovery_state_for(cv, debt) {
            Some(rec) => rec.anchor,
            None => BigUint::ZERO,
        }
    }

    fn post_any_anchor_accepted(
        &self,
        pre_anchor: &BigUint,
        collateral: &BigUint,
        debt: &BigUint,
        price: &BigUint,
        spread: &BigUint,
    ) -> bool {
        let Some(cv) = marked_value(collateral, price) else {
            return false;
        };
        let post_anchor = self.anchor_best_effort(&cv, debt);
        if post_anchor == BigUint::ZERO {
            return false;
        }
        if *spread == BigUint::ZERO {
            post_anchor >= *pre_anchor
        } else {
            post_anchor > *pre_anchor
        }
    }

    /// `isStateSafe`: the fill-acceptance predicate the rebalancer applies on top of
    /// every quote.
    pub fn is_state_safe(
        &self,
        collateral: &BigUint,
        debt: &BigUint,
        price: &BigUint,
        required_x_anchor: &BigUint,
    ) -> bool {
        let Some(cv) = marked_value(collateral, price) else {
            return false;
        };
        if cv == BigUint::ZERO {
            return *collateral == BigUint::ZERO &&
                *debt == BigUint::ZERO &&
                *required_x_anchor == BigUint::ZERO;
        }
        if let Some((anchor, _, _)) =
            self.strict_anchor(&cv, debt, &self.leverage_ratio_wad.clone())
        {
            return anchor > BigUint::ZERO && anchor >= *required_x_anchor;
        }
        match self.recovery_state_for(&cv, debt) {
            Some(rec) => rec.anchor >= *required_x_anchor,
            None => false,
        }
    }

    /// The pre-fill anchor the rebalancer holds fills against.
    pub fn x_anchor_for_state(
        &self,
        collateral: &BigUint,
        debt: &BigUint,
        price: &BigUint,
    ) -> BigUint {
        match marked_value(collateral, price) {
            Some(cv) => self.anchor_best_effort(&cv, debt),
            None => BigUint::ZERO,
        }
    }

    // ---------- Post-fill acceptance (_assertFillSafe) ----------

    /// `_assertFillSafe`, the post-EXECUTION predicate applied on top of every quote.
    fn fill_outcome_accepted(
        &self,
        s: &VaultState,
        is_leverage: bool,
        shares: &BigUint,
        new_coll: &BigUint,
        new_debt: &BigUint,
    ) -> bool {
        let (post_assets, post_supply) = if is_leverage {
            s.post_vault_leverage(shares)
        } else {
            s.post_vault_deleverage(shares)
        };
        let res_post = s.post_reservation(new_coll, &post_assets, &post_supply);

        let pre_anchor = self.x_anchor_for_state(&s.collateral, &s.debt, &s.price_wad);
        if !self.is_state_safe(new_coll, new_debt, &res_post, &pre_anchor) {
            return false;
        }
        let (x_after, base_x_after) = self.anchor_and_base(new_coll, new_debt, &res_post);
        if x_after < pre_anchor || base_x_after == BigUint::ZERO {
            return false;
        }
        if *new_coll == BigUint::ZERO {
            return false;
        }
        let internal_value = mul_div(&base_x_after, &wad(), new_coll);
        let upper = &res_post * big(2);
        let lower = &res_post / big(2);
        if internal_value > upper || internal_value < lower {
            return false;
        }

        if s.icr_price_wad > BigUint::ZERO && s.mcr_wad > BigUint::ZERO {
            if let Some(icr_after) = compute_cr(new_coll, new_debt, &s.icr_price_wad) {
                let floor_cr = mul_div(&s.mcr_wad, &icr_floor_factor(), &wad());
                if icr_after < floor_cr {
                    // A pre-fill zero debt means infinite ICR, which always exceeds.
                    match compute_cr(&s.collateral, &s.debt, &s.icr_price_wad) {
                        None => return false,
                        Some(icr_before) => {
                            if icr_after < icr_before {
                                return false;
                            }
                        }
                    }
                }
            }
        }
        true
    }

    // ---------- Composite quotes (the values swap legs settle by) ----------

    /// Net stable the caller receives for minting `shares_in` CollVault shares —
    /// gross debt draw minus the stable leg pulled to mint the shares.
    pub fn quote_leverage_at(&self, s: &VaultState, shares_in: &BigUint) -> Option<BigUint> {
        let (gross_stable_out, _, _) = self.leverage_quote_checked(s, shares_in);
        let (stable_in, _volatile, ok) = s.preview_token_amounts(shares_in, true);
        if !ok {
            return None;
        }
        if gross_stable_out > stable_in {
            Some(&gross_stable_out - &stable_in)
        } else {
            Some(BigUint::ZERO)
        }
    }

    /// Wraps `leverage_quote` with the rebalancer's fill-acceptance predicate; a zero
    /// first return means the fill is rejected.
    pub fn leverage_quote_checked(
        &self,
        s: &VaultState,
        collateral_in: &BigUint,
    ) -> (BigUint, BigUint, BigUint) {
        let (out, new_coll, new_debt) = self.leverage_quote(
            &s.collateral,
            &s.debt,
            &s.price_wad,
            &self.leverage_ratio_wad.clone(),
            &s.spread_ppm,
            collateral_in,
        );
        if out == BigUint::ZERO {
            return (out, new_coll, new_debt);
        }
        let pre_anchor = self.x_anchor_for_state(&s.collateral, &s.debt, &s.price_wad);
        if !self.is_state_safe(&new_coll, &new_debt, &s.price_wad, &pre_anchor) {
            return (BigUint::ZERO, s.collateral.clone(), s.debt.clone());
        }
        if !self.fill_outcome_accepted(s, true, collateral_in, &new_coll, &new_debt) {
            return (BigUint::ZERO, s.collateral.clone(), s.debt.clone());
        }
        (out, new_coll, new_debt)
    }

    pub fn deleverage_quote_checked(
        &self,
        s: &VaultState,
        stable_in: &BigUint,
    ) -> (BigUint, BigUint, BigUint) {
        let (out, new_coll, new_debt) = self.deleverage_quote(
            &s.collateral,
            &s.debt,
            &s.price_wad,
            &self.leverage_ratio_wad.clone(),
            &s.spread_ppm,
            stable_in,
        );
        if out == BigUint::ZERO {
            return (out, new_coll, new_debt);
        }
        let pre_anchor = self.x_anchor_for_state(&s.collateral, &s.debt, &s.price_wad);
        if !self.is_state_safe(&new_coll, &new_debt, &s.price_wad, &pre_anchor) {
            return (BigUint::ZERO, s.collateral.clone(), s.debt.clone());
        }
        if !self.fill_outcome_accepted(s, false, &out, &new_coll, &new_debt) {
            return (BigUint::ZERO, s.collateral.clone(), s.debt.clone());
        }
        (out, new_coll, new_debt)
    }

    /// Both freed legs at a gross `stable_debt_in` -> (stableOut, volatileOut);
    /// `None` when the fill is rejected or the preview degenerates.
    pub fn deleverage_legs_at(
        &self,
        s: &VaultState,
        stable_debt_in: &BigUint,
    ) -> Option<(BigUint, BigUint)> {
        let (shares_out, _, _) = self.deleverage_quote_checked(s, stable_debt_in);
        if shares_out == BigUint::ZERO {
            return None;
        }
        let (stable, volatile, ok) = s.preview_token_amounts(&shares_out, false);
        ok.then_some((stable, volatile))
    }

    // ---------- Max-lot sizing ----------

    /// A leverage fill of `collateral_in` quotes, passes the fill-acceptance
    /// predicate AND clears the physical-CR floor evaluated on the POST-fill book.
    fn leverage_ok(
        &self,
        s: &VaultState,
        collateral_in: &BigUint,
        total_physical_value: &BigUint,
    ) -> bool {
        let (out, new_coll, new_debt) = self.leverage_quote_checked(s, collateral_in);
        if out == BigUint::ZERO {
            return false;
        }
        if new_debt == BigUint::ZERO {
            return true;
        }
        let alm_shares_minted = s.cv_convert_to_assets(collateral_in, true);
        let mut post = s.clone();
        post.cv_total_assets = &s.cv_total_assets + &alm_shares_minted;
        post.cv_total_supply = &s.cv_total_supply + collateral_in;
        let alm_shares = post.cv_convert_to_assets(&new_coll, false);
        let position_physical_value = mul_div(total_physical_value, &alm_shares, &s.alm_supply);
        let required_value = mul_div_up(&new_debt, &self.physical_cr_floor_wad, &wad());
        position_physical_value >= required_value
    }

    /// Largest `collateral_in` that fills and clears the physical-CR floor.
    pub fn max_leverage_shares(&self, s: &VaultState) -> BigUint {
        if s.alm_supply == BigUint::ZERO || s.ref_raw_reference_wad == BigUint::ZERO {
            return BigUint::ZERO;
        }
        let total_physical_value =
            &s.ref_stable_reserve + mul_div(&s.ref_asset_reserve, &s.ref_raw_reference_wad, &wad());
        if !self.leverage_ok(s, &one(), &total_physical_value) {
            return BigUint::ZERO;
        }
        let mut lo = one();
        let mut hi = mul_div(&max_input(), &wad(), &s.price_wad);
        while lo < hi {
            let mid = &lo + (&hi - &lo + one()) / big(2);
            if self.leverage_ok(s, &mid, &total_physical_value) {
                lo = mid;
            } else {
                hi = &mid - one();
            }
        }
        lo
    }

    /// Largest gross `stable_in` that still quotes, passes acceptance AND stays
    /// within the CDP's minimum-net-debt bound.
    pub fn max_deleverage_in(&self, s: &VaultState) -> BigUint {
        if s.debt == BigUint::ZERO {
            return BigUint::ZERO;
        }
        let ceiling = s.debt_repay_ceiling();
        if ceiling == BigUint::ZERO {
            return BigUint::ZERO;
        }
        let mut lo = one();
        let mut hi = ceiling;
        while lo < hi {
            let mid = &lo + (&hi - &lo + one()) / big(2);
            let (out, _, _) = self.deleverage_quote_checked(s, &mid);
            if out != BigUint::ZERO {
                lo = mid;
            } else {
                hi = &mid - one();
            }
        }
        let (out, _, _) = self.deleverage_quote_checked(s, &lo);
        if out == BigUint::ZERO {
            return BigUint::ZERO;
        }
        lo
    }

    /// Largest gross `stable_debt_in` in `[1, max_gross]` whose NET front
    /// (gross - freed stable) fits within `net_budget`.
    pub fn gross_for_net_stable_in(
        &self,
        s: &VaultState,
        net_budget: &BigUint,
        max_gross: &BigUint,
    ) -> BigUint {
        if *net_budget == BigUint::ZERO || *max_gross == BigUint::ZERO {
            return BigUint::ZERO;
        }
        let fits_net = |gross: &BigUint| -> bool {
            match self.deleverage_legs_at(s, gross) {
                // Only tiny grosses are invalid below max_gross; net <= gross bounds them.
                None => gross <= net_budget,
                Some((stable_out, _)) => {
                    let net = gross - &stable_out;
                    net <= *net_budget
                }
            }
        };
        let mut lo = one();
        let mut hi = max_gross.clone();
        while lo < hi {
            let mid = &lo + (&hi - &lo + one()) / big(2);
            if fits_net(&mid) {
                lo = mid;
            } else {
                hi = &mid - one();
            }
        }
        if self
            .deleverage_legs_at(s, &lo)
            .is_none()
        {
            return BigUint::ZERO;
        }
        lo
    }
}

struct RecoveryState {
    anchor: BigUint,
    base_x: BigUint,
    y: BigUint,
    wall_cv: BigUint,
    stable_to_wall: BigUint,
}

impl VaultState {
    // ---------- previewTokenAmounts (CollVault 4626 -> ALM proportional split) ----------

    /// `ERC4626Upgradeable._convertToAssets`:
    /// `shares*(totalAssets+1)/(totalSupply+10^offset)` with the given rounding.
    pub fn cv_convert_to_assets(&self, shares: &BigUint, up: bool) -> BigUint {
        let num = &self.cv_total_assets + one();
        let den = &self.cv_total_supply + BigUint::from(10u8).pow(self.cv_decimals_offset as u32);
        if up {
            mul_div_up(shares, &num, &den)
        } else {
            mul_div(shares, &num, &den)
        }
    }

    /// `Swapper._previewTokenAmounts` -> (stable, volatile, ok); `ok == false` is
    /// the NothingToFill revert.
    pub fn preview_token_amounts(
        &self,
        coll_vault_shares: &BigUint,
        mint: bool,
    ) -> (BigUint, BigUint, bool) {
        let alm_shares = if mint {
            self.cv_convert_to_assets(coll_vault_shares, true) // previewMint
        } else {
            let share_fee = mul_div_up(coll_vault_shares, &self.withdraw_fee_bp, &bp());
            let net = coll_vault_shares - &share_fee;
            self.cv_convert_to_assets(&net, false) // previewRedeem
        };
        if self.alm_supply == BigUint::ZERO || alm_shares == BigUint::ZERO {
            return (BigUint::ZERO, BigUint::ZERO, false);
        }
        if mint {
            (
                mul_div_up(&self.alm_stable_reserve, &alm_shares, &self.alm_supply),
                mul_div_up(&self.alm_volatile_reserve, &alm_shares, &self.alm_supply),
                true,
            )
        } else {
            (
                mul_div(&self.alm_stable_reserve, &alm_shares, &self.alm_supply),
                mul_div(&self.alm_volatile_reserve, &alm_shares, &self.alm_supply),
                true,
            )
        }
    }

    /// The CollVault's post-fill (totalAssets, totalSupply) for a leverage mint.
    pub fn post_vault_leverage(&self, shares_in: &BigUint) -> (BigUint, BigUint) {
        (
            &self.cv_total_assets + self.cv_convert_to_assets(shares_in, true),
            &self.cv_total_supply + shares_in,
        )
    }

    /// Deleverage burns only the NET shares (the fee shares re-mint to the fee
    /// receiver) and moves assets by the raw ratio (no ERC-4626 virtual offsets).
    pub fn post_vault_deleverage(&self, shares_out: &BigUint) -> (BigUint, BigUint) {
        let share_fee = mul_div_up(shares_out, &self.withdraw_fee_bp, &bp());
        let net_shares = shares_out - &share_fee;
        let asset_delta = if self.cv_total_supply == BigUint::ZERO {
            BigUint::ZERO
        } else {
            mul_div(&net_shares, &self.cv_total_assets, &self.cv_total_supply)
        };
        (&self.cv_total_assets - &asset_delta, &self.cv_total_supply - &net_shares)
    }

    /// The reservation value the venue would report after the fill, or the pre-fill
    /// value when rvps is not tracked.
    pub fn post_reservation(
        &self,
        new_coll: &BigUint,
        post_assets: &BigUint,
        post_supply: &BigUint,
    ) -> BigUint {
        if self.rvps_wad == BigUint::ZERO {
            return self.price_wad.clone();
        }
        reservation_value_at(
            new_coll,
            post_assets,
            post_supply,
            self.cv_decimals_offset,
            &self.rvps_wad,
        )
    }

    /// The largest repayment the CDP itself accepts: `debt - gasComp - minNetDebt`,
    /// or the full debt when the deployment does not report the bound words.
    pub fn debt_repay_ceiling(&self) -> BigUint {
        if self.min_net_debt == BigUint::ZERO {
            return self.debt.clone();
        }
        let floor = &self.debt_gas_compensation + &self.min_net_debt;
        match checked_sub(&self.debt, &floor) {
            Some(ceiling) => ceiling,
            None => BigUint::ZERO,
        }
    }

    /// Largest `shares_in` whose mint-leg volatile requirement fits within
    /// `amount_in`, additionally capped by `max_shares`.
    pub fn shares_for_volatile_in(&self, amount_in: &BigUint, max_shares: &BigUint) -> BigUint {
        if *amount_in == BigUint::ZERO || *max_shares == BigUint::ZERO {
            return BigUint::ZERO;
        }
        let (_, vol_at_one, ok) = self.preview_token_amounts(&one(), true);
        if !ok || vol_at_one > *amount_in {
            return BigUint::ZERO;
        }
        let mut lo = one();
        let mut hi = max_shares.clone();
        while lo < hi {
            let mid = &lo + (&hi - &lo + one()) / big(2);
            let (_, vol, ok) = self.preview_token_amounts(&mid, true);
            if ok && vol <= *amount_in {
                lo = mid;
            } else {
                hi = &mid - one();
            }
        }
        lo
    }
}

/// `CollateralRebalancer._reservationValue` on the given vault words.
fn reservation_value_at(
    coll_vault_shares: &BigUint,
    cv_total_assets: &BigUint,
    cv_total_supply: &BigUint,
    decimals_offset: u8,
    rvps_wad: &BigUint,
) -> BigUint {
    if *coll_vault_shares == BigUint::ZERO {
        return BigUint::ZERO;
    }
    let num = cv_total_assets + one();
    let den = cv_total_supply + BigUint::from(10u8).pow(decimals_offset as u32);
    let alm_shares = mul_div(coll_vault_shares, &num, &den);
    let value = mul_div(&alm_shares, rvps_wad, &wad());
    mul_div(&value, &wad(), coll_vault_shares)
}

/// `PropMath._computeCR`: `coll*price/debt` floored; `None` means infinite (debt 0).
fn compute_cr(coll: &BigUint, debt: &BigUint, price: &BigUint) -> Option<BigUint> {
    if *debt == BigUint::ZERO {
        return None;
    }
    Some(mul_div(coll, price, debt))
}
