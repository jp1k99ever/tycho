use tycho_substreams::prelude as tycho;

use crate::rebalancer::Address;

pub const PROTOCOL_TYPE_NAME: &str = "everlong_rebalancer_pool";

pub fn component_id(swapper: Address) -> String {
    format!("0x{}", hex::encode(swapper))
}

/// The venue address is the permissionless `CollateralRebalancerSwapper`; the
/// rebalancer, vault and ALM behind it are internal plumbing that never reaches the
/// caller. Tokens: [0] the 18-dec stable (NECT), [1] the volatile leg (WBTC).
///
/// Two static attributes are what a partial deleverage fill needs at execution time and
/// cannot read on-chain. `coll_rebalancer_math` is the linked library the venue's own
/// quotes go through: it has no getter on the rebalancer, so it is configuration, and the
/// executor re-derives the gross against it rather than rescaling a stale hint.
/// `leverage_ratio_wad` is the curve word that library validates its answer against.
pub fn protocol_component(
    swapper: Address,
    stable: Address,
    volatile: Address,
    math: Address,
    leverage_ratio_wad: &[u8],
) -> tycho::ProtocolComponent {
    tycho::ProtocolComponent::new(&component_id(swapper))
        .with_tokens(&[stable, volatile])
        .with_attributes(&[
            ("coll_rebalancer_math", math.as_slice()),
            ("leverage_ratio_wad", leverage_ratio_wad),
        ])
        .as_swap_type(PROTOCOL_TYPE_NAME, tycho::ImplementationType::Custom)
}
