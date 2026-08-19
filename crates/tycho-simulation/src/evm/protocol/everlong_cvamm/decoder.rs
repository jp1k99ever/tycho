use std::collections::HashMap;

use alloy::primitives::U256;
use num_bigint::BigUint;
use tycho_client::feed::{synchronizer::ComponentWithState, BlockHeader};
use tycho_common::{models::token::Token, Bytes};

use super::{
    fee_law::FeeLawTerms,
    state::{Address, EverlongCvammState},
};
use crate::protocol::{
    errors::InvalidSnapshotError,
    models::{DecoderContext, TryFromWithBlock},
};

mod attrs {
    pub const X_WAD: &str = "x_wad";
    pub const KAPPA: &str = "kappa";
    pub const ANCHOR_SQRT_CURVE_X96: &str = "anchor_sqrt_curve_x96";
    pub const A_WAD: &str = "a_wad";
    pub const SPAN_UP_WAD: &str = "span_up_wad";
    pub const SPAN_DN_WAD: &str = "span_dn_wad";
    pub const RESERVE_STABLE: &str = "reserve_stable";
    pub const RESERVE_VOLATILE: &str = "reserve_volatile";
    pub const FEE_STABLE_IN_WAD: &str = "fee_stable_in_wad";
    pub const FEE_VOLATILE_IN_WAD: &str = "fee_volatile_in_wad";
    pub const PAUSED: &str = "paused";
    pub const RETRACTED: &str = "retracted";

    pub const FEE_LAW_TRACKED: &str = "fee_law_tracked";
    pub const MID_FEE_WAD: &str = "mid_fee_wad";
    pub const OUT_FEE_WAD: &str = "out_fee_wad";
    pub const CURVATURE_WAD: &str = "curvature_wad";
    pub const LP_FEE_WAD: &str = "lp_fee_wad";
    pub const DIR_SKEW_WAD: &str = "dir_skew_wad";
    pub const INV_SKEW_KAPPA_WAD: &str = "inv_skew_kappa_wad";
    pub const INV_SKEW_BAND_WAD: &str = "inv_skew_band_wad";
    pub const VOL_SIGMA_REF_WAD: &str = "vol_sigma_ref_wad";
    pub const VOL_BETA_WAD: &str = "vol_beta_wad";
    pub const VOL_MIN_WAD: &str = "vol_min_wad";
    pub const VOL_MAX_WAD: &str = "vol_max_wad";
    pub const FLOOR_STABLE_IN_WAD: &str = "floor_stable_in_wad";
    pub const FLOOR_VOLATILE_IN_WAD: &str = "floor_volatile_in_wad";

    /// Every term the fee law reads. All or nothing: a snapshot missing one of them
    /// carries no law at all, and the simulation falls back to the conservative fold.
    pub const FEE_LAW: &[&str] = &[
        MID_FEE_WAD,
        OUT_FEE_WAD,
        CURVATURE_WAD,
        LP_FEE_WAD,
        DIR_SKEW_WAD,
        INV_SKEW_KAPPA_WAD,
        INV_SKEW_BAND_WAD,
        VOL_SIGMA_REF_WAD,
        VOL_BETA_WAD,
        VOL_MIN_WAD,
        VOL_MAX_WAD,
        FLOOR_STABLE_IN_WAD,
        FLOOR_VOLATILE_IN_WAD,
    ];
}

impl TryFromWithBlock<ComponentWithState, BlockHeader> for EverlongCvammState {
    type Error = InvalidSnapshotError;

    async fn try_from_with_header(
        snapshot: ComponentWithState,
        _block: BlockHeader,
        _account_balances: &HashMap<Bytes, HashMap<Bytes, Bytes>>,
        _all_tokens: &HashMap<Bytes, Token>,
        _decoder_context: &DecoderContext,
    ) -> Result<Self, Self::Error> {
        decode_cvamm_snapshot(&snapshot)
    }
}

