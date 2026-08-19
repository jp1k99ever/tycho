use tycho_substreams::prelude as tycho;

use crate::cvamm::Address;

pub const PROTOCOL_TYPE_NAME: &str = "everlong_cvamm_pool";

pub fn component_id(alm: Address) -> String {
    format!("0x{}", hex::encode(alm))
}

/// The CvammALM is the pool: single contract, token0 pinned to the 18-dec stable,
/// token1 the volatile leg (order enforced on-chain, not address-sorted).
pub fn protocol_component(
    alm: Address,
    stable: Address,
    volatile: Address,
) -> tycho::ProtocolComponent {
    tycho::ProtocolComponent::new(&component_id(alm))
        .with_tokens(&[stable, volatile])
        .as_swap_type(PROTOCOL_TYPE_NAME, tycho::ImplementationType::Custom)
}
