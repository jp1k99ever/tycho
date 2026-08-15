use std::collections::HashMap;

use num_bigint::BigUint;
use tycho_client::feed::{synchronizer::ComponentWithState, BlockHeader};
use tycho_common::{models::token::Token, Bytes};

use super::state::{Address, EverlongRebalancerState};
use crate::protocol::{
    errors::InvalidSnapshotError,
    models::{DecoderContext, TryFromWithBlock},
};

mod attrs {
    // Mutable exchange/aggregate words.
    pub const COLLATERAL: &str = "collateral";
    pub const DEBT: &str = "debt";
    pub const PRICE_WAD: &str = "price_wad";
    pub const SPREAD_PPM: &str = "spread_ppm";
    pub const ALM_STABLE_RESERVE: &str = "alm_stable_reserve";
    pub const ALM_VOLATILE_RESERVE: &str = "alm_volatile_reserve";
    pub const ALM_SUPPLY: &str = "alm_supply";
    pub const CV_TOTAL_ASSETS: &str = "cv_total_assets";
    pub const CV_TOTAL_SUPPLY: &str = "cv_total_supply";
    pub const WITHDRAW_FEE_BP: &str = "withdraw_fee_bp";
    pub const INTEREST_RATE: &str = "interest_rate";
    pub const REF_STABLE_RESERVE: &str = "ref_stable_reserve";
    pub const REF_ASSET_RESERVE: &str = "ref_asset_reserve";
    pub const REF_RAW_REFERENCE_WAD: &str = "ref_raw_reference_wad";
    // Optional acceptance-fidelity words (zero when the feed does not carry them).
    pub const RVPS_WAD: &str = "rvps_wad";
    pub const ICR_PRICE_WAD: &str = "icr_price_wad";
    // Deployment-frozen words.
    pub const CV_DECIMALS_OFFSET: &str = "cv_decimals_offset";
    pub const MIN_NET_DEBT: &str = "min_net_debt";
    pub const DEBT_GAS_COMPENSATION: &str = "debt_gas_compensation";
    pub const MCR_WAD: &str = "mcr_wad";
    // Curve constants.
    pub const LEVERAGE_RATIO_WAD: &str = "leverage_ratio_wad";
    pub const H_ZERO: &str = "h_zero";
    pub const H_JOIN: &str = "h_join";
    pub const H_WALL: &str = "h_wall";
    pub const WIDTH: &str = "width";
    pub const D_JOIN: &str = "d_join";
    pub const D_WALL: &str = "d_wall";
    pub const RESCUE_SPREAD_PPM: &str = "rescue_spread_ppm";
    pub const PHYSICAL_CR_FLOOR_WAD: &str = "physical_cr_floor_wad";
    pub const BEZIER_PHI: &str = "bezier_phi";
    pub const BEZIER_INTEGRAL: &str = "bezier_integral";
    pub const CURVE_TRACKED: &str = "curve_tracked";
}

impl TryFromWithBlock<ComponentWithState, BlockHeader> for EverlongRebalancerState {
    type Error = InvalidSnapshotError;

    async fn try_from_with_header(
        snapshot: ComponentWithState,
        _block: BlockHeader,
        _account_balances: &HashMap<Bytes, HashMap<Bytes, Bytes>>,
        _all_tokens: &HashMap<Bytes, Token>,
        _decoder_context: &DecoderContext,
    ) -> Result<Self, Self::Error> {
        decode_rebalancer_snapshot(&snapshot)
    }
}

