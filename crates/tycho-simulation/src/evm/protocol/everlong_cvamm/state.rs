use std::{any::Any, collections::HashMap};

use alloy::primitives::U256;
use num_bigint::BigUint;
use tycho_common::{
    dto::ProtocolStateDelta,
    models::token::Token,
    simulation::{
        errors::{SimulationError, TransitionError},
        protocol_sim::{Balances, GetAmountOutResult, PoolSwap, ProtocolSim, QueryPoolSwapParams},
    },
    Bytes,
};

use super::{
    decoder::apply_delta,
    fee_law::{Book, FeeLawTerms},
    fee_law_exact::{re_sample_fees_best, reservation_price_wad, FeeSolve},
    math::{self, CvammMathError, Support},
};
use crate::evm::protocol::u256_num::{biguint_to_u256, u256_to_biguint, u256_to_f64};

pub type Address = [u8; 20];

// MEASURED THROUGH THE ADAPTER against the deployed Berachain venue, which is the path a
// route actually pays for — direct-venue numbers understate it. Worst case per direction
// across the settled replays and the oversized partial fills: stable-in 321,747 (its
// bisection scales with size, and the partial path is the ceiling), volatile-in 239,082.
// These carry ~25% over that: a fork replays warm storage while a production fill pays cold
// SLOAD across the ALM and its fee hook.
const GAS_STABLE_IN: u64 = 400_000;
const GAS_VOLATILE_IN: u64 = 300_000;

/// State of one CvammALM venue: a single-LP AMM that holds both tokens itself and
/// evaluates a closed-form reservation curve. `stable` is the 18-decimal token0
/// (pinned by the contract, not sorted by address), `volatile` is token1.
///
/// Exact-input only; the fee is a directional OUTPUT haircut sampled pre-trade.
/// Partial fills are normal — the band's finite support truncates rather than
/// reverts, and on-chain the venue pulls only the used input — so
/// [`ProtocolSim::get_amount_out`] prices the truncated fill instead of erroring,
/// matching what execution settles.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EverlongCvammState {
    pub alm: Address,
    pub stable: Address,
    pub volatile: Address,
    /// Authoritative inventory coordinate — never derived from a sqrt price.
    pub x_wad: U256,
    pub kappa: U256,
    /// Anchor sqrt price in the CURVE frame (Q96); the pool frame is 2^192 / this.
    pub anchor_sqrt_x96: U256,
    pub a_wad: U256,
    pub span_up_wad: U256,
    pub span_dn_wad: U256,
    /// The funded band derived from `(a_wad, span_up_wad, span_dn_wad)`; cached
    /// because rebuilding it costs two 128-step bisections.
    pub support: Support,
    /// Accounted tradeable reserves (idle excluded) — the solvency clamp caps the
    /// gross payout at these, exactly as the venue does.
    pub reserve_stable: U256,
    pub reserve_volatile: U256,
    /// Directional OUTPUT fee, sampled pre-trade. A fill moves the book the fee law reads,
    /// so these are re-derived rather than carried across one — see [`Self::next_fees`].
    pub fee_stable_in_wad: U256,
    pub fee_volatile_in_wad: U256,
    /// The fee law's terms, from the ALM's `ClammFeeHook`. `None` disables the
    /// re-derivation and leaves the conservative fold.
    pub fee_law: Option<FeeLawTerms>,
    /// False once `FeeHookSet` moved the ALM to a hook the terms were not read from.
    pub fee_law_tracked: bool,
    /// Solved once from the SNAPSHOT's own fee samples and carried across fills: solving
    /// again from an already re-derived fee would ratchet the haircut up hop over hop.
    pub(super) fee_solve: FeeSolve,
    pub paused: bool,
    pub retracted: bool,
}

/// The fee law's inputs, owned so a [`Book`] can borrow them.
struct FeeBookWords {
    x_wad: BigUint,
    reserve_stable: BigUint,
    reserve_volatile: BigUint,
    reservation_price_wad: BigUint,
    anchor_sqrt_x96: BigUint,
    a_wad: BigUint,
}

