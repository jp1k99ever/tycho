use tycho_substreams::prelude as tycho;

use crate::collvault::Address;

pub const PROTOCOL_TYPE_NAME: &str = "everlong_collvault";

pub fn component_id(swapper: Address) -> String {
    format!("0x{}", hex::encode(swapper))
}

/// The venue address is the permissionless `CollateralRebalancerSwapper`; the
/// rebalancer, vault and ALM behind it are internal plumbing that never reaches the
/// caller. Tokens: [0] the 18-dec stable (NECT), [1] the volatile leg (WBTC).
pub fn protocol_component(
    swapper: Address,
    stable: Address,
    volatile: Address,
) -> tycho::ProtocolComponent {
    tycho::ProtocolComponent::new(&component_id(swapper))
        .with_tokens(&[stable, volatile])
        .as_swap_type(PROTOCOL_TYPE_NAME, tycho::ImplementationType::Custom)
}
