use std::{any::Any, collections::HashMap};

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
    math::{self, CurveParams, Region, VaultState},
};
use crate::evm::protocol::u256_num::u256_to_f64;

pub type Address = [u8; 20];

// MEASURED THROUGH THE ADAPTER against the deployed Berachain venue, which is the path a
// route actually pays for: leverage 4,849,423 and deleverage 3,716,813 (the partial path,
// where the gross is re-derived against the venue's own math). These carry ~25% over that —
// a fork replays warm storage while a production fill pays cold SLOAD across the
// rebalancer, CollVault, ALM, fee hook and CDP, and the share bisection deepens with the
// position's scale.
const GAS_LEVERAGE: u64 = 6_000_000;
const GAS_DELEVERAGE: u64 = 4_600_000;

/// State of the Everlong CollateralRebalancer settlement venue (CollateralRebalancerSwapper):
/// a two-token venue between the stable (NECT) and volatile (WBTC) leg priced by the
/// deployed CollateralRebalancer CR bonding curve, not an AMM invariant.
///
/// - volatile -> stable is LEVERAGE (`swapVolatileForStable`): the caller's volatile leg mints
///   CollVault shares, the position draws debt, the caller receives net stable.
/// - stable -> volatile is DELEVERAGE (`swapStableForVolatile`): the caller fronts net stable to
///   repay debt, CollVault shares burn and the freed volatile leg pays out.
///
/// Direction availability is state-dependent: the curve realigns one direction at a
/// time, so the non-wanted direction rejects rather than pricing worse. Quotes size
/// the fill to the curve's own caps and report the sized amount, matching what
/// execution settles.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EverlongRebalancerState {
    pub swapper: Address,
    pub stable: Address,
    pub volatile: Address,
    pub vault: VaultState,
    pub curve: CurveParams,
    /// False once the rebalancer proxy upgraded away from the implementation the curve
    /// constants were read from. `leverageCurve()` is `pure` over the linked
    /// CollRebalancerMath, so an upgrade is the only way they can change and the only
    /// notice a base block carries — and a routable pool priced off constants nothing has
    /// re-verified is worse than no pool at all.
    pub curve_tracked: bool,
}

struct Quote {
    amount_out: BigUint,
    /// Input actually consumed (the sized lot's requirement, <= amount_in).
    used_in: BigUint,
    is_leverage: bool,
    new_collateral: BigUint,
    new_debt: BigUint,
    stable_leg: BigUint,
    volatile_leg: BigUint,
    alm_shares: BigUint,
    alm_burned: Option<BigUint>,
    post_cv_assets: BigUint,
    post_cv_supply: BigUint,
    post_price_wad: BigUint,
}

impl EverlongRebalancerState {
    fn direction(&self, token_in: &[u8], token_out: &[u8]) -> Result<bool, SimulationError> {
        if token_in == self.volatile && token_out == self.stable {
            Ok(true) // leverage
        } else if token_in == self.stable && token_out == self.volatile {
            Ok(false) // deleverage
        } else {
            Err(SimulationError::InvalidInput(
                "invalid Everlong Rebalancer token pair".to_owned(),
                None,
            ))
        }
    }

    fn region_gate(&self) -> Result<(), SimulationError> {
        if !self.curve_tracked {
            return Err(SimulationError::RecoverableError(
                "Everlong Rebalancer curve constants are not known to describe the deployed \
                 implementation"
                    .to_owned(),
            ));
        }
        if !self.curve.usable() {
            return Err(SimulationError::RecoverableError(
                "Everlong Rebalancer curve constants are not quotable".to_owned(),
            ));
        }
        let region = self.curve.state_region(
            &self.vault.collateral,
            &self.vault.debt,
            &self.vault.price_wad,
        );
        if region == Region::Out {
            // Degenerate/unmarked states — refuse to quote rather than risk a wrong
            // price. Recovery states past the CR wall quote deleverage-only.
            return Err(SimulationError::RecoverableError(
                "Everlong Rebalancer state is not priceable".to_owned(),
            ));
        }
        Ok(())
    }