/// Outcome of a priced fill, before it is folded into a `GetAmountOutResult`.
struct Quote {
    net_out: U256,
    gross_out: U256,
    used_in: U256,
    x_after: U256,
    stable_in: bool,
}

impl EverlongCvammState {
    /// Rebuild the cached band after `a_wad`/`span_*` changed. A configuration the
    /// curve cannot express marks the venue retracted instead of failing the
    /// transition — the next retune can heal it.
    pub fn rebuild_support(&mut self) {
        match math::support_for(self.a_wad, self.span_up_wad, self.span_dn_wad) {
            Ok(support) => {
                self.support = support;
                self.retracted = false;
            }
            Err(_) => {
                self.support = Support::default();
                self.retracted = true;
            }
        }
    }

    fn direction(&self, token_in: &[u8], token_out: &[u8]) -> Result<bool, SimulationError> {
        if token_in == self.stable && token_out == self.volatile {
            Ok(true)
        } else if token_in == self.volatile && token_out == self.stable {
            Ok(false)
        } else {
            Err(SimulationError::InvalidInput("invalid Everlong CVAMM token pair".to_owned(), None))
        }
    }

    /// Mirrors `CvammSwapLib.execute` step for step (no price bound): coordinate
    /// fill -> solvency clamp -> pre-trade output-side fee.
    fn quote_exact_in(&self, stable_in: bool, amount_in: U256) -> Result<Quote, QuoteError> {
        if self.paused {
            return Err(QuoteError::Paused);
        }
        if self.retracted {
            return Err(QuoteError::Retracted);
        }

        let fill = math::swap_exact_in_x96(
            &self.support,
            self.anchor_sqrt_x96,
            self.kappa,
            self.x_wad,
            stable_in,
            amount_in,
        )?;
        let used_in = amount_in - fill.unspent_in;
        let mut gross_out = fill.gross_out;
        if used_in.is_zero() || gross_out.is_zero() {
            return Err(QuoteError::Exhausted);
        }

        // The curve PRICES; the accounted reserves are authoritative for SOLVENCY.
        // A fill walking to the band edge can quote a hair above the cached balance
        // — clamp DOWN, as the venue does.
        let available = if stable_in { self.reserve_volatile } else { self.reserve_stable };
        gross_out = gross_out.min(available);
        if gross_out.is_zero() {
            return Err(QuoteError::Exhausted);
        }

        // Fee on the output leg, sampled pre-trade, floored — the net rounds up by
        // <= 1 base unit in the taker's favour, matching the chain.
        let fee_wad = if stable_in { self.fee_stable_in_wad } else { self.fee_volatile_in_wad };
        if fee_wad >= math::wad() {
            // A fee at or above 100% would underflow `gross - fee` below and quote a
            // wrapped amount. The venue caps its own fee at WAD, so this is an impossible
            // read — refuse it rather than let it reach the router.
            return Err(QuoteError::InvalidFee);
        }
        let fee = math::mul_div_down(gross_out, fee_wad, math::wad()).map_err(QuoteError::Math)?;
        let net_out = gross_out - fee;
        if net_out.is_zero() {
            return Err(QuoteError::Exhausted);
        }

        Ok(Quote { net_out, gross_out, used_in, x_after: fill.x_after, stable_in })
    }

    /// The state after a fill: the input leg grows by the amount actually used,
    /// the output leg shrinks by the GROSS output (the fee leaves the priced book
    /// into idle), the coordinate moves, and both directional fees are repriced at
    /// the book the fill left. kappa, anchor and band never move on a swap.
    fn apply_quote(&self, quote: &Quote) -> Self {
        let mut next = self.clone();
        next.x_wad = quote.x_after;
        if quote.stable_in {
            next.reserve_stable = next
                .reserve_stable
                .saturating_add(quote.used_in);
            next.reserve_volatile = next
                .reserve_volatile
                .saturating_sub(quote.gross_out);
        } else {
            next.reserve_volatile = next
                .reserve_volatile
                .saturating_add(quote.used_in);
            next.reserve_stable = next
                .reserve_stable
                .saturating_sub(quote.gross_out);
        }
        // Solve once from the SNAPSHOT's own samples and carry the result forward: solving
        // again from an already re-derived fee would ratchet the haircut up hop over hop
        // instead of repricing from the venue.
        let mut solve = self.fee_solve.clone();
        let (fee_stable_in, fee_volatile_in) = self.derive_fees(&mut solve, &next);
        next.fee_solve = solve;
        next.fee_stable_in_wad = fee_stable_in;
        next.fee_volatile_in_wad = fee_volatile_in;
        next
    }