pub fn decode_rebalancer_snapshot(
    snapshot: &ComponentWithState,
) -> Result<EverlongRebalancerState, InvalidSnapshotError> {
    let attributes = &snapshot.state.attributes;
    let required = |name: &'static str| -> Result<BigUint, InvalidSnapshotError> {
        attributes
            .get(name)
            .ok_or_else(|| InvalidSnapshotError::MissingAttribute(name.to_owned()))
            .map(|value| BigUint::from_bytes_be(value.as_ref()))
    };
    let optional = |name: &'static str| -> BigUint {
        attributes
            .get(name)
            .map(|value| BigUint::from_bytes_be(value.as_ref()))
            .unwrap_or_default()
    };

    let mut state = EverlongRebalancerState {
        swapper: address_from_component_id(&snapshot.component.id)?,
        stable: component_token(snapshot, 0)?,
        volatile: component_token(snapshot, 1)?,
        vault: super::math::VaultState {
            collateral: required(attrs::COLLATERAL)?,
            debt: required(attrs::DEBT)?,
            price_wad: required(attrs::PRICE_WAD)?,
            spread_ppm: required(attrs::SPREAD_PPM)?,
            alm_stable_reserve: required(attrs::ALM_STABLE_RESERVE)?,
            alm_volatile_reserve: required(attrs::ALM_VOLATILE_RESERVE)?,
            alm_supply: required(attrs::ALM_SUPPLY)?,
            cv_total_assets: required(attrs::CV_TOTAL_ASSETS)?,
            cv_total_supply: required(attrs::CV_TOTAL_SUPPLY)?,
            cv_decimals_offset: decode_u8(
                attrs::CV_DECIMALS_OFFSET,
                &required(attrs::CV_DECIMALS_OFFSET)?,
            )?,
            withdraw_fee_bp: required(attrs::WITHDRAW_FEE_BP)?,
            interest_rate: required(attrs::INTEREST_RATE)?,
            min_net_debt: required(attrs::MIN_NET_DEBT)?,
            debt_gas_compensation: required(attrs::DEBT_GAS_COMPENSATION)?,
            ref_stable_reserve: required(attrs::REF_STABLE_RESERVE)?,
            ref_asset_reserve: required(attrs::REF_ASSET_RESERVE)?,
            ref_raw_reference_wad: required(attrs::REF_RAW_REFERENCE_WAD)?,
            rvps_wad: optional(attrs::RVPS_WAD),
            mcr_wad: required(attrs::MCR_WAD)?,
            icr_price_wad: optional(attrs::ICR_PRICE_WAD),
        },
        curve: super::math::CurveParams {
            leverage_ratio_wad: required(attrs::LEVERAGE_RATIO_WAD)?,
            h_zero: required(attrs::H_ZERO)?,
            h_join: required(attrs::H_JOIN)?,
            h_wall: required(attrs::H_WALL)?,
            width: required(attrs::WIDTH)?,
            d_join: required(attrs::D_JOIN)?,
            d_wall: required(attrs::D_WALL)?,
            rescue_spread_ppm: required(attrs::RESCUE_SPREAD_PPM)?,
            bezier_phi: {
                let raw = required_bytes(attributes, attrs::BEZIER_PHI)?;
                decode_word_array::<4>(attrs::BEZIER_PHI, raw)?
            },
            bezier_integral: {
                let raw = required_bytes(attributes, attrs::BEZIER_INTEGRAL)?;
                decode_word_array::<5>(attrs::BEZIER_INTEGRAL, raw)?
            },
            physical_cr_floor_wad: required(attrs::PHYSICAL_CR_FLOOR_WAD)?,
        },
        // A snapshot predating the flag was written against the implementation the
        // constants came from, so it is tracked until an upgrade says otherwise.
        curve_tracked: attributes
            .get(attrs::CURVE_TRACKED)
            .map(|value| decode_bool(attrs::CURVE_TRACKED, value))
            .transpose()?
            .unwrap_or(true),
    };
    // A yet-unobserved reservation price is stored as zero; the region gate then
    // refuses to quote until the first StateSync arrives.
    if state.vault.price_wad == BigUint::ZERO {
        state.vault.collateral = BigUint::ZERO;
    }
    Ok(state)
}