    fn quote_leverage(&self, amount_in: &BigUint) -> Result<Quote, SimulationError> {
        let curve = &self.curve;
        let vault = &self.vault;

        // Debt origination is halted while the CDP charges borrow interest.
        if vault.interest_rate != BigUint::ZERO {
            return Err(SimulationError::RecoverableError(
                "Everlong Rebalancer leverage is disabled while borrow interest accrues".to_owned(),
            ));
        }
        let max_shares = curve.max_leverage_shares(vault);
        if max_shares == BigUint::ZERO {
            return Err(rejected());
        }
        let shares = vault.shares_for_volatile_in(amount_in, &max_shares);
        if shares == BigUint::ZERO {
            return Err(rejected());
        }
        let (stable_leg, volatile_leg, ok) = vault.preview_token_amounts(&shares, true);
        if !ok {
            return Err(rejected());
        }
        let net_stable_out = curve
            .quote_leverage_at(vault, &shares)
            .ok_or_else(rejected)?;
        if net_stable_out == BigUint::ZERO {
            return Err(rejected());
        }
        let (_, new_collateral, new_debt) = curve.leverage_quote_checked(vault, &shares);
        if volatile_leg > *amount_in {
            return Err(rejected());
        }
        let (post_cv_assets, post_cv_supply) = vault.post_vault_leverage(&shares);
        let post_price_wad =
            vault.post_reservation(&new_collateral, &post_cv_assets, &post_cv_supply);
        Ok(Quote {
            amount_out: net_stable_out,
            used_in: volatile_leg.clone(),
            is_leverage: true,
            new_collateral,
            new_debt,
            stable_leg,
            volatile_leg,
            alm_shares: vault.cv_convert_to_assets(&shares, true),
            alm_burned: None,
            post_cv_assets,
            post_cv_supply,
            post_price_wad,
        })
    }

    fn quote_deleverage(&self, amount_in: &BigUint) -> Result<Quote, SimulationError> {
        let curve = &self.curve;
        let vault = &self.vault;

        let max_gross = curve.max_deleverage_in(vault);
        if max_gross == BigUint::ZERO {
            return Err(rejected());
        }
        let gross = curve.gross_for_net_stable_in(vault, amount_in, &max_gross);
        if gross == BigUint::ZERO {
            return Err(rejected());
        }
        let (shares_out, new_collateral, new_debt) = curve.deleverage_quote_checked(vault, &gross);
        if shares_out == BigUint::ZERO {
            return Err(rejected());
        }
        let (stable_out, volatile_out, ok) = vault.preview_token_amounts(&shares_out, false);
        if !ok || volatile_out == BigUint::ZERO {
            return Err(rejected());
        }

        // The forward net is what the swapper actually pulls: gross - freed stable.
        if gross < stable_out {
            return Err(rejected());
        }
        let net = &gross - &stable_out;
        if net > *amount_in {
            return Err(rejected());
        }

        let (post_cv_assets, post_cv_supply) = vault.post_vault_deleverage(&shares_out);
        let post_price_wad =
            vault.post_reservation(&new_collateral, &post_cv_assets, &post_cv_supply);
        let share_fee =
            math::mul_div_up(&shares_out, &vault.withdraw_fee_bp, &BigUint::from(10_000u32));
        let net_shares = &shares_out - &share_fee;
        let alm_shares = vault.cv_convert_to_assets(&net_shares, false);
        let alm_burned = &vault.cv_total_assets - &post_cv_assets;
        Ok(Quote {
            amount_out: volatile_out.clone(),
            used_in: net,
            is_leverage: false,
            new_collateral,
            new_debt,
            stable_leg: stable_out,
            volatile_leg: volatile_out,
            alm_shares,
            alm_burned: Some(alm_burned),
            post_cv_assets,
            post_cv_supply,
            post_price_wad,
        })
    }