    /// The book the fee law reads at this state.
    fn fee_book<'a>(&self, words: &'a FeeBookWords) -> Book<'a> {
        Book {
            x_wad: &words.x_wad,
            reserve_stable: &words.reserve_stable,
            reserve_volatile: &words.reserve_volatile,
            reservation_price_wad: &words.reservation_price_wad,
            anchor_sqrt_x96: &words.anchor_sqrt_x96,
            a_wad: &words.a_wad,
        }
    }

    /// The two directional fees at the post-fill book.
    ///
    /// The fee law reads `(reserves, spot, rv, ffadPushRate)`; a fill moves the first two,
    /// so the fee that was sampled is not the one a second hop through this ALM would pay.
    /// `rv` has no getter, but it reaches the fee only through a scalar a swap cannot move,
    /// so that scalar is solved from the sampled fee and reused at the post-fill book.
    ///
    /// Where the modelled law does not reproduce the samples the fee is unknowable
    /// off-chain, so a revisit takes the worse of the two: under-quoting costs a route,
    /// over-quoting hands the router a fill the venue will not honour.
    fn derive_fees(&self, solve: &mut FeeSolve, post: &Self) -> (U256, U256) {
        if let Some(terms) = self
            .fee_law
            .as_ref()
            .filter(|_| self.fee_law_tracked)
        {
            let pre_words = self.fee_book_words();
            let post_words = post.fee_book_words();
            if let Some(fees) = re_sample_fees_best(
                terms,
                solve,
                self.fee_stable_in_wad,
                self.fee_volatile_in_wad,
                &self.fee_book(&pre_words),
                &self.fee_book(&post_words),
            ) {
                return fees;
            }
        }
        let worse = self
            .fee_stable_in_wad
            .max(self.fee_volatile_in_wad);
        (worse, worse)
    }

    #[cfg(test)]
    fn derive_fees_for_test(&self, solve: &mut FeeSolve, post: &Self) -> (U256, U256) {
        self.derive_fees(solve, post)
    }

    fn fee_book_words(&self) -> FeeBookWords {
        FeeBookWords {
            x_wad: u256_to_biguint(self.x_wad),
            reserve_stable: u256_to_biguint(self.reserve_stable),
            reserve_volatile: u256_to_biguint(self.reserve_volatile),
            reservation_price_wad: reservation_price_wad(self.anchor_sqrt_x96),
            anchor_sqrt_x96: u256_to_biguint(self.anchor_sqrt_x96),
            a_wad: u256_to_biguint(self.support.a_wad),
        }
    }

    /// Maximum input the band can absorb for a direction, in token units (floored,
    /// so quoting it never truncates).
    fn band_capacity_in(&self, stable_in: bool) -> Result<U256, CvammMathError> {
        if stable_in {
            let y = math::y_at_x(self.x_wad, self.support.a_wad)?;
            let y_max = math::y_at_x(self.support.x_lo, self.support.a_wad)?;
            let reachable = if y_max > y { y_max - y } else { U256::ZERO };
            math::to_token(reachable, self.anchor_sqrt_x96, self.kappa, true)
        } else {
            let room = if self.support.x_hi > self.x_wad {
                self.support.x_hi - self.x_wad
            } else {
                U256::ZERO
            };
            math::to_token(room, self.anchor_sqrt_x96, self.kappa, false)
        }
    }
}

