use tycho_substreams::prelude as tycho;

use crate::{
    cvamm::{
        events::{event_attributes, CvammEvent},
        state::{attrs, creation_attribute},
    },
    modules::config::AlmConfig,
};

/// Initial attribute set for a newly tracked ALM. Values that base blocks cannot
/// observe at bootstrap (the initialize-time curve config and fee levels) come from
/// the substreams params — the configurable-constants channel — and are corrected by
/// the first on-chain event that carries them.
pub fn initial_entity_change(component_id: &str, alm: &AlmConfig) -> tycho::EntityChanges {
    tycho::EntityChanges {
        component_id: component_id.to_owned(),
        attributes: vec![
            creation_attribute(attrs::X_WAD, vec![0u8]),
            creation_attribute(attrs::KAPPA, vec![0u8]),
            creation_attribute(attrs::ANCHOR_SQRT_CURVE_X96, vec![0u8]),
            creation_attribute(attrs::A_WAD, alm.a_wad.clone()),
            creation_attribute(attrs::SPAN_UP_WAD, alm.span_up_wad.clone()),
            creation_attribute(attrs::SPAN_DN_WAD, alm.span_dn_wad.clone()),
            creation_attribute(attrs::RESERVE_STABLE, vec![0u8]),
            creation_attribute(attrs::RESERVE_VOLATILE, vec![0u8]),
            creation_attribute(attrs::FEE_STABLE_IN_WAD, alm.initial_fee_wad.clone()),
            creation_attribute(attrs::FEE_VOLATILE_IN_WAD, alm.initial_fee_wad.clone()),
            creation_attribute(attrs::PAUSED, vec![0u8]),
            creation_attribute(attrs::RETRACTED, vec![0u8]),
        ],
    }
}

pub fn entity_change_for_event(
    component_id: &str,
    event: &CvammEvent,
) -> Option<tycho::EntityChanges> {
    let attributes = event_attributes(event);
    if attributes.is_empty() {
        return None;
    }
    Some(tycho::EntityChanges { component_id: component_id.to_owned(), attributes })
}

/// `Sync` fires after every state change and carries absolute totals (including
/// idle) for both legs — exactly what Tycho wants as component balances.
pub fn balance_changes_for_event(
    component_id: &str,
    stable: &[u8],
    volatile: &[u8],
    event: &CvammEvent,
) -> Vec<tycho::BalanceChange> {
    let CvammEvent::Sync { reserve0, reserve1 } = event else {
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