    /// Replays the fill into a fresh state, mirroring the Go `UpdateBalance` exactly.
    fn apply_quote(&self, quote: &Quote) -> Self {
        let mut next = self.clone();
        let vault = &mut next.vault;
        vault.collateral = quote.new_collateral.clone();
        vault.debt = quote.new_debt.clone();
        if quote.is_leverage {
            vault.alm_stable_reserve = &vault.alm_stable_reserve + &quote.stable_leg;
            vault.alm_volatile_reserve = &vault.alm_volatile_reserve + &quote.volatile_leg;
            vault.alm_supply = &vault.alm_supply + &quote.alm_shares;
            vault.ref_stable_reserve = &vault.ref_stable_reserve + &quote.stable_leg;
            vault.ref_asset_reserve = &vault.ref_asset_reserve + &quote.volatile_leg;
        } else {
            vault.alm_stable_reserve = saturating_sub(&vault.alm_stable_reserve, &quote.stable_leg);
            vault.alm_volatile_reserve =
                saturating_sub(&vault.alm_volatile_reserve, &quote.volatile_leg);
            let burned = quote
                .alm_burned
                .as_ref()
                .unwrap_or(&quote.alm_shares);
            vault.alm_supply = saturating_sub(&vault.alm_supply, burned);
            vault.ref_stable_reserve = saturating_sub(&vault.ref_stable_reserve, &quote.stable_leg);
            vault.ref_asset_reserve = saturating_sub(&vault.ref_asset_reserve, &quote.volatile_leg);
        }
        vault.cv_total_assets = quote.post_cv_assets.clone();
        vault.cv_total_supply = quote.post_cv_supply.clone();
        if quote.post_price_wad != BigUint::ZERO {
            vault.price_wad = quote.post_price_wad.clone();
        }
        next
    }
}

#[typetag::serde]
impl ProtocolSim for EverlongRebalancerState {
    fn fee(&self) -> f64 {
        // The spread lives inside the curve; there is no output-side fee to report.
        0.0
    }

    fn spot_price(&self, base: &Token, quote: &Token) -> Result<f64, SimulationError> {
        // The venue has no marginal-price closed form; probe the executed price of a
        // small fill in the requested direction (1e-6 of the reference volatile
        // reserve, floor 1 unit).
        let is_leverage = self.direction(base.address.as_ref(), quote.address.as_ref())?;
        self.region_gate()?;
        let probe_in = if is_leverage {
            (&self.vault.alm_volatile_reserve / BigUint::from(1_000_000u32)).max(BigUint::from(1u8))
        } else {
            (&self.vault.alm_stable_reserve / BigUint::from(1_000_000u32)).max(BigUint::from(1u8))
        };
        let quote_result = if is_leverage {
            self.quote_leverage(&probe_in)
        } else {
            self.quote_deleverage(&probe_in)
        }?;
        let out = biguint_to_f64(&quote_result.amount_out)?;
        let used = biguint_to_f64(&quote_result.used_in)?;
        if used == 0.0 {
            return Err(SimulationError::RecoverableError(
                "Everlong Rebalancer probe fill consumed no input".to_owned(),
            ));
        }
        let decimals_adjustment = 10f64.powi(base.decimals as i32 - quote.decimals as i32);
        Ok(out / used * decimals_adjustment)
    }

    fn get_amount_out(
        &self,
        amount_in: BigUint,
        token_in: &Token,
        token_out: &Token,
    ) -> Result<GetAmountOutResult, SimulationError> {
        let is_leverage = self.direction(token_in.address.as_ref(), token_out.address.as_ref())?;
        let gas = if is_leverage { GAS_LEVERAGE } else { GAS_DELEVERAGE };
        if amount_in == BigUint::ZERO {
            return Ok(GetAmountOutResult::new(
                BigUint::ZERO,
                BigUint::from(gas),
                Box::new(self.clone()),
            ));
        }
        self.region_gate()?;
        let quote = if is_leverage {
            self.quote_leverage(&amount_in)
        } else {
            self.quote_deleverage(&amount_in)
        }?;
        let next = self.apply_quote(&quote);
        Ok(GetAmountOutResult::new(quote.amount_out.clone(), BigUint::from(gas), Box::new(next)))
    }