pub fn decode_cvamm_snapshot(
    snapshot: &ComponentWithState,
) -> Result<EverlongCvammState, InvalidSnapshotError> {
    let attributes = &snapshot.state.attributes;

    let mut state = EverlongCvammState {
        alm: address_from_component_id(&snapshot.component.id)?,
        stable: component_token(snapshot, 0)?,
        volatile: component_token(snapshot, 1)?,
        x_wad: decode_u256(attrs::X_WAD, required_attr(attributes, attrs::X_WAD)?)?,
        kappa: decode_u256(attrs::KAPPA, required_attr(attributes, attrs::KAPPA)?)?,
        anchor_sqrt_x96: decode_u256(
            attrs::ANCHOR_SQRT_CURVE_X96,
            required_attr(attributes, attrs::ANCHOR_SQRT_CURVE_X96)?,
        )?,
        a_wad: decode_u256(attrs::A_WAD, required_attr(attributes, attrs::A_WAD)?)?,
        span_up_wad: decode_u256(
            attrs::SPAN_UP_WAD,
            required_attr(attributes, attrs::SPAN_UP_WAD)?,
        )?,
        span_dn_wad: decode_u256(
            attrs::SPAN_DN_WAD,
            required_attr(attributes, attrs::SPAN_DN_WAD)?,
        )?,
        support: Default::default(),
        reserve_stable: decode_u256(
            attrs::RESERVE_STABLE,
            required_attr(attributes, attrs::RESERVE_STABLE)?,
        )?,
        reserve_volatile: decode_u256(
            attrs::RESERVE_VOLATILE,
            required_attr(attributes, attrs::RESERVE_VOLATILE)?,
        )?,
        fee_stable_in_wad: decode_u256(
            attrs::FEE_STABLE_IN_WAD,
            required_attr(attributes, attrs::FEE_STABLE_IN_WAD)?,
        )?,
        fee_volatile_in_wad: decode_u256(
            attrs::FEE_VOLATILE_IN_WAD,
            required_attr(attributes, attrs::FEE_VOLATILE_IN_WAD)?,
        )?,
        fee_law: decode_fee_law(attributes)?,
        fee_law_tracked: attributes
            .get(attrs::FEE_LAW_TRACKED)
            .map(|value| decode_bool(attrs::FEE_LAW_TRACKED, value))
            .transpose()?
            .unwrap_or(true),
        fee_solve: Default::default(),
        paused: decode_bool(attrs::PAUSED, required_attr(attributes, attrs::PAUSED)?)?,
        retracted: decode_bool(attrs::RETRACTED, required_attr(attributes, attrs::RETRACTED)?)?,
    };
    // An unexpressible or placeholder curve config marks the venue retracted (not
    // quotable) rather than failing the snapshot; a later retune delta heals it.
    let was_retracted = state.retracted;
    state.rebuild_support();
    state.retracted = state.retracted || was_retracted;
    Ok(state)
}

