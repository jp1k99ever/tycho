//! Exact port of `CvammFeeLib` for the UNSATURATED regime. The bound in
//! [`super::fee_law`] is safe but conservative: off the cap it substitutes `MidFee` for a
//! base the venue prices lower, which understates a revisit by the whole gap (58-59 bps
//! measured on the live book).
//!
//! The law's one unobservable input is `rv` (realized variance) — no getter on the ALM.
//! But it reaches the fee ONLY through `sigma/sigmaRef`, a scalar a swap does not move, so
//! it never has to be read: it is SOLVED from the fee the snapshot already carries, then
//! reused at the post-fill state where everything else (reserves, spot, dislocation) is
//! known.

use alloy::primitives::U256;
use num_bigint::{BigInt, BigUint, Sign};
use num_traits::{One, Signed, Zero};

use super::{
    fee_law::{floor_for, mul_div_floor, skew_multiplier, wad, Book, FeeLawTerms},
    math,
};
use crate::evm::protocol::{
    safe_math::sqrt_u256,
    u256_num::{biguint_to_u256, u256_to_biguint},
};

fn q192() -> BigUint {
    BigUint::one() << 192
}

/// `CvammFeeLib.spotRawWad(CvammCurve.sqrtX96At(..))`: the live marginal price in the frame
/// the anchor uses. Kept stepwise so each floor lands where Solidity puts it.
///
/// `None` where `sqrtX96At` reverts — outside the uint160 sqrt-price envelope the venue
/// takes the whole fee sample with it, so declining here refuses exactly where it does.
fn spot_raw_wad(book: &Book<'_>) -> Option<BigUint> {
    let price = math::price_at_x(biguint_to_u256(book.x_wad), biguint_to_u256(book.a_wad)).ok()?;
    let root = sqrt_u256(price.checked_mul(math::wad())?).ok()?;
    let s = mul_div_floor(book.anchor_sqrt_x96, &u256_to_biguint(root), &wad());
    if s.is_zero() || s.bits() > 160 {
        return None;
    }
    Some(mul_div_floor(&s, &(&s * wad()), &q192()))
}

/// `RepegMath.logRatioAbsWad`: `|ln(a/b)|` in WAD, and 0 when the ratio floors away.
fn log_ratio_abs_wad(a_wad: &BigUint, b_wad: &BigUint) -> Option<BigUint> {
    if a_wad.is_zero() || b_wad.is_zero() {
        return Some(BigUint::ZERO);
    }
    let ratio = mul_div_floor(a_wad, &wad(), b_wad);
    if ratio.is_zero() {
        return Some(BigUint::ZERO);
    }
    Some(
        ln_wad(&ratio)?
            .abs()
            .to_biguint()
            .expect("abs is non-negative"),
    )
}

/// `CvammFeeLib._reductionG`: `(g, volatile value weight)`. A zero `g` marks a degenerate or
/// one-sided book, where the law returns the imbalanced fee instead of interpolating.
fn reduction_g(terms: &FeeLawTerms, book: &Book<'_>) -> (BigUint, BigUint) {
    let volatile_value = mul_div_floor(book.reserve_volatile, book.reservation_price_wad, &wad());
    let total = book.reserve_stable + &volatile_value;
    if book.reserve_stable.is_zero() || volatile_value.is_zero() || total.is_zero() {
        return (BigUint::ZERO, BigUint::ZERO);
    }
    // Divide by `total` twice: forming `total^2/WAD` can floor to zero on a small book,
    // while two successive divisions cannot while both legs are non-zero.
    let k = mul_div_floor(
        &mul_div_floor(&(BigUint::from(4u8) * book.reserve_stable), &wad(), &total),
        &volatile_value,
        &total,
    );
    let gk = mul_div_floor(&terms.curvature_wad, &k, &wad());
    let denom = &gk + wad();
    if denom <= k {
        return (BigUint::ZERO, BigUint::ZERO);
    }
    (mul_div_floor(&gk, &wad(), &(denom - &k)), mul_div_floor(&volatile_value, &wad(), &total))
}

/// The part of the law a fill fixes but the vol scalar does not touch. It costs one square
/// root and one logarithm, so it is built once per book state and re-evaluated cheaply
/// across the solve.
struct FeeCtx {
    /// Set on the law's two short-circuit branches (zero curvature, degenerate book), where
    /// the fee is a constant and so carries no information about the scalar.
    scalar_branch: Option<BigUint>,
    g: BigUint,
    boost: BigUint,
    mult_stable_in: BigUint,
    mult_vol_in: BigUint,
}