    fn get_limits(
        &self,
        sell_token: Bytes,
        buy_token: Bytes,
    ) -> Result<(BigUint, BigUint), SimulationError> {
        let is_leverage = self.direction(sell_token.as_ref(), buy_token.as_ref())?;
        if self.region_gate().is_err() {
            return Ok((BigUint::ZERO, BigUint::ZERO));
        }
        if is_leverage {
            if self.vault.interest_rate != BigUint::ZERO {
                return Ok((BigUint::ZERO, BigUint::ZERO));
            }
            let max_shares = self
                .curve
                .max_leverage_shares(&self.vault);
            if max_shares == BigUint::ZERO {
                return Ok((BigUint::ZERO, BigUint::ZERO));
            }
            let (_, volatile_leg, ok) = self
                .vault
                .preview_token_amounts(&max_shares, true);
            if !ok {
                return Ok((BigUint::ZERO, BigUint::ZERO));
            }
            let out = self
                .curve
                .quote_leverage_at(&self.vault, &max_shares)
                .unwrap_or(BigUint::ZERO);
            Ok((volatile_leg, out))
        } else {
            let max_gross = self
                .curve
                .max_deleverage_in(&self.vault);
            if max_gross == BigUint::ZERO {
                return Ok((BigUint::ZERO, BigUint::ZERO));
            }
            match self
                .curve
                .deleverage_legs_at(&self.vault, &max_gross)
            {
                Some((stable_out, volatile_out)) => {
                    let net = if max_gross > stable_out {
                        &max_gross - &stable_out
                    } else {
                        BigUint::ZERO
                    };
                    Ok((net, volatile_out))
                }
                None => Ok((BigUint::ZERO, BigUint::ZERO)),
            }
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
                "Everlong Rebalancer does not support deleted attributes: {name}"
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

fn rejected() -> SimulationError {
    SimulationError::RecoverableError("Everlong Rebalancer fill rejected".to_owned())
}

fn saturating_sub(a: &BigUint, b: &BigUint) -> BigUint {
    if a >= b {
        a - b
    } else {
        BigUint::ZERO
    }
}

fn biguint_to_f64(value: &BigUint) -> Result<f64, SimulationError> {
    u256_to_f64(crate::evm::protocol::u256_num::biguint_to_u256(value))
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;
    use crate::evm::protocol::everlong_rebalancer::math::{CurveParams, VaultState};

    const FIXTURES: &str = include_str!("testdata/collvault_fixtures.csv");
    const LIVE_SNAPSHOT: &str = include_str!("testdata/berachain_block_24710262.json");

    fn u(value: &str) -> BigUint {
        value
            .parse()
            .expect("decimal fixture value")
    }

    /// The deployed Berachain "champion-v3" constants — in production these arrive
    /// as component attributes; here they pin the fixture grid.
    fn berachain_curve() -> CurveParams {
        CurveParams {
            leverage_ratio_wad: u("444444444444444444"),
            h_zero: u("562500000000000000"),
            h_join: u("1010000000000000000"),
            h_wall: u("1882448291726770582"),
            width: u("872448291726770582"),
            d_join: u("509975124224178054"),
            d_wall: u("1214482768855981020"),
            rescue_spread_ppm: u("13000"),
            bezier_phi: [
                u("995037190209989135"),
                u("851783312849706840"),
                u("738044106433170508"),
                u("645161290322580645"),
            ],
            bezier_integral: [
                u("0"),
                u("248759297552497283"),
                u("461705125764923993"),
                u("646216152373216620"),
                u("807506474953861782"),
            ],
            physical_cr_floor_wad: u("1820000000000000000"),
        }
    }

    fn json_uint(value: &Value) -> BigUint {
        match value {
            Value::String(text) => u(text),
            Value::Number(number) => u(&number.to_string()),
            other => panic!("unexpected fixture value: {other}"),
        }
    }

    fn live_vault() -> VaultState {
        let snapshot: Value = serde_json::from_str(LIVE_SNAPSHOT).expect("valid snapshot json");
        let field = |name: &str| json_uint(&snapshot[name]);
        VaultState {
            collateral: field("C"),
            debt: field("D"),
            price_wad: field("R"),
            spread_ppm: field("spread"),
            alm_stable_reserve: field("almStableReserve"),
            alm_volatile_reserve: field("almVolatileReserve"),
            alm_supply: field("almSupply"),
            cv_total_assets: field("cvTotalAssets"),
            cv_total_supply: field("cvTotalSupply"),
            cv_decimals_offset: snapshot["cvDecimalsOffset"]
                .as_u64()
                .expect("offset") as u8,
            withdraw_fee_bp: field("withdrawFeeBp"),
            interest_rate: BigUint::ZERO,
            min_net_debt: field("minNetDebt"),
            debt_gas_compensation: field("debtGasCompensation"),
            ref_stable_reserve: field("refStableReserve"),
            ref_asset_reserve: field("refAssetReserve"),
            ref_raw_reference_wad: field("refRawReferenceWad"),
            rvps_wad: BigUint::ZERO,
            mcr_wad: BigUint::ZERO,
            icr_price_wad: BigUint::ZERO,
        }
    }

    fn live_state() -> EverlongRebalancerState {
        EverlongRebalancerState {
            swapper: [9u8; 20],
            stable: [1u8; 20],
            volatile: [2u8; 20],
            vault: live_vault(),
            curve: berachain_curve(),
            curve_tracked: true,
        }
    }

    /// An upgrade the curve constants were not re-verified against must stop the venue
    /// quoting outright, in both directions — a routable pool priced off a curve that may
    /// no longer be the venue's is worse than no pool.
    #[test]
    fn an_untracked_curve_stops_quoting() {
        let mut state = live_state();
        state.curve_tracked = false;
        let stable = Bytes::from(state.stable.to_vec());
        let volatile = Bytes::from(state.volatile.to_vec());
        for (sell, buy) in [(stable.clone(), volatile.clone()), (volatile, stable)] {
            assert_eq!(
                state
                    .get_limits(sell, buy)
                    .expect("limits are computable"),
                (BigUint::ZERO, BigUint::ZERO)
            );
        }
    }

    /// A curve word the quote path divides by or interpolates through must be rejected
    /// before it reaches the curve, not discovered inside it.
    #[test]
    fn a_degenerate_curve_is_not_quotable() {
        assert!(berachain_curve().usable());
        // Zero width divides by zero in the Bezier normalisation; zero joins collapse the
        // half-law segment; a debt at the wall above the wall itself takes the recovery
        // continuation's square root negative.
        let mut zero_width = berachain_curve();
        zero_width.width = BigUint::ZERO;
        let mut zero_join = berachain_curve();
        zero_join.h_join = BigUint::ZERO;
        let mut zero_wall = berachain_curve();
        zero_wall.h_wall = BigUint::ZERO;
        let mut inverted_wall = berachain_curve();
        inverted_wall.d_wall = &inverted_wall.h_wall + BigUint::from(1u8);
        for curve in [zero_width, zero_join, zero_wall, inverted_wall] {
            assert!(!curve.usable());
        }
    }

    #[test]
    fn quotes_match_shipped_library_fixture_grid() {
        let curve = berachain_curve();
        let mut cases = 0usize;
        let mut recovery_quoted = 0usize;
        for line in FIXTURES.lines().skip(1) {
            let cols: Vec<&str> = line.split(',').collect();
            let (collateral, debt, price) = (u(cols[1]), u(cols[2]), u(cols[3]));
            let (spread, amount_in) = (u(cols[4]), u(cols[5]));
            let (out, new_coll, new_debt) = if cols[0] == "L" {
                curve.leverage_quote(
                    &collateral,
                    &debt,
                    &price,
                    &curve.leverage_ratio_wad.clone(),
                    &spread,
                    &amount_in,
                )
            } else {
                let result = curve.deleverage_quote(
                    &collateral,
                    &debt,
                    &price,
                    &curve.leverage_ratio_wad.clone(),
                    &spread,
                    &amount_in,
                );
                if result.0 != BigUint::ZERO &&
                    curve.state_region(&collateral, &debt, &price) ==
                        crate::evm::protocol::everlong_rebalancer::math::Region::Recovery
                {
                    recovery_quoted += 1;
                }
                result
            };
            assert_eq!(out, u(cols[6]), "out mismatch: {line}");
            assert_eq!(new_coll, u(cols[7]), "newColl mismatch: {line}");
            assert_eq!(new_debt, u(cols[8]), "newDebt mismatch: {line}");

            let pre_anchor = curve.x_anchor_for_state(&collateral, &debt, &price);
            assert_eq!(pre_anchor, u(cols[9]), "preAnchor mismatch: {line}");
            assert_eq!(
                curve.is_state_safe(&new_coll, &new_debt, &price, &pre_anchor),
                cols[10] == "1",
                "isStateSafe mismatch: {line}"
            );
            cases += 1;
        }
        assert!(cases > 750, "expected the fixture rows to load, got {cases}");
        assert!(recovery_quoted > 0, "fixtures must exercise the recovery continuation");
    }

    #[test]
    fn preview_token_amounts_matches_venue_oracle() {
        let snapshot: Value = serde_json::from_str(LIVE_SNAPSHOT).unwrap();
        let vault = live_vault();
        let mut cases = 0usize;
        for row in snapshot["previewOracle"]
            .as_array()
            .unwrap()
        {
            let shares = json_uint(&row["shares"]);
            let mint = row["mint"].as_bool().unwrap();
            let (stable, volatile, ok) = vault.preview_token_amounts(&shares, mint);
            assert!(ok, "preview must not degenerate: {row}");
            assert_eq!(stable, json_uint(&row["stable"]), "stable leg: {row}");
            assert_eq!(volatile, json_uint(&row["volatile"]), "volatile leg: {row}");
            cases += 1;
        }
        assert!(cases >= 10);
    }

    /// Share-sized oracle rows: the venue's `swapVolatileForStable` is sized in
    /// CollVault shares, so parity is asserted at the share lot (the volatile-budget
    /// inversion may legitimately pick a larger lot fitting the same budget).
    #[test]
    fn leverage_executions_settle_wei_exact() {
        let snapshot: Value = serde_json::from_str(LIVE_SNAPSHOT).unwrap();
        let curve = berachain_curve();
        let vault = live_vault();
        let max_lot = curve.max_leverage_shares(&vault);
        for row in snapshot["leverageExecutions"]
            .as_array()
            .unwrap()
        {
            let shares = json_uint(&row["shares"]);
            let (_, volatile_in, ok) = vault.preview_token_amounts(&shares, true);
            assert!(ok, "preview degenerated: {row}");
            assert_eq!(volatile_in, json_uint(&row["volatileIn"]), "volatileIn: {row}");
            let net = curve
                .quote_leverage_at(&vault, &shares)
                .expect("venue settled this fill");
            assert_eq!(net, json_uint(&row["netStableOut"]), "netStableOut: {row}");
            assert!(shares <= max_lot, "settled lot must sit within the max lot: {row}");
        }
        // A lot above the physical-CR cap reverts on-chain, so the cap must exclude it.
        let reverting_lot = json_uint(&snapshot["leverageRevertsAboveMaxLot"]);
        assert!(reverting_lot > max_lot);
    }

    #[test]
    fn deleverage_executions_settle_wei_exact() {
        let snapshot: Value = serde_json::from_str(LIVE_SNAPSHOT).unwrap();
        let state = live_state();
        for row in snapshot["deleverageExecutions"]
            .as_array()
            .unwrap()
        {
            let net_in = json_uint(&row["netStableIn"]);
            let quote = state
                .quote_deleverage(&net_in)
                .expect("venue settled this fill");
            assert_eq!(quote.amount_out, json_uint(&row["volatileOut"]), "volatileOut: {row}");
            assert_eq!(quote.used_in, net_in, "net front: {row}");
        }
    }

    #[test]
    fn max_deleverage_matches_cdp_bound() {
        let snapshot: Value = serde_json::from_str(LIVE_SNAPSHOT).unwrap();
        let curve = berachain_curve();
        let vault = live_vault();
        assert_eq!(
            curve.max_deleverage_in(&vault),
            json_uint(&snapshot["maxDeleverageInExpected"])
        );
        // Repaying the full debt violates the CDP min-net-debt bound.
        let full = json_uint(&snapshot["deleverageRevertsAtFullDebt"]);
        let (out, _, _) = curve.deleverage_quote_checked(&vault, &full);
        // The raw curve may quote it, but the sized path never exceeds the ceiling.
        assert!(curve.max_deleverage_in(&vault) < full || out == BigUint::ZERO);
    }

    #[test]
    fn max_leverage_matches_venue_boundary() {
        let snapshot: Value = serde_json::from_str(LIVE_SNAPSHOT).unwrap();
        let curve = berachain_curve();
        let mut vault = live_vault();
        vault.ref_raw_reference_wad = json_uint(&snapshot["maxLotBoundaryRefRawReferenceWad"]);
        assert_eq!(
            curve.max_leverage_shares(&vault),
            json_uint(&snapshot["maxLotBoundaryExpected"])
        );
    }

    #[test]
    fn get_limits_are_positive_both_directions() {
        let state = live_state();
        let stable = Bytes::from(state.stable.to_vec());
        let volatile = Bytes::from(state.volatile.to_vec());
        let (lev_in, lev_out) = state
            .get_limits(volatile.clone(), stable.clone())
            .unwrap();
        let (del_in, del_out) = state
            .get_limits(stable, volatile)
            .unwrap();
        // At this live state the venue quotes both directions (spread-priced).
        assert!(lev_in > BigUint::ZERO && lev_out > BigUint::ZERO);
        assert!(del_in > BigUint::ZERO && del_out > BigUint::ZERO);
    }
}