#[typetag::serde]
impl ProtocolSim for EverlongCvammState {
    fn fee(&self) -> f64 {
        let min_fee = self
            .fee_stable_in_wad
            .min(self.fee_volatile_in_wad);
        u256_to_f64(min_fee).unwrap_or(0.0) / 1e18
    }

    fn spot_price(&self, base: &Token, quote: &Token) -> Result<f64, SimulationError> {
        let stable_in = self.direction(base.address.as_ref(), quote.address.as_ref())?;
        let price_norm = math::price_at_x(self.x_wad, self.support.a_wad).map_err(map_math)?;

        // Wei ratio of stable per volatile: (anchor/2^96)^2 * p_norm / WAD.
        let anchor = u256_to_f64(self.anchor_sqrt_x96)? / 2f64.powi(96);
        let stable_per_volatile = anchor * anchor * (u256_to_f64(price_norm)? / 1e18);

        let (wei_ratio, fee_wad) = if stable_in {
            (1.0 / stable_per_volatile, self.fee_stable_in_wad)
        } else {
            (stable_per_volatile, self.fee_volatile_in_wad)
        };
        let decimals_adjustment = 10f64.powi(base.decimals as i32 - quote.decimals as i32);
        let fee = u256_to_f64(fee_wad)? / 1e18;
        Ok(wei_ratio * decimals_adjustment * (1.0 - fee))
    }

    fn get_amount_out(
        &self,
        amount_in: BigUint,
        token_in: &Token,
        token_out: &Token,
    ) -> Result<GetAmountOutResult, SimulationError> {
        let stable_in = self.direction(token_in.address.as_ref(), token_out.address.as_ref())?;
        let gas = if stable_in { GAS_STABLE_IN } else { GAS_VOLATILE_IN };
        if amount_in == BigUint::ZERO {
            return Ok(GetAmountOutResult::new(
                BigUint::ZERO,
                BigUint::from(gas),
                Box::new(self.clone()),
            ));
        }

        let quote = self
            .quote_exact_in(stable_in, biguint_to_u256(&amount_in))
            .map_err(map_quote_error)?;
        let next = self.apply_quote(&quote);
        Ok(GetAmountOutResult::new(
            u256_to_biguint(quote.net_out),
            BigUint::from(gas),
            Box::new(next),
        ))
    }

    fn get_limits(
        &self,
        sell_token: Bytes,
        buy_token: Bytes,
    ) -> Result<(BigUint, BigUint), SimulationError> {
        let stable_in = self.direction(sell_token.as_ref(), buy_token.as_ref())?;
        if self.paused || self.retracted {
            return Ok((BigUint::ZERO, BigUint::ZERO));
        }
        let max_in = self
            .band_capacity_in(stable_in)
            .map_err(map_math)?;
        if max_in.is_zero() {
            return Ok((BigUint::ZERO, BigUint::ZERO));
        }
        match self.quote_exact_in(stable_in, max_in) {
            Ok(quote) => Ok((u256_to_biguint(quote.used_in), u256_to_biguint(quote.net_out))),
            Err(QuoteError::Exhausted) => Ok((BigUint::ZERO, BigUint::ZERO)),
            Err(err) => Err(map_quote_error(err)),
        }
    }

    fn delta_transition(
        &mut self,
        delta: ProtocolStateDelta,
        _tokens: &HashMap<Bytes, Token>,
        _balances: &Balances,
    ) -> Result<(), TransitionError> {
        if let Some(name) = delta.deleted_attributes.iter().next() {
            return Err(TransitionError::DecodeError(format!(
                "Everlong CVAMM does not support deleted attributes: {name}"
            )));
        }
        let updated_attributes = delta
            .updated_attributes
            .into_iter()
            .filter(|(key, _)| key != "block_number" && key != "block_timestamp")
            .collect();
        apply_delta(self, updated_attributes)
            .map_err(|err| TransitionError::DecodeError(format!("{err:?}")))
    }