impl FeeCtx {
    fn build(terms: &FeeLawTerms, book: &Book<'_>) -> Option<Self> {
        if terms.curvature_wad.is_zero() {
            return Some(Self::constant(terms.lp_fee_wad.clone()));
        }
        let (g, volatile_weight) = reduction_g(terms, book);
        if g.is_zero() {
            return Some(Self::constant(terms.out_fee_wad.clone()));
        }
        let spot = spot_raw_wad(book)?;
        let dislocation = log_ratio_abs_wad(&spot, book.reservation_price_wad)?;
        let below = spot < *book.reservation_price_wad;
        Some(Self {
            scalar_branch: None,
            g,
            boost: wad() + mul_div_floor(&terms.vol_beta_wad, &dislocation, &wad()),
            mult_stable_in: skew_multiplier(terms, &volatile_weight, below, true),
            mult_vol_in: skew_multiplier(terms, &volatile_weight, !below, false),
        })
    }

    fn constant(fee: BigUint) -> Self {
        Self {
            scalar_branch: Some(fee),
            g: BigUint::ZERO,
            boost: BigUint::ZERO,
            mult_stable_in: BigUint::ZERO,
            mult_vol_in: BigUint::ZERO,
        }
    }

    /// The law at this state and vol scalar, WITHOUT the hook floor.
    fn raw(&self, terms: &FeeLawTerms, scalar: &BigUint, stable_in: bool) -> BigUint {
        if let Some(fee) = &self.scalar_branch {
            return fee.clone();
        }
        // A zero reference disables the vol term outright — the hook returns WAD unclamped.
        let v = if terms.vol_sigma_ref_wad.is_zero() {
            wad()
        } else {
            mul_div_floor(scalar, &self.boost, &wad())
                .max(terms.vol_min_wad.clone())
                .min(terms.vol_max_wad.clone())
        };
        let base = &terms.out_fee_wad +
            mul_div_floor(
                &(&terms.mid_fee_wad - &terms.out_fee_wad),
                &mul_div_floor(&self.g, &v, &wad()),
                &wad(),
            );
        let multiplier = if stable_in { &self.mult_stable_in } else { &self.mult_vol_in };
        mul_div_floor(&base.min(terms.mid_fee_wad.clone()), multiplier, &wad()).min(wad())
    }
}

/// The largest `a` with `floor(a * mul / div) <= limit` — the inverse of one floored
/// multiply. `None` means the step imposes no bound at all.
fn max_factor(limit: &BigUint, mul: &BigUint, div: &BigUint) -> Option<BigUint> {
    if mul.is_zero() {
        return None;
    }
    Some(((limit + BigUint::one()) * div - BigUint::one()) / mul)
}

/// The largest vol scalar that both sampled fees admit, or `None` when the inversion and
/// the forward law disagree.
///
/// The scalar is `sigma/sigmaRef`, which a swap cannot move, so the value solved here at
/// the pre-fill book is still the venue's at the post-fill one. It is an UPPER bound rather
/// than the exact value because the hook floor only ever raises a fee: the true scalar
/// always satisfies `raw <= sampled`, and this returns the largest scalar that still does.
/// The fee is monotone in it, so pricing the fill with it cannot under-quote.
///
/// The law is a chain of floored multiplies, so it inverts step by step rather than by
/// search. The result is then confirmed to be exactly maximal.
fn solve_vol_scalar(
    terms: &FeeLawTerms,
    ctx: &FeeCtx,
    sampled_stable_in: &BigUint,
    sampled_volatile_in: &BigUint,
) -> Option<BigUint> {
    let fits = |scalar: &BigUint| {
        ctx.raw(terms, scalar, true) <= *sampled_stable_in &&
            ctx.raw(terms, scalar, false) <= *sampled_volatile_in
    };
    let unbounded = &terms.vol_max_wad;
    if ctx.scalar_branch.is_some() || terms.vol_sigma_ref_wad.is_zero() {
        // The fee does not depend on the scalar here, so the samples say nothing about it.
        return fits(unbounded).then(|| unbounded.clone());
    }

    // Back through the directional multiplier, taking the tighter of the two legs.
    let mut base_hi: Option<BigUint> = None;
    for (multiplier, sampled) in
        [(&ctx.mult_stable_in, sampled_stable_in), (&ctx.mult_vol_in, sampled_volatile_in)]
    {
        if *sampled >= wad() {
            continue; // the law caps at WAD, so this leg admits any base
        }
        if let Some(bound) = max_factor(sampled, multiplier, &wad()) {
            if base_hi
                .as_ref()
                .is_none_or(|current| bound < *current)
            {
                base_hi = Some(bound);
            }
        }
    }

    let mut hi = unbounded.clone();
    if let Some(base_hi) = base_hi.filter(|bound| *bound < terms.mid_fee_wad) {
        if base_hi < terms.out_fee_wad {
            return None; // below the law's own floor at every scalar
        }
        // Back through the interpolation, the reduction coefficient, and the boost.
        let interpolated = max_factor(
            &(&base_hi - &terms.out_fee_wad),
            &(&terms.mid_fee_wad - &terms.out_fee_wad),
            &wad(),
        );
        let v = interpolated.and_then(|bound| max_factor(&bound, &ctx.g, &wad()));
        if let Some(v) = v.filter(|bound| *bound < terms.vol_max_wad) {
            if v < terms.vol_min_wad {
                return None; // the clamp holds the multiplier above this
            }
            if let Some(scalar) = max_factor(&v, &ctx.boost, &wad()) {
                hi = hi.min(scalar);
            }
        }
    }

    // Maximal by construction; confirmed so an inversion that disagrees with the forward
    // law declines instead of pricing off a scalar on either side of the truth.
    if !fits(&hi) || (hi < *unbounded && fits(&(&hi + BigUint::one()))) {
        return None;
    }
    Some(hi)
}

