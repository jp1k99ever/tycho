use std::{collections::HashMap, str::FromStr};

use alloy::{primitives::Address, sol_types::SolValue};
use tycho_common::{models::Chain, Bytes};

use crate::encoding::{
    errors::EncodingError,
    evm::utils::bytes_to_address,
    models::{EncodingContext, Swap},
    swap_encoder::SwapEncoder,
};

/// Encodes a swap against the Everlong CVAMM: the CvammALM is the pool, the swap
/// entrypoint and the approval target at once. Packed layout:
/// `alm (20) | token_in (20) | token_out (20) | stable_in (1)`.
///
/// `stable_in` is derived from the component's pinned token order — token0 is always
/// the 18-decimal stable (the venue pins the legs; it does not sort by address).
#[derive(Clone)]
pub struct EverlongCvammSwapEncoder {
    executor_address: Bytes,
}

impl SwapEncoder for EverlongCvammSwapEncoder {
    fn new(
        executor_address: Bytes,
        _chain: Chain,
        _config: Option<HashMap<String, String>>,
    ) -> Result<Self, EncodingError> {
        Ok(Self { executor_address })
    }

    fn encode_swap(
        &self,
        swap: &Swap,
        _encoding_context: &EncodingContext,
    ) -> Result<Vec<u8>, EncodingError> {
        let alm = Address::from_str(&swap.component().id).map_err(|_| {
            EncodingError::FatalError("Invalid Everlong CVAMM component id".to_owned())
        })?;
        let token_in = bytes_to_address(&swap.token_in().address)?;
        let token_out = bytes_to_address(&swap.token_out().address)?;
        let stable = swap
            .component()
            .tokens
            .first()
            .ok_or_else(|| {
                EncodingError::FatalError("Everlong CVAMM component carries no tokens".to_owned())
            })?;
        let stable_in = swap.token_in().address == *stable;

        Ok((alm, token_in, token_out, stable_in).abi_encode_packed())
    }

    fn executor_address(&self) -> &Bytes {
        &self.executor_address
    }

    fn clone_box(&self) -> Box<dyn SwapEncoder> {
        Box::new(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use alloy::hex::encode;
    use num_bigint::BigUint;
    use tycho_common::models::protocol::ProtocolComponent;

    use super::*;
    use crate::encoding::models::default_token;

    const ALM: &str = "0xf5124f5605ce1e91a7429b837b7dac8f9e5378dd";
    const NECT: &str = "0x1ce0a25d13ce4d52071ae7e02cf1f6606f4c79d3";
    const WBTC: &str = "0x0555e30da8f98308edb960aa94c0db47230d2b9c";

    fn component() -> ProtocolComponent {
        ProtocolComponent {
            id: ALM.to_owned(),
            protocol_system: "everlong_cvamm".to_owned(),
            tokens: vec![Bytes::from(NECT), Bytes::from(WBTC)],
            ..Default::default()
        }
    }

    fn context(token_in: Bytes, token_out: Bytes) -> EncodingContext {
        EncodingContext {
            router_address: Some(Bytes::zero(20)),
            group_token_in: token_in,
            group_token_out: token_out,
        }
    }

    #[test]
    fn stable_in_sets_the_direction_flag() {
        let swap = Swap::new(
            component(),
            default_token(Bytes::from(NECT)),
            default_token(Bytes::from(WBTC)),
            BigUint::ZERO,
        );
        let encoder =
            EverlongCvammSwapEncoder::new(Bytes::zero(20), Chain::Ethereum, None).unwrap();

        assert_eq!(
            encode(
                encoder
                    .encode_swap(&swap, &context(Bytes::from(NECT), Bytes::from(WBTC)))
                    .unwrap()
            ),
            concat!(
                "f5124f5605ce1e91a7429b837b7dac8f9e5378dd",
                "1ce0a25d13ce4d52071ae7e02cf1f6606f4c79d3",
                "0555e30da8f98308edb960aa94c0db47230d2b9c",
                "01",
            )
        );
    }

    #[test]
    fn volatile_in_clears_the_direction_flag() {
        let swap = Swap::new(
            component(),
            default_token(Bytes::from(WBTC)),
            default_token(Bytes::from(NECT)),
            BigUint::ZERO,
        );
        let encoder =
            EverlongCvammSwapEncoder::new(Bytes::zero(20), Chain::Ethereum, None).unwrap();

        let encoded = encode(
            encoder
                .encode_swap(&swap, &context(Bytes::from(WBTC), Bytes::from(NECT)))
                .unwrap(),
        );
        assert!(encoded.ends_with("00"));
        assert_eq!(encoded.len(), 61 * 2);
    }
}
