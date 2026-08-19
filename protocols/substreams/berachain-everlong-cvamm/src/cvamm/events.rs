use substreams_ethereum::{pb::eth, Event};
use tycho_substreams::prelude as tycho;

use crate::{
    abi::cvamm_alm::events as alm_events,
    cvamm::{
        state::{attribute, attrs},
        Address,
    },
};

/// Decoded CVAMM state-relevant events.
///
/// `StateSync` is the authoritative full snapshot (contract-side addition; see the
/// integration notes). Until it is live on-chain, the remaining events provide the
/// exactly-derivable subset: kappa (every `Swap` carries it as `liquidity`), the
/// anchor/coordinate on recenters, curve params on retunes, and pause/retract flags.
/// `SwapFeeCharged` yields a per-direction fee approximation that is stale between
/// swaps — production indexing should start at (or after) the StateSync deployment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CvammEvent {
    StateSync {
        x_wad: Vec<u8>,
        kappa: Vec<u8>,
        reserve_stable: Vec<u8>,
        reserve_volatile: Vec<u8>,
        fee_stable_in_wad: Vec<u8>,
        fee_volatile_in_wad: Vec<u8>,
    },
    /// `Swap.liquidity` == kappa after the fill.
    Swap {
        kappa: Vec<u8>,
    },
    /// fee0 > 0 → fee taken on the stable leg → volatile-in swap (and vice versa).
    SwapFeeCharged {
        fee0_nonzero: bool,
        fee1_nonzero: bool,
        fee_wad: Vec<u8>,
    },
    /// Reserve totals INCLUDING idle — used for component balances (TVL), not for
    /// the solvency-clamp reserve attributes.
    Sync {
        reserve0: Vec<u8>,
        reserve1: Vec<u8>,
    },
    Recentered {
        anchor_after: Vec<u8>,
        kappa_after: Vec<u8>,
        x_after: Vec<u8>,
    },
    CurveRetuned {
        a_wad: Vec<u8>,
        span_up_wad: Vec<u8>,
        span_dn_wad: Vec<u8>,
    },
    IdleDeployed {
        kappa_after: Vec<u8>,
    },
    FeesCompounded {
        kappa_after: Vec<u8>,
    },
    ReservesRetracted,
    PauseSet {
        paused: bool,
    },
    /// The ALM's fee hook moved. The indexed fee-law terms were read from ONE hook, so
    /// they only describe the venue while this is still that hook.
    FeeHookSet {
        hook: Vec<u8>,
    },
}

pub fn decode_cvamm_log(log: &eth::v2::Log) -> Option<CvammEvent> {
    if log.topics.is_empty() {
        return None;
    }

    if let Some(event) = alm_events::StateSync::match_and_decode(log) {
        return Some(CvammEvent::StateSync {
            x_wad: be_bytes(&event.x_wad),
            kappa: be_bytes(&event.kappa),
            reserve_stable: be_bytes(&event.reserve_stable),
            reserve_volatile: be_bytes(&event.reserve_volatile),
            fee_stable_in_wad: be_bytes(&event.fee_stable_in_wad),
            fee_volatile_in_wad: be_bytes(&event.fee_volatile_in_wad),
        });
    }
    if let Some(event) = alm_events::Swap::match_and_decode(log) {
        return Some(CvammEvent::Swap { kappa: be_bytes(&event.liquidity) });
    }
    if let Some(event) = alm_events::SwapFeeCharged::match_and_decode(log) {
        return Some(CvammEvent::SwapFeeCharged {
            fee0_nonzero: event.fee0.to_string() != "0",
            fee1_nonzero: event.fee1.to_string() != "0",
            fee_wad: be_bytes(&event.fee_wad),
        });
    }
    if let Some(event) = alm_events::Sync::match_and_decode(log) {
        return Some(CvammEvent::Sync {
            reserve0: be_bytes(&event.reserve0),
            reserve1: be_bytes(&event.reserve1),
        });
    }
    if let Some(event) = alm_events::Recentered::match_and_decode(log) {
        return Some(CvammEvent::Recentered {
            anchor_after: be_bytes(&event.anchor_after),
            kappa_after: be_bytes(&event.kappa_after),
            x_after: be_bytes(&event.x_after),
        });
    }
    if let Some(event) = alm_events::CurveRetuned::match_and_decode(log) {
        return Some(CvammEvent::CurveRetuned {
            a_wad: be_bytes(&event.concentration_wad),
            span_up_wad: be_bytes(&event.span_up_wad),
            span_dn_wad: be_bytes(&event.span_dn_wad),
        });
    }
    if let Some(event) = alm_events::IdleDeployed::match_and_decode(log) {
        return Some(CvammEvent::IdleDeployed { kappa_after: be_bytes(&event.kappa_after) });
    }
    if let Some(event) = alm_events::FeesCompounded::match_and_decode(log) {
        return Some(CvammEvent::FeesCompounded { kappa_after: be_bytes(&event.new_kappa) });
    }
    if alm_events::ReservesRetracted::match_and_decode(log).is_some() {
        return Some(CvammEvent::ReservesRetracted);
    }
    if let Some(event) = alm_events::PauseSet::match_and_decode(log) {
        return Some(CvammEvent::PauseSet { paused: event.paused });
    }
    if let Some(event) = alm_events::FeeHookSet::match_and_decode(log) {
        return Some(CvammEvent::FeeHookSet { hook: event.fee_hook.to_vec() });
    }

    None
}