pub fn apply_delta(
    state: &mut EverlongCvammState,
    updated_attributes: HashMap<String, Bytes>,
) -> Result<(), InvalidSnapshotError> {
    let mut support_changed = false;
    for (name, value) in updated_attributes {
        match name.as_str() {
            attrs::X_WAD => state.x_wad = decode_u256(attrs::X_WAD, &value)?,
            attrs::KAPPA => state.kappa = decode_u256(attrs::KAPPA, &value)?,
            attrs::ANCHOR_SQRT_CURVE_X96 => {
                state.anchor_sqrt_x96 = decode_u256(attrs::ANCHOR_SQRT_CURVE_X96, &value)?
            }
            attrs::A_WAD => {
                state.a_wad = decode_u256(attrs::A_WAD, &value)?;
                support_changed = true;
            }
            attrs::SPAN_UP_WAD => {
                state.span_up_wad = decode_u256(attrs::SPAN_UP_WAD, &value)?;
                support_changed = true;
            }
            attrs::SPAN_DN_WAD => {
                state.span_dn_wad = decode_u256(attrs::SPAN_DN_WAD, &value)?;
                support_changed = true;
            }
            attrs::RESERVE_STABLE => {
                state.reserve_stable = decode_u256(attrs::RESERVE_STABLE, &value)?
            }
            attrs::RESERVE_VOLATILE => {
                state.reserve_volatile = decode_u256(attrs::RESERVE_VOLATILE, &value)?
            }
            attrs::FEE_STABLE_IN_WAD => {
                state.fee_stable_in_wad = decode_u256(attrs::FEE_STABLE_IN_WAD, &value)?
            }
            attrs::FEE_VOLATILE_IN_WAD => {
                state.fee_volatile_in_wad = decode_u256(attrs::FEE_VOLATILE_IN_WAD, &value)?
            }
            attrs::FEE_LAW_TRACKED => {
                state.fee_law_tracked = decode_bool(attrs::FEE_LAW_TRACKED, &value)?
            }
            attrs::PAUSED => state.paused = decode_bool(attrs::PAUSED, &value)?,
            attrs::RETRACTED => state.retracted = decode_bool(attrs::RETRACTED, &value)?,
            _ => {}
        }
    }
    if support_changed {
        state.rebuild_support();
    }
    // The vol scalar is solved from the fee samples at ONE book and is only valid for it.
    // A delta brings fresh samples and a fresh book, so the cached solve is stale — drop
    // it and let the next fill solve again. Carrying it across a FILL is the point (see
    // `EverlongCvammState::apply_quote`); carrying it across a BLOCK is not.
    state.fee_solve = Default::default();
    Ok(())
}

/// The fee law's terms, or `None` when the snapshot predates them or carries only some.
///
/// The law is not partially evaluable — a missing term is a missing multiply, not a zero —
/// so an incomplete set disables the re-derivation rather than being filled in.
fn decode_fee_law(
    attributes: &HashMap<String, Bytes>,
) -> Result<Option<FeeLawTerms>, InvalidSnapshotError> {
    if !attrs::FEE_LAW
        .iter()
        .all(|name| attributes.contains_key(*name))
    {
        return Ok(None);
    }
    let term = |name: &'static str| -> Result<BigUint, InvalidSnapshotError> {
        Ok(BigUint::from_bytes_be(required_attr(attributes, name)?.as_ref()))
    };
    Ok(Some(FeeLawTerms {
        mid_fee_wad: term(attrs::MID_FEE_WAD)?,
        out_fee_wad: term(attrs::OUT_FEE_WAD)?,
        curvature_wad: term(attrs::CURVATURE_WAD)?,
        lp_fee_wad: term(attrs::LP_FEE_WAD)?,
        dir_skew_wad: term(attrs::DIR_SKEW_WAD)?,
        inv_skew_kappa_wad: term(attrs::INV_SKEW_KAPPA_WAD)?,
        inv_skew_band_wad: term(attrs::INV_SKEW_BAND_WAD)?,
        vol_sigma_ref_wad: term(attrs::VOL_SIGMA_REF_WAD)?,
        vol_beta_wad: term(attrs::VOL_BETA_WAD)?,
        vol_min_wad: term(attrs::VOL_MIN_WAD)?,
        vol_max_wad: term(attrs::VOL_MAX_WAD)?,
        floor_stable_in_wad: term(attrs::FLOOR_STABLE_IN_WAD)?,
        floor_volatile_in_wad: term(attrs::FLOOR_VOLATILE_IN_WAD)?,
    }))
}

fn required_attr<'a>(
    attributes: &'a HashMap<String, Bytes>,
    name: &'static str,
) -> Result<&'a Bytes, InvalidSnapshotError> {
    attributes
        .get(name)
        .ok_or_else(|| InvalidSnapshotError::MissingAttribute(name.to_owned()))
}

fn decode_u256(name: &'static str, value: &Bytes) -> Result<U256, InvalidSnapshotError> {
    if value.len() > 32 {
        return Err(InvalidSnapshotError::ValueError(format!(
            "attribute {name} exceeds 32 bytes: {}",
            value.len()
        )));
    }
    Ok(U256::from_be_slice(value.as_ref()))
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