/// What the re-derivation must not recompute per hop: the vol scalar, which a swap cannot
/// move, and the snapshot's OWN sampled fees, which bound the hook floor the law never gets
/// to read.
///
/// Solving again from an already re-derived fee would ratchet the haircut up hop over hop
/// instead of repricing from the venue, so this is carried across a fill rather than
/// rebuilt from the state the fill produced.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FeeSolve {
    solved: Option<Solved>,
    /// Set once the solve has been attempted, so a failure is not retried per hop.
    attempted: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct Solved {
    scalar: BigUint,
    /// Per-direction upper bounds on the hook floor, fixed at solve time.
    floor_stable_in: BigUint,
    floor_volatile_in: BigUint,
}

impl FeeSolve {
    /// Brackets the scalar once, from the book the snapshot was read at. Repeated calls
    /// return the first outcome.
    fn solve(
        &mut self,
        terms: &FeeLawTerms,
        sampled_stable_in: &BigUint,
        sampled_volatile_in: &BigUint,
        pre: &Book<'_>,
    ) -> Option<&Solved> {
        if !self.attempted {
            self.attempted = true;
            self.solved = Self::solve_at(terms, sampled_stable_in, sampled_volatile_in, pre);
        }
        self.solved.as_ref()
    }

    fn solve_at(
        terms: &FeeLawTerms,
        sampled_stable_in: &BigUint,
        sampled_volatile_in: &BigUint,
        pre: &Book<'_>,
    ) -> Option<Solved> {
        let ctx = FeeCtx::build(terms, pre)?;
        let scalar = solve_vol_scalar(terms, &ctx, sampled_stable_in, sampled_volatile_in)?;
        Some(Solved {
            floor_stable_in: floor_upper_bound(
                terms,
                &ctx.raw(terms, &scalar, true),
                sampled_stable_in,
                true,
            ),
            floor_volatile_in: floor_upper_bound(
                terms,
                &ctx.raw(terms, &scalar, false),
                sampled_volatile_in,
                false,
            ),
            scalar,
        })
    }
}

/// Bounds the hook floor for one direction, from above, at the book the snapshot was read
/// at.
///
/// The floor depends on the direction and the decayed push rate, neither of which a fill
/// touches, so the bound still holds after one. Two things constrain it. The sampled fee is
/// `max(law, floor)`: when it sits ABOVE what the law produces, the floor is what raised it
/// and the sample IS the floor exactly; otherwise the floor merely sits at or below the
/// law's own output. And the floor at a SATURATED push rate is its ceiling outright.
///
/// Taking the tighter of the two is what keeps a direction REVERSAL exact. The pre-fill
/// sample for the reversing leg is high for a directional reason, not a floor one, so
/// carrying it forward would re-impose the fee the fill just moved away from.
fn floor_upper_bound(
    terms: &FeeLawTerms,
    raw_pre: &BigUint,
    sampled: &BigUint,
    stable_in: bool,
) -> BigUint {
    if sampled > raw_pre {
        return sampled.clone();
    }
    raw_pre
        .clone()
        .min(floor_for(terms, stable_in).clone())
}

