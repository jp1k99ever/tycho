use substreams_ethereum::pb::eth;
use tycho_substreams::prelude as tycho;

use crate::{
    abi::{alm::events as alm_events, rebalancer::events as rebalancer_events},
    collvault::state::{attribute, attrs},
};

/// Decoded state-relevant events for the CollVault venue.
///
/// Exchange words come from the rebalancer's fill events today; the aggregate words
/// (ALM/vault/reference/interest) only from the proposed `StateSync` snapshot. The
/// ALM's `Sync` totals serve as the component's balances.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CollVaultEvent {
    LeverageChanged { collateral: Vec<u8>, debt: Vec<u8>, spread_ppm: Vec<u8> },
    StateSync(Box<StateSyncEvent>),
    AlmSync { reserve0: Vec<u8>, reserve1: Vec<u8> },
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

pub fn decode_rebalancer_log(log: &eth::v2::Log) -> Option<CollVaultEvent> {
    use substreams_ethereum::Event as _;

    if log.topics.is_empty() {
        return None;
    }
    if let Some(event) = rebalancer_events::LeverageIncreased::match_and_decode(log) {
        return Some(CollVaultEvent::LeverageChanged {
            collateral: be_bytes(&event.new_coll),
            debt: be_bytes(&event.new_debt),
            spread_ppm: be_bytes(&event.spread_ppm),
        });
    }
    if let Some(event) = rebalancer_events::LeverageDecreased::match_and_decode(log) {
        return Some(CollVaultEvent::LeverageChanged {
            collateral: be_bytes(&event.new_coll),
            debt: be_bytes(&event.new_debt),
            spread_ppm: be_bytes(&event.spread_ppm),
        });
    }
    if let Some(event) = rebalancer_events::StateSync::match_and_decode(log) {
        return Some(CollVaultEvent::StateSync(Box::new(StateSyncEvent {
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
    None
}

pub fn decode_alm_log(log: &eth::v2::Log) -> Option<CollVaultEvent> {
    use substreams_ethereum::Event as _;

    if log.topics.is_empty() {
        return None;
    }
    let event = alm_events::Sync::match_and_decode(log)?;
    Some(CollVaultEvent::AlmSync {
        reserve0: be_bytes(&event.reserve0),
        reserve1: be_bytes(&event.reserve1),
    })
}

pub fn event_attributes(event: &CollVaultEvent) -> Vec<tycho::Attribute> {
    match event {
        CollVaultEvent::LeverageChanged { collateral, debt, spread_ppm } => vec![
            attribute(attrs::COLLATERAL, collateral.clone()),
            attribute(attrs::DEBT, debt.clone()),
            attribute(attrs::SPREAD_PPM, spread_ppm.clone()),
        ],
        CollVaultEvent::StateSync(snapshot) => vec![
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
        CollVaultEvent::AlmSync { .. } => vec![],
    }
}

/// Unsigned big-endian bytes for an ABI-decoded unsigned integer.
pub(crate) fn be_bytes(value: &substreams::scalar::BigInt) -> Vec<u8> {
    let (sign, bytes) = value.to_bytes_be();
    debug_assert!(sign != num_bigint::Sign::Minus, "unsigned ABI value decoded negative");
    bytes
}
