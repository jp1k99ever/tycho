use tycho_substreams::prelude as tycho;

use crate::{
    modules::config::VenueConfig,
    rebalancer::{
        events::{event_attributes, RebalancerEvent},
        state::{attrs, creation_attribute},
    },
};

/// Initial attribute set for the venue. The deployment-frozen curve constants come
/// from the substreams params — the configurable-constants channel — and the mutable
/// exchange/aggregate words start at zero until the first fill or `StateSync` event.
/// `price_wad = 0` signals "reservation price unobserved"; the simulation must not
/// quote until it turns nonzero.
pub fn initial_entity_change(component_id: &str, venue: &VenueConfig) -> tycho::EntityChanges {
    tycho::EntityChanges {
        component_id: component_id.to_owned(),
        attributes: vec![
            creation_attribute(attrs::COLLATERAL, vec![0u8]),
            creation_attribute(attrs::DEBT, vec![0u8]),
            creation_attribute(attrs::PRICE_WAD, vec![0u8]),
            creation_attribute(attrs::SPREAD_PPM, vec![0u8]),
            creation_attribute(attrs::ALM_STABLE_RESERVE, vec![0u8]),
            creation_attribute(attrs::ALM_VOLATILE_RESERVE, vec![0u8]),
            creation_attribute(attrs::ALM_SUPPLY, vec![0u8]),
            creation_attribute(attrs::CV_TOTAL_ASSETS, vec![0u8]),
            creation_attribute(attrs::CV_TOTAL_SUPPLY, vec![0u8]),
            creation_attribute(attrs::WITHDRAW_FEE_BP, vec![0u8]),
            creation_attribute(attrs::INTEREST_RATE, vec![0u8]),
            creation_attribute(attrs::REF_STABLE_RESERVE, vec![0u8]),
            creation_attribute(attrs::REF_ASSET_RESERVE, vec![0u8]),
            creation_attribute(attrs::REF_RAW_REFERENCE_WAD, vec![0u8]),
            creation_attribute(attrs::CURVE_TRACKED, vec![1u8]),
            creation_attribute(attrs::CV_DECIMALS_OFFSET, venue.cv_decimals_offset.clone()),
            creation_attribute(attrs::MIN_NET_DEBT, venue.min_net_debt.clone()),
            creation_attribute(attrs::DEBT_GAS_COMPENSATION, venue.debt_gas_compensation.clone()),
            creation_attribute(attrs::MCR_WAD, venue.mcr_wad.clone()),
            creation_attribute(attrs::LEVERAGE_RATIO_WAD, venue.leverage_ratio_wad.clone()),
            creation_attribute(attrs::H_ZERO, venue.h_zero.clone()),
            creation_attribute(attrs::H_JOIN, venue.h_join.clone()),
            creation_attribute(attrs::H_WALL, venue.h_wall.clone()),
            creation_attribute(attrs::WIDTH, venue.width.clone()),
            creation_attribute(attrs::D_JOIN, venue.d_join.clone()),
            creation_attribute(attrs::D_WALL, venue.d_wall.clone()),
            creation_attribute(attrs::RESCUE_SPREAD_PPM, venue.rescue_spread_ppm.clone()),
            creation_attribute(attrs::PHYSICAL_CR_FLOOR_WAD, venue.physical_cr_floor_wad.clone()),
            creation_attribute(attrs::BEZIER_PHI, venue.bezier_phi.clone()),
            creation_attribute(attrs::BEZIER_INTEGRAL, venue.bezier_integral.clone()),
        ],
    }
}

pub fn entity_change_for_event(
    component_id: &str,
    venue: &VenueConfig,
    event: &RebalancerEvent,
) -> Option<tycho::EntityChanges> {
    let attributes = event_attributes(event, &venue.rebalancer_impl);
    if attributes.is_empty() {
        return None;
    }
    Some(tycho::EntityChanges { component_id: component_id.to_owned(), attributes })
}

/// The ALM's `Sync` totals (both legs, including idle) are the closest on-log proxy
/// for the venue's backing liquidity; they serve as the component balances for TVL
/// gating, matching how the existing router integration reports reserves.
pub fn balance_changes_for_event(
    component_id: &str,
    stable: &[u8],
    volatile: &[u8],
    event: &RebalancerEvent,
) -> Vec<tycho::BalanceChange> {
    let RebalancerEvent::AlmSync { reserve0, reserve1 } = event else {
        return vec![];
    };
    vec![
        balance_change(component_id, stable, reserve0.clone()),
        balance_change(component_id, volatile, reserve1.clone()),
    ]
}

fn balance_change(component_id: &str, token: &[u8], balance: Vec<u8>) -> tycho::BalanceChange {
    tycho::BalanceChange {
        token: token.to_vec(),
        balance,
        component_id: component_id.as_bytes().to_vec(),
    }
}