/// Reprices both legs at the post-fill book through the full law.
///
/// The hook floor is never read live: [`floor_upper_bound`] pins it from the sample and the
/// saturated-rate term, so no second round of reads is needed.
pub fn re_sample_fees_exact(
    terms: &FeeLawTerms,
    solve: &mut FeeSolve,
    sampled_stable_in: &BigUint,
    sampled_volatile_in: &BigUint,
    pre: &Book<'_>,
    post: &Book<'_>,
) -> Option<(BigUint, BigUint)> {
    let solved = solve
        .solve(terms, sampled_stable_in, sampled_volatile_in, pre)?
        .clone();
    let ctx = FeeCtx::build(terms, post)?;
    Some((
        ctx.raw(terms, &solved.scalar, true)
            .max(solved.floor_stable_in),
        ctx.raw(terms, &solved.scalar, false)
            .max(solved.floor_volatile_in),
    ))
}

// ---------------------------------------------------------------------------------------
// Solady `lnWad`, the logarithm `RepegMath.logRatioAbsWad` measures dislocation with. Same
// rounding as the venue's, so the dislocation term matches wei for wei.
// ---------------------------------------------------------------------------------------

fn ln_constant(digits: &str) -> BigInt {
    BigInt::parse_bytes(digits.as_bytes(), 10).expect("valid lnWad constant")
}

/// Arithmetic shift right, as EVM `sar`: `num_bigint` shifts a negative value toward
/// negative infinity, which is the same rounding.
fn sar(value: &BigInt, shift: u64) -> BigInt {
    value >> shift
}

/// `None` for an input outside the domain, where the venue's `lnWad` reverts.
fn ln_wad(x: &BigUint) -> Option<BigInt> {
    if x.is_zero() || x.bits() > 256 {
        return None;
    }
    let c0 = ln_constant("43456485725739037958740375743393");
    let c1 = ln_constant("24828157081833163892658089445524");
    let c2 = ln_constant("3273285459638523848632254066296");
    let c3 = ln_constant("11111509109440967052023855526967");
    let c4 = ln_constant("45023709667254063763336534515857");
    let c5 = ln_constant("14706773417378608786704636184526");
    let c6 = ln_constant("795164235651350426258249787498");
    let c7 = ln_constant("5573035233440673466300451813936");
    let c8 = ln_constant("71694874799317883764090561454958");
    let c9 = ln_constant("283447036172924575727196451306956");
    let c10 = ln_constant("401686690394027663651624208769553");
    let c11 = ln_constant("204048457590392012362485061816622");
    let c12 = ln_constant("31853899698501571402653359427138");
    let c13 = ln_constant("909429971244387300277376558375");
    let c14 = ln_constant("1677202110996718588342820967067443963516166");
    let c15 =
        ln_constant("16597577552685614221487285958193947469193820559219878177908093499208371");
    let c16 =
        ln_constant("600920179829731861736702779321621459595472258049074101567377883020018308");

    let r = 256i64 - x.bits() as i64;
    let x96 = BigInt::from_biguint(Sign::Plus, (x << (r as u64)) >> 159u64);

    let mut p = &c0 + sar(&((&c1 + sar(&((&c2 + &x96) * &x96), 96)) * &x96), 96);
    p = sar(&(&p * &x96), 96) - &c3;
    p = sar(&(&p * &x96), 96) - &c4;
    p = sar(&(&p * &x96), 96) - &c5;
    p = &p * &x96 - (&c6 << 96u64);

    let mut q = &c7 + &x96;
    for constant in [&c8, &c9, &c10, &c11, &c12, &c13] {
        q = constant + sar(&(&x96 * &q), 96);
    }

    p /= q;
    p = &c14 * p;
    p = &c15 * BigInt::from(159 - r) + p;
    p = &c16 + p;
    Some(sar(&p, 174))
}

