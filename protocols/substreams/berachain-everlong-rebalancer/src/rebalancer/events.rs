use substreams_ethereum::pb::eth;
use tycho_substreams::prelude as tycho;

use crate::{
    abi::{alm::events as alm_events, rebalancer::events as rebalancer_events},
    rebalancer::{
        state::{attribute, attrs},
        Address,
    },
};

/// Decoded state-relevant events for the CollateralRebalancer venue.
///
/// Exchange words come from the rebalancer's fill events today; the aggregate words
/// (ALM/vault/reference/interest) only from the proposed `StateSync` snapshot. The
/// ALM's `Sync` totals serve as the component's balances.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RebalancerEvent {
    LeverageChanged {
        collateral: Vec<u8>,
        debt: Vec<u8>,
        spread_ppm: Vec<u8>,
    },
    StateSync(Box<StateSyncEvent>),
    AlmSync {
        reserve0: Vec<u8>,
        reserve1: Vec<u8>,
    },
    /// The rebalancer proxy moved to another implementation, which is the only way the
    /// linked curve constants can change.
    Upgraded {
        implementation: Vec<u8>,
    },
}

/// Full snapshot carried by the rebalancer's `StateSync` event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StateSyncEvent {
    collateral: Vec<u8>,
    debt: Vec<u8>,
    price_wad: Vec<u8>,
    spread_ppm: Vec<u8>,
    alm_stable_reserve: Vec<u8>,
    alm_volatile_reserve: Vec<u8>,
    alm_supply: Vec<u8>,
    cv_total_assets: Vec<u8>,
    cv_total_supply: Vec<u8>,
    withdraw_fee_bp: Vec<u8>,
    interest_rate: Vec<u8>,
    ref_stable_reserve: Vec<u8>,
    ref_asset_reserve: Vec<u8>,
    ref_raw_reference_wad: Vec<u8>,
}

pub fn decode_rebalancer_log(log: &eth::v2::Log) -> Option<RebalancerEvent> {
    use substreams_ethereum::Event as _;

    if log.topics.is_empty() {
        return None;
    }
    if let Some(event) = rebalancer_events::LeverageIncreased::match_and_decode(log) {
        return Some(RebalancerEvent::LeverageChanged {
            collateral: be_bytes(&event.new_coll),
            debt: be_bytes(&event.new_debt),
            spread_ppm: be_bytes(&event.spread_ppm),
        });
    }
    if let Some(event) = rebalancer_events::LeverageDecreased::match_and_decode(log) {
        return Some(RebalancerEvent::LeverageChanged {
            collateral: be_bytes(&event.new_coll),
            debt: be_bytes(&event.new_debt),
            spread_ppm: be_bytes(&event.spread_ppm),
        });
    }
    if let Some(event) = rebalancer_events::StateSync::match_and_decode(log) {
        return Some(RebalancerEvent::StateSync(Box::new(StateSyncEvent {
            collateral: be_bytes(&event.collateral),
            debt: be_bytes(&event.debt),
            price_wad: be_bytes(&event.price_wad),
            spread_ppm: be_bytes(&event.spread_ppm),
            alm_stable_reserve: be_bytes(&event.alm_stable_reserve),
            alm_volatile_reserve: be_bytes(&event.alm_volatile_reserve),
            alm_supply: be_bytes(&event.alm_supply),
            cv_total_assets: be_bytes(&event.cv_total_assets),
            cv_total_supply: be_bytes(&event.cv_total_supply),
            withdraw_fee_bp: be_bytes(&event.withdraw_fee_bp),
            interest_rate: be_bytes(&event.interest_rate),
            ref_stable_reserve: be_bytes(&event.ref_stable_reserve),
            ref_asset_reserve: be_bytes(&event.ref_asset_reserve),
            ref_raw_reference_wad: be_bytes(&event.ref_raw_reference_wad),
        })));
    }
    if let Some(event) = rebalancer_events::Upgraded::match_and_decode(log) {
        return Some(RebalancerEvent::Upgraded { implementation: event.implementation.to_vec() });
    }
    None
}

pub fn decode_alm_log(log: &eth::v2::Log) -> Option<RebalancerEvent> {
    use substreams_ethereum::Event as _;

    if log.topics.is_empty() {
        return None;
    }
    let event = alm_events::Sync::match_and_decode(log)?;
    Some(RebalancerEvent::AlmSync {
        reserve0: be_bytes(&event.reserve0),
        reserve1: be_bytes(&event.reserve1),
    })
}