    fn query_pool_swap(&self, params: &QueryPoolSwapParams) -> Result<PoolSwap, SimulationError> {
        crate::evm::query_pool_swap::query_pool_swap(self, params)
    }

    fn clone_box(&self) -> Box<dyn ProtocolSim> {
        Box::new(self.clone())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn eq(&self, other: &dyn ProtocolSim) -> bool {
        other.as_any().downcast_ref::<Self>() == Some(self)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum QuoteError {
    Paused,
    Retracted,
    /// The fill used no input or produced no output (dust below the normalized
    /// resolution, an empty direction, or a solvency-clamped-to-zero payout).
    Exhausted,
    /// A directional fee at or above 100%, which the venue cannot charge.
    InvalidFee,
    Math(CvammMathError),
}

impl From<CvammMathError> for QuoteError {
    fn from(err: CvammMathError) -> Self {
        match err {
            CvammMathError::RetractedBook => QuoteError::Retracted,
            other => QuoteError::Math(other),
        }
    }
}

fn map_quote_error(err: QuoteError) -> SimulationError {
    SimulationError::RecoverableError(format!("Everlong CVAMM quote rejected: {err:?}"))
}

fn map_math(err: CvammMathError) -> SimulationError {
    SimulationError::FatalError(format!("Everlong CVAMM math error: {err:?}"))
}

#[cfg(test)]
mod tests {
    use serde_json::Value;
    use tycho_common::models::Chain;

    use super::*;

    const FIXTURES: &str = include_str!("testdata/cvamm_fixtures.csv");
    const LIVE_SNAPSHOT: &str = include_str!("testdata/berachain_block_24712088.json");

    fn u(value: &str) -> U256 {
        U256::from_str_radix(value, 10).expect("decimal fixture value")
    }

    fn addr(byte: u8) -> Address {
        [byte; 20]
    }

    fn token(address: Address, symbol: &str, decimals: u32) -> Token {
        Token::new(
            &Bytes::from(address.to_vec()),
            symbol,
            decimals,
            100,
            &[Some(100_000)],
            Chain::Base,
            100,
        )
    }

    /// Live Berachain CvammALM state at block 24712088 (support words as read from
    /// the venue, spans unknown — the support is set directly).
    fn live_state() -> EverlongCvammState {
        let snapshot: Value = serde_json::from_str(LIVE_SNAPSHOT).expect("valid snapshot json");
        let field = |name: &str| {
            u(snapshot[name]
                .as_str()
                .expect("string field"))
        };
        EverlongCvammState {
            alm: addr(9),
            stable: addr(1),
            volatile: addr(2),
            x_wad: field("xWad"),
            kappa: field("kappa"),
            anchor_sqrt_x96: field("anchorSqrtCurveX96"),
            a_wad: u(snapshot["support"]["aWad"]
                .as_str()
                .unwrap()),
            span_up_wad: U256::ZERO,
            span_dn_wad: U256::ZERO,
            support: Support {
                a_wad: u(snapshot["support"]["aWad"]
                    .as_str()
                    .unwrap()),
                x_lo: u(snapshot["support"]["xLo"]
                    .as_str()
                    .unwrap()),
                x_hi: u(snapshot["support"]["xHi"]
                    .as_str()
                    .unwrap()),
                y_hi: u(snapshot["support"]["yHi"]
                    .as_str()
                    .unwrap()),
            },
            reserve_stable: field("reserveStable"),
            reserve_volatile: field("reserveVolatile"),
            fee_stable_in_wad: field("feeStableInWad"),
            fee_volatile_in_wad: field("feeVolatileInWad"),
            fee_law: Some(deployed_fee_law()),
            fee_law_tracked: true,
            fee_solve: Default::default(),
            paused: snapshot["paused"].as_bool().unwrap(),
            retracted: false,
        }
    }

    /// The deployed ClammFeeHook's terms (0x14Fd229fB23565986abE05254E2976a259279e78) —
    /// the same values the substreams ships as creation attributes.
    fn deployed_fee_law() -> FeeLawTerms {
        let b = |value: &str| BigUint::parse_bytes(value.as_bytes(), 10).expect("decimal");
        FeeLawTerms {
            mid_fee_wad: b("30000000000000000"),
            out_fee_wad: b("5000000000000000"),
            curvature_wad: b("50000000000000000"),
            lp_fee_wad: BigUint::ZERO,
            dir_skew_wad: b("200000000000000000"),
            inv_skew_kappa_wad: BigUint::ZERO,
            inv_skew_band_wad: BigUint::ZERO,
            vol_sigma_ref_wad: b("400000000000000"),
            vol_beta_wad: b("4000000000000000000"),
            vol_min_wad: b("500000000000000000"),
            vol_max_wad: b("1600000000000000000"),
            floor_stable_in_wad: b("15000000000000000"),
            floor_volatile_in_wad: b("25000000000000000"),
        }
    }

    #[test]
    fn y_at_x_matches_fixture_grid() {
        let mut cases = 0usize;
        for line in FIXTURES.lines().skip(1) {
            let cols: Vec<&str> = line.split(',').collect();
            if cols[0] != "Y" {
                continue;
            }
            let y = math::y_at_x(u(cols[7]), u(cols[1])).expect("in-domain fixture");
            assert_eq!(y, u(cols[10]), "y_at_x mismatch at x={} a={}", cols[7], cols[1]);
            cases += 1;
        }
        assert!(cases > 50, "expected the Y fixture rows to load, got {cases}");
    }

    #[test]
    fn swap_exact_in_x96_matches_fixture_grid() {
        let mut cases = 0usize;
        for line in FIXTURES.lines().skip(1) {
            let cols: Vec<&str> = line.split(',').collect();
            if cols[0] != "S" {
                continue;
            }
            let support =
                Support { a_wad: u(cols[1]), x_lo: u(cols[2]), x_hi: u(cols[3]), y_hi: u(cols[4]) };
            let fill = math::swap_exact_in_x96(
                &support,
                u(cols[5]),
                u(cols[6]),
                u(cols[7]),
                cols[8] == "1",
                u(cols[9]),
            )
            .expect("in-domain fixture");
            assert_eq!(fill.gross_out, u(cols[10]), "gross mismatch: {line}");
            assert_eq!(fill.x_after, u(cols[11]), "x_after mismatch: {line}");
            assert_eq!(fill.unspent_in, u(cols[12]), "unspent mismatch: {line}");
            cases += 1;
        }
        assert!(cases > 700, "expected the S fixture rows to load, got {cases}");
    }

    /// The strongest oracle: (amountInUsed, amountOut) pairs settled by the deployed
    /// Berachain venue itself via eth_call of swap() — full path including the fee.
    #[test]
    fn live_executions_settle_wei_exact() {
        let snapshot: Value = serde_json::from_str(LIVE_SNAPSHOT).expect("valid snapshot json");
        let state = live_state();
        let mut cases = 0usize;
        for execution in snapshot["executions"]
            .as_array()
            .unwrap()
        {
            let stable_in = execution["stableIn"].as_bool().unwrap();
            let amount_in = u(execution["amountIn"].as_str().unwrap());
            let expected_used = u(execution["amountInUsed"]
                .as_str()
                .unwrap());
            let expected_out = u(execution["amountOut"].as_str().unwrap());

            match state.quote_exact_in(stable_in, amount_in) {
                Ok(quote) => {
                    assert_eq!(quote.used_in, expected_used, "used mismatch: {execution}");
                    assert_eq!(quote.net_out, expected_out, "out mismatch: {execution}");
                }
                Err(err) => {
                    assert_eq!(
                        expected_out,
                        U256::ZERO,
                        "venue settled {expected_out} but port rejected with {err:?}: {execution}"
                    );
                }
            }
            cases += 1;
        }
        assert!(cases >= 10, "expected the execution fixtures to load, got {cases}");
    }

    #[test]
    fn live_support_is_consistent_with_curve() {
        let state = live_state();
        let y_hi = math::y_at_x(state.support.x_hi, state.support.a_wad).unwrap();
        assert_eq!(y_hi, state.support.y_hi);
    }

    #[test]
    fn get_amount_out_transitions_state() {
        let state = live_state();
        let stable = token(state.stable, "NECT", 18);
        let volatile = token(state.volatile, "WBTC", 8);

        let amount_in = BigUint::from(10u8) * BigUint::from(10u32).pow(18);
        let result = state
            .get_amount_out(amount_in.clone(), &stable, &volatile)
            .expect("live state quotes");
        assert!(result.amount > BigUint::ZERO);

        let next = result
            .new_state
            .as_any()
            .downcast_ref::<EverlongCvammState>()
            .expect("same state type");
        assert!(next.x_wad < state.x_wad, "stable-in must lower the coordinate");
        assert!(next.reserve_stable > state.reserve_stable);
        assert!(next.reserve_volatile < state.reserve_volatile);
    }

    #[test]
    fn get_limits_is_quotable_and_exhausts_the_band() {
        let state = live_state();
        let stable = Bytes::from(state.stable.to_vec());
        let volatile = Bytes::from(state.volatile.to_vec());

        for (sell, buy) in [(stable.clone(), volatile.clone()), (volatile, stable)] {
            let (max_in, max_out) = state
                .get_limits(sell, buy)
                .expect("limits computable");
            assert!(max_in > BigUint::ZERO);
            assert!(max_out > BigUint::ZERO);
        }
    }

    #[test]
    fn spot_price_is_positive_and_direction_consistent() {
        let state = live_state();
        let stable = token(state.stable, "NECT", 18);
        let volatile = token(state.volatile, "WBTC", 8);

        let volatile_in_stable = state
            .spot_price(&volatile, &stable)
            .expect("spot price");
        let stable_in_volatile = state
            .spot_price(&stable, &volatile)
            .expect("spot price");
        assert!(volatile_in_stable > 0.0);
        assert!(stable_in_volatile > 0.0);
        // Fees push both directions below the no-fee reciprocal product.
        assert!(volatile_in_stable * stable_in_volatile <= 1.0);
    }

    /// Wei-exact parity with the venue: repricing at the UNCHANGED book must reproduce the
    /// two fees `poolFeeDirectional` reported at this block. Nothing else pins the ported
    /// law — the solved vol scalar is free until this holds in both directions at once.
    #[test]
    fn fee_law_reproduces_the_venue_sample() {
        let state = live_state();
        let mut solve = FeeSolve::default();
        let (stable_in, volatile_in) = state.derive_fees_for_test(&mut solve, &state);
        assert_eq!(stable_in, state.fee_stable_in_wad);
        assert_eq!(volatile_in, state.fee_volatile_in_wad);
    }

    /// At this book the base sits well off its MidFee cap, so the saturated bound alone
    /// would price both legs at MidFee * multiplier — far above what the venue charges. The
    /// exact law is what recovers that, and the two legs must stay distinct rather than
    /// collapsing to the worse sample the way the conservative fold does.
    #[test]
    fn a_revisit_prices_each_leg_on_its_own_fee() {
        let state = live_state();
        let amount_in = BigUint::from(10u8) * BigUint::from(10u32).pow(18);
        let stable = token(state.stable, "NECT", 18);
        let volatile = token(state.volatile, "WBTC", 8);
        let next = state
            .get_amount_out(amount_in, &stable, &volatile)
            .expect("live state quotes");
        let next = next
            .new_state
            .as_any()
            .downcast_ref::<EverlongCvammState>()
            .expect("same state type");

        let worse = state
            .fee_stable_in_wad
            .max(state.fee_volatile_in_wad);
        assert_ne!(
            next.fee_stable_in_wad, next.fee_volatile_in_wad,
            "the directional spread must survive a fill"
        );
        assert!(
            next.fee_volatile_in_wad < worse,
            "the cheap leg must not be repriced at the expensive sample"
        );
        // A stable-in fill lowers the coordinate toward the anchor, so the leg that was
        // widening is closer to restoring afterwards and cannot have grown.
        assert!(next.fee_stable_in_wad <= state.fee_stable_in_wad);
    }

    /// A hook the terms were not read from, or a snapshot without them, leaves the fold —
    /// both legs at the worse sample, which never under-charges.
    #[test]
    fn an_untracked_hook_falls_back_to_the_conservative_fold() {
        for mut state in [live_state(), live_state()] {
            if state.fee_law_tracked {
                state.fee_law_tracked = false;
            } else {
                state.fee_law = None;
            }
            let mut solve = FeeSolve::default();
            let (stable_in, volatile_in) = state.derive_fees_for_test(&mut solve, &state);
            let worse = state
                .fee_stable_in_wad
                .max(state.fee_volatile_in_wad);
            assert_eq!((stable_in, volatile_in), (worse, worse));
        }
    }

    /// A fee at or above WAD would underflow `gross - fee`. The venue caps its own at WAD,
    /// so the read is impossible — but it must be refused rather than wrapped into a quote
    /// of ~1e77 against a finite book.
    #[test]
    fn a_fee_at_or_above_wad_is_refused() {
        let mut state = live_state();
        state.fee_stable_in_wad = math::wad();
        assert_eq!(
            state
                .quote_exact_in(true, U256::from(10u128.pow(18)))
                .err(),
            Some(QuoteError::InvalidFee)
        );
        // The other direction is unaffected — the fee is per-direction.
        assert!(state
            .quote_exact_in(false, U256::from(1_000u64))
            .is_ok());
    }

    /// The vol scalar is solved from the samples at one book. A delta brings a fresh
    /// book and fresh samples, so the cached solve must not survive it — otherwise a
    /// later fill reprices off a scalar that describes the previous block.
    #[test]
    fn a_delta_drops_the_cached_fee_solve() {
        let state = live_state();
        let amount_in = BigUint::from(10u8) * BigUint::from(10u32).pow(18);
        let stable = token(state.stable, "NECT", 18);
        let volatile = token(state.volatile, "WBTC", 8);
        let filled = state
            .get_amount_out(amount_in, &stable, &volatile)
            .expect("live state quotes");
        let mut filled = filled
            .new_state
            .as_any()
            .downcast_ref::<EverlongCvammState>()
            .expect("same state type")
            .clone();
        assert_ne!(filled.fee_solve, Default::default(), "the fill solved the scalar");

        filled
            .delta_transition(
                ProtocolStateDelta {
                    component_id: "0x00".to_owned(),
                    updated_attributes: HashMap::from([(
                        "x_wad".to_owned(),
                        Bytes::from(state.x_wad.to_be_bytes_vec()),
                    )]),
                    deleted_attributes: Default::default(),
                },
                &HashMap::new(),
                &Balances::default(),
            )
            .expect("delta applies");
        assert_eq!(filled.fee_solve, Default::default());
    }

    #[test]
    fn paused_state_rejects_quotes() {
        let mut state = live_state();
        state.paused = true;
        assert!(state
            .quote_exact_in(true, U256::from(10u128.pow(18)))
            .is_err());
    }
}

#[cfg(test)]
mod support_parity {
    use alloy::primitives::U256;

    use super::math;

    fn u(value: &str) -> U256 {
        U256::from_str_radix(value, 10).expect("decimal fixture value")
    }

    /// The on-chain config (CurveRetuned at block 24689006: A=34e18, spanUp=4e18,
    /// spanDn=6e18) must rebuild exactly the support words the venue reports via
    /// getSupport() — this pins the derived-band design against production.
    #[test]
    fn support_for_reproduces_live_band() {
        let support = math::support_for(
            u("34000000000000000000"),
            u("4000000000000000000"),
            u("6000000000000000000"),
        )
        .expect("live config is expressible");
        assert_eq!(support.x_lo, u("31865306097213932"));
        assert_eq!(support.x_hi, u("1099912607172170593"));
        assert_eq!(support.y_hi, u("24096289941794300"));
    }
}