/// Reprices both legs by whichever re-derivation is tighter.
///
/// Both bound the post-fill fee from above, so the smaller is the better quote and is still
/// safe. The exact law wins wherever the base is off its cap — the case the bound alone
/// prices as `MidFee`, measured 58-59 bps too wide on a revisit. The bound wins where the
/// exact law declines: a hook upgrade, or a book the modelled law does not reproduce.
pub fn re_sample_fees_best(
    terms: &FeeLawTerms,
    solve: &mut FeeSolve,
    sampled_stable_in: U256,
    sampled_volatile_in: U256,
    pre: &Book<'_>,
    post: &Book<'_>,
) -> Option<(U256, U256)> {
    let sampled_stable_in = u256_to_biguint(sampled_stable_in);
    let sampled_volatile_in = u256_to_biguint(sampled_volatile_in);
    if !terms_are_usable(terms, pre) {
        return None;
    }
    let exact =
        re_sample_fees_exact(terms, solve, &sampled_stable_in, &sampled_volatile_in, pre, post);
    let bound =
        super::fee_law::re_sample_fees(terms, &sampled_stable_in, &sampled_volatile_in, pre, post);
    let (stable_in, volatile_in) = match (exact, bound) {
        (Some((xs, xv)), Some((bs, bv))) => (xs.min(bs), xv.min(bv)),
        (Some(pair), None) | (None, Some(pair)) => pair,
        (None, None) => return None,
    };
    Some((biguint_to_u256(&stable_in), biguint_to_u256(&volatile_in)))
}

/// Whether the terms and the book carry everything the law reads. A snapshot that fails
/// this leaves the conservative fold, which never needs any of it.
fn terms_are_usable(terms: &FeeLawTerms, book: &Book<'_>) -> bool {
    !terms.vol_max_wad.is_zero() &&
        !book.reservation_price_wad.is_zero() &&
        !book.anchor_sqrt_x96.is_zero() &&
        terms.mid_fee_wad >= terms.out_fee_wad
}

/// `CvammALM`'s stored `reservationPriceWad`, which it sets from the anchor as
/// `mulDiv(anchorSqrtCurveX96, anchorSqrtCurveX96 * WAD, Q192)`.
///
/// Exact at initialize. A recenter assigns the repeg controller's target directly and emits
/// only the sqrt price it derived from it, so after one this reproduces the anchor to
/// within the sqrt's own rounding. The re-derivation self-checks against the sampled fees
/// and declines when the modelled law does not reproduce them, so a drift there degrades to
/// the conservative fold rather than mispricing.
pub fn reservation_price_wad(anchor_sqrt_x96: U256) -> BigUint {
    let anchor = u256_to_biguint(anchor_sqrt_x96);
    mul_div_floor(&anchor, &(&anchor * wad()), &q192())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(value: &str) -> BigUint {
        BigUint::parse_bytes(value.as_bytes(), 10).expect("decimal")
    }

    /// `ln(1) = 0` and `ln(2)` to the wei, against Solady's own published values — the
    /// dislocation term is only as exact as this.
    #[test]
    fn ln_wad_matches_solady() {
        assert_eq!(ln_wad(&wad()).unwrap(), BigInt::ZERO);
        assert_eq!(
            ln_wad(&(BigUint::from(2u8) * wad())).unwrap(),
            BigInt::parse_bytes(b"693147180559945309", 10).unwrap()
        );
        // Below WAD the logarithm is negative, and the dislocation takes its magnitude.
        assert_eq!(
            ln_wad(&(wad() / 2u8)).unwrap(),
            BigInt::parse_bytes(b"-693147180559945310", 10).unwrap()
        );
        assert_eq!(log_ratio_abs_wad(&(wad() / 2u8), &wad()).unwrap(), u("693147180559945310"));
        assert!(ln_wad(&BigUint::ZERO).is_none());
    }

    /// A ratio that floors to zero must read as no measurable dislocation rather than
    /// reverting — the venue treats it that way so a stuck slot does not DoS every swap.
    #[test]
    fn log_ratio_floors_to_zero_instead_of_failing() {
        assert_eq!(log_ratio_abs_wad(&BigUint::one(), &(u("1") << 200)).unwrap(), BigUint::ZERO);
        assert_eq!(log_ratio_abs_wad(&BigUint::ZERO, &wad()).unwrap(), BigUint::ZERO);
    }

    /// `max_factor` inverts one floored multiply, so its result must be admissible and its
    /// successor must not.
    #[test]
    fn max_factor_is_the_largest_admissible_input() {
        let limit = u("12345");
        let mul = u("777");
        let div = wad();
        let bound = max_factor(&limit, &mul, &div).unwrap();
        assert!(mul_div_floor(&bound, &mul, &div) <= limit);
        assert!(mul_div_floor(&(&bound + BigUint::one()), &mul, &div) > limit);
        assert!(max_factor(&limit, &BigUint::ZERO, &div).is_none());
    }
}