pub fn event_attributes(
    event: &RebalancerEvent,
    pinned_implementation: &Address,
) -> Vec<tycho::Attribute> {
    match event {
        RebalancerEvent::LeverageChanged { collateral, debt, spread_ppm } => vec![
            attribute(attrs::COLLATERAL, collateral.clone()),
            attribute(attrs::DEBT, debt.clone()),
            attribute(attrs::SPREAD_PPM, spread_ppm.clone()),
        ],
        RebalancerEvent::StateSync(snapshot) => vec![
            attribute(attrs::COLLATERAL, snapshot.collateral.clone()),
            attribute(attrs::DEBT, snapshot.debt.clone()),
            attribute(attrs::PRICE_WAD, snapshot.price_wad.clone()),
            attribute(attrs::SPREAD_PPM, snapshot.spread_ppm.clone()),
            attribute(attrs::ALM_STABLE_RESERVE, snapshot.alm_stable_reserve.clone()),
            attribute(attrs::ALM_VOLATILE_RESERVE, snapshot.alm_volatile_reserve.clone()),
            attribute(attrs::ALM_SUPPLY, snapshot.alm_supply.clone()),
            attribute(attrs::CV_TOTAL_ASSETS, snapshot.cv_total_assets.clone()),
            attribute(attrs::CV_TOTAL_SUPPLY, snapshot.cv_total_supply.clone()),
            attribute(attrs::WITHDRAW_FEE_BP, snapshot.withdraw_fee_bp.clone()),
            attribute(attrs::INTEREST_RATE, snapshot.interest_rate.clone()),
            attribute(attrs::REF_STABLE_RESERVE, snapshot.ref_stable_reserve.clone()),
            attribute(attrs::REF_ASSET_RESERVE, snapshot.ref_asset_reserve.clone()),
            attribute(attrs::REF_RAW_REFERENCE_WAD, snapshot.ref_raw_reference_wad.clone()),
        ],
        // ALM totals feed component balances, not state attributes.
        RebalancerEvent::AlmSync { .. } => vec![],
        RebalancerEvent::Upgraded { implementation } => {
            // An unpinned deployment (the zero address) treats every upgrade as a curve
            // change, because nothing has established which implementation the configured
            // constants were read from. Pinning one narrows that to a move away from it,
            // and lets a rollback restore quoting.
            let tracked = pinned_implementation
                .iter()
                .any(|byte| *byte != 0)
                && implementation.as_slice() == pinned_implementation;
            vec![attribute(attrs::CURVE_TRACKED, vec![u8::from(tracked)])]
        }
    }
}

/// Unsigned big-endian bytes for an ABI-decoded unsigned integer.
pub(crate) fn be_bytes(value: &substreams::scalar::BigInt) -> Vec<u8> {
    let (sign, bytes) = value.to_bytes_be();
    debug_assert!(sign != num_bigint::Sign::Minus, "unsigned ABI value decoded negative");
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tracked_flag(event: &RebalancerEvent, pinned: &Address) -> Option<Vec<u8>> {
        event_attributes(event, pinned)
            .into_iter()
            .find(|a| a.name == attrs::CURVE_TRACKED)
            .map(|a| a.value)
    }

    /// The curve constants are configuration read off ONE implementation, so an upgrade
    /// away from it must stop the simulation quoting until they are re-verified.
    #[test]
    fn upgrade_away_from_the_pinned_implementation_untracks_the_curve() {
        let pinned: Address = [0xABu8; 20];
        let upgrade = |to: u8| RebalancerEvent::Upgraded { implementation: vec![to; 20] };
        assert_eq!(tracked_flag(&upgrade(0xAB), &pinned), Some(vec![1u8]));
        assert_eq!(tracked_flag(&upgrade(0xCD), &pinned), Some(vec![0u8]));
    }

    /// Unpinned is the default, and it cannot vouch for any implementation — including
    /// the one an upgrade happens to land on.
    #[test]
    fn an_unpinned_deployment_untracks_on_any_upgrade() {
        let unpinned: Address = [0u8; 20];
        let event = RebalancerEvent::Upgraded { implementation: vec![0xABu8; 20] };
        assert_eq!(tracked_flag(&event, &unpinned), Some(vec![0u8]));
        let to_zero = RebalancerEvent::Upgraded { implementation: vec![0u8; 20] };
        assert_eq!(tracked_flag(&to_zero, &unpinned), Some(vec![0u8]));
    }
}