pub fn event_attributes(event: &CvammEvent, fee_hook: &Address) -> Vec<tycho::Attribute> {
    match event {
        CvammEvent::StateSync {
            x_wad,
            kappa,
            reserve_stable,
            reserve_volatile,
            fee_stable_in_wad,
            fee_volatile_in_wad,
        } => vec![
            attribute(attrs::X_WAD, x_wad.clone()),
            attribute(attrs::KAPPA, kappa.clone()),
            attribute(attrs::RESERVE_STABLE, reserve_stable.clone()),
            attribute(attrs::RESERVE_VOLATILE, reserve_volatile.clone()),
            attribute(attrs::FEE_STABLE_IN_WAD, fee_stable_in_wad.clone()),
            attribute(attrs::FEE_VOLATILE_IN_WAD, fee_volatile_in_wad.clone()),
        ],
        CvammEvent::Swap { kappa } => vec![attribute(attrs::KAPPA, kappa.clone())],
        CvammEvent::SwapFeeCharged { fee0_nonzero, fee1_nonzero, fee_wad } => {
            let mut attributes = Vec::with_capacity(2);
            // Fee on the stable leg means the swap was volatile-in, and vice versa.
            if *fee0_nonzero {
                attributes.push(attribute(attrs::FEE_VOLATILE_IN_WAD, fee_wad.clone()));
            }
            if *fee1_nonzero {
                attributes.push(attribute(attrs::FEE_STABLE_IN_WAD, fee_wad.clone()));
            }
            attributes
        }
        // Sync carries totals including idle — balances only, no state attributes.
        CvammEvent::Sync { .. } => vec![],
        CvammEvent::Recentered { anchor_after, kappa_after, x_after } => vec![
            attribute(attrs::ANCHOR_SQRT_CURVE_X96, anchor_after.clone()),
            attribute(attrs::KAPPA, kappa_after.clone()),
            attribute(attrs::X_WAD, x_after.clone()),
        ],
        CvammEvent::CurveRetuned { a_wad, span_up_wad, span_dn_wad } => vec![
            attribute(attrs::A_WAD, a_wad.clone()),
            attribute(attrs::SPAN_UP_WAD, span_up_wad.clone()),
            attribute(attrs::SPAN_DN_WAD, span_dn_wad.clone()),
        ],
        CvammEvent::IdleDeployed { kappa_after } | CvammEvent::FeesCompounded { kappa_after } => {
            vec![attribute(attrs::KAPPA, kappa_after.clone())]
        }
        CvammEvent::ReservesRetracted => {
            vec![attribute(attrs::RETRACTED, vec![1u8]), attribute(attrs::KAPPA, vec![0u8])]
        }
        CvammEvent::PauseSet { paused } => {
            vec![attribute(attrs::PAUSED, vec![u8::from(*paused)])]
        }
        CvammEvent::FeeHookSet { hook } => {
            vec![attribute(attrs::FEE_LAW_TRACKED, vec![u8::from(hook.as_slice() == fee_hook)])]
        }
    }
}

/// Unsigned big-endian bytes for an ABI-decoded unsigned integer.
fn be_bytes(value: &substreams::scalar::BigInt) -> Vec<u8> {
    let (sign, bytes) = value.to_bytes_be();
    debug_assert!(sign != num_bigint::Sign::Minus, "unsigned ABI value decoded negative");
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOOK: Address = [0x14u8; 20];

    /// The fee-law terms are read from ONE hook, so the flag must follow the ALM's hook
    /// exactly — including back to 1 when a hook swap is reverted.
    #[test]
    fn fee_hook_set_tracks_the_configured_hook() {
        let attrs_for = |hook: Vec<u8>| {
            event_attributes(&CvammEvent::FeeHookSet { hook }, &HOOK)
                .into_iter()
                .map(|a| (a.name, a.value))
                .collect::<Vec<_>>()
        };
        assert_eq!(attrs_for(HOOK.to_vec()), vec![(attrs::FEE_LAW_TRACKED.to_owned(), vec![1u8])]);
        assert_eq!(
            attrs_for(vec![0x99u8; 20]),
            vec![(attrs::FEE_LAW_TRACKED.to_owned(), vec![0u8])]
        );
        // The zero address selects the ALM's flat fallback fee, which the law does not model.
        assert_eq!(attrs_for(vec![0u8; 20]), vec![(attrs::FEE_LAW_TRACKED.to_owned(), vec![0u8])]);
    }
}