pub fn apply_delta(
    state: &mut EverlongRebalancerState,
    updated_attributes: HashMap<String, Bytes>,
) -> Result<(), InvalidSnapshotError> {
    for (name, value) in updated_attributes {
        let uint = || BigUint::from_bytes_be(value.as_ref());
        let vault = &mut state.vault;
        match name.as_str() {
            attrs::CURVE_TRACKED => {
                state.curve_tracked = decode_bool(attrs::CURVE_TRACKED, &value)?
            }
            attrs::COLLATERAL => vault.collateral = uint(),
            attrs::DEBT => vault.debt = uint(),
            attrs::PRICE_WAD => vault.price_wad = uint(),
            attrs::SPREAD_PPM => vault.spread_ppm = uint(),
            attrs::ALM_STABLE_RESERVE => vault.alm_stable_reserve = uint(),
            attrs::ALM_VOLATILE_RESERVE => vault.alm_volatile_reserve = uint(),
            attrs::ALM_SUPPLY => vault.alm_supply = uint(),
            attrs::CV_TOTAL_ASSETS => vault.cv_total_assets = uint(),
            attrs::CV_TOTAL_SUPPLY => vault.cv_total_supply = uint(),
            attrs::WITHDRAW_FEE_BP => vault.withdraw_fee_bp = uint(),
            attrs::INTEREST_RATE => vault.interest_rate = uint(),
            attrs::MIN_NET_DEBT => vault.min_net_debt = uint(),
            attrs::DEBT_GAS_COMPENSATION => vault.debt_gas_compensation = uint(),
            attrs::REF_STABLE_RESERVE => vault.ref_stable_reserve = uint(),
            attrs::REF_ASSET_RESERVE => vault.ref_asset_reserve = uint(),
            attrs::REF_RAW_REFERENCE_WAD => vault.ref_raw_reference_wad = uint(),
            attrs::RVPS_WAD => vault.rvps_wad = uint(),
            attrs::ICR_PRICE_WAD => vault.icr_price_wad = uint(),
            attrs::MCR_WAD => vault.mcr_wad = uint(),
            attrs::CV_DECIMALS_OFFSET => {
                vault.cv_decimals_offset = decode_u8(attrs::CV_DECIMALS_OFFSET, &uint())?
            }
            attrs::LEVERAGE_RATIO_WAD => state.curve.leverage_ratio_wad = uint(),
            attrs::H_ZERO => state.curve.h_zero = uint(),
            attrs::H_JOIN => state.curve.h_join = uint(),
            attrs::H_WALL => state.curve.h_wall = uint(),
            attrs::WIDTH => state.curve.width = uint(),
            attrs::D_JOIN => state.curve.d_join = uint(),
            attrs::D_WALL => state.curve.d_wall = uint(),
            attrs::RESCUE_SPREAD_PPM => state.curve.rescue_spread_ppm = uint(),
            attrs::PHYSICAL_CR_FLOOR_WAD => state.curve.physical_cr_floor_wad = uint(),
            attrs::BEZIER_PHI => {
                state.curve.bezier_phi = decode_word_array::<4>(attrs::BEZIER_PHI, &value)?
            }
            attrs::BEZIER_INTEGRAL => {
                state.curve.bezier_integral =
                    decode_word_array::<5>(attrs::BEZIER_INTEGRAL, &value)?
            }
            _ => {}
        }
    }
    Ok(())
}

fn decode_bool(name: &'static str, value: &Bytes) -> Result<bool, InvalidSnapshotError> {
    if value.len() != 1 {
        return Err(InvalidSnapshotError::ValueError(format!(
            "attribute {name} has invalid length: expected 1, got {}",
            value.len()
        )));
    }
    Ok(value[0] != 0)
}

fn required_bytes<'a>(
    attributes: &'a HashMap<String, Bytes>,
    name: &'static str,
) -> Result<&'a Bytes, InvalidSnapshotError> {
    attributes
        .get(name)
        .ok_or_else(|| InvalidSnapshotError::MissingAttribute(name.to_owned()))
}

/// Concatenated 32-byte big-endian words.
fn decode_word_array<const N: usize>(
    name: &'static str,
    value: &Bytes,
) -> Result<[BigUint; N], InvalidSnapshotError> {
    let raw = value.as_ref();
    if raw.len() != N * 32 {
        return Err(InvalidSnapshotError::ValueError(format!(
            "attribute {name} must be {} bytes, got {}",
            N * 32,
            raw.len()
        )));
    }
    Ok(std::array::from_fn(|idx| BigUint::from_bytes_be(&raw[idx * 32..(idx + 1) * 32])))
}

fn decode_u8(name: &'static str, value: &BigUint) -> Result<u8, InvalidSnapshotError> {
    u8::try_from(value.clone()).map_err(|_| {
        InvalidSnapshotError::ValueError(format!("attribute {name} does not fit u8: {value}"))
    })
}

fn component_token(
    snapshot: &ComponentWithState,
    idx: usize,
) -> Result<Address, InvalidSnapshotError> {
    snapshot
        .component
        .tokens
        .get(idx)
        .map(|token| token.as_ref())
        .ok_or_else(|| InvalidSnapshotError::ValueError(format!("missing token index {idx}")))
        .and_then(|value| {
            value.try_into().map_err(|_| {
                InvalidSnapshotError::ValueError(format!(
                    "expected 20-byte address, got {}",
                    value.len()
                ))
            })
        })
}

fn address_from_component_id(value: &str) -> Result<Address, InvalidSnapshotError> {
    let value = value
        .strip_prefix("0x")
        .unwrap_or(value);
    let decoded = hex::decode(value).map_err(|err| {
        InvalidSnapshotError::ValueError(format!("invalid component id hex: {err}"))
    })?;
    decoded
        .as_slice()
        .try_into()
        .map_err(|_| {
            InvalidSnapshotError::ValueError(format!(
                "expected 20-byte component id, got {} bytes",
                decoded.len()
            ))
        })
}
