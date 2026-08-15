use std::{collections::HashMap, str::FromStr};

use alloy::{
    primitives::{Address, U256},
    sol_types::SolValue,
};
use tycho_common::{models::Chain, Bytes};

use crate::encoding::{
    errors::EncodingError,
    evm::utils::bytes_to_address,
    models::{EncodingContext, Swap},
    swap_encoder::SwapEncoder,
};

/// Encodes a swap against the Everlong CollateralRebalancer settlement venue
/// (CollateralRebalancerSwapper). Packed layout:
/// `swapper (20) | token_in (20) | token_out (20) | is_deleverage (1) |
///  hint_a (32) | hint_b (32) | coll_rebalancer_math (20) | leverage_ratio_wad (32)`.
///
/// The direction is derived from the component's token order — token0 is the CDP
/// stable, so stable-in means DELEVERAGE and volatile-in means LEVERAGE.
///
/// The hints let the executor skip or shorten its on-chain sizing: for leverage,
/// `hint_a` is the quote-time CollVault share count (bisection seed); for deleverage,
/// `hint_a` is the quote-time gross debt and `hint_b` the quoted net stable spend
/// (proportional scaling reference). They arrive through `Swap::user_data` as two
/// concatenated 32-byte words; absent or malformed user data encodes as zeros, which
/// falls back to full on-chain derivation.
///
/// The last two words are the venue's CR-math library and the leverage ratio it validates
/// against. They come from the component's static attributes, not from the caller: the
/// library has no getter on the rebalancer, so a partial deleverage fill can only
/// re-derive its gross against a configured address. Both are required — a deleverage
/// sized off a wrong or absent one is a fill the swapper reverts.
#[derive(Clone)]
pub struct EverlongRebalancerSwapEncoder {
    executor_address: Bytes,
}

impl SwapEncoder for EverlongRebalancerSwapEncoder {
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
        let swapper = Address::from_str(&swap.component().id).map_err(|_| {
            EncodingError::FatalError("Invalid Everlong Rebalancer component id".to_owned())
        })?;
        let token_in = bytes_to_address(&swap.token_in().address)?;
        let token_out = bytes_to_address(&swap.token_out().address)?;
        let stable = swap
            .component()
            .tokens
            .first()
            .ok_or_else(|| {
                EncodingError::FatalError(
                    "Everlong Rebalancer component carries no tokens".to_owned(),
                )
            })?;
        let is_deleverage = swap.token_in().address == *stable;

        let attributes = &swap.component().static_attributes;
        let math = attributes
            .get("coll_rebalancer_math")
            .ok_or_else(|| {
                EncodingError::FatalError(
                    "Everlong Rebalancer component carries no coll_rebalancer_math".to_owned(),
                )
            })
            .and_then(bytes_to_address)?;
        let leverage_ratio_wad = attributes
            .get("leverage_ratio_wad")
            .map(|value| U256::from_be_slice(value.as_ref()))
            .ok_or_else(|| {
                EncodingError::FatalError(
                    "Everlong Rebalancer component carries no leverage_ratio_wad".to_owned(),
                )
            })?;

        let (hint_a, hint_b) = decode_hints(swap.user_data());
        Ok((swapper, token_in, token_out, is_deleverage, hint_a, hint_b, math, leverage_ratio_wad)
            .abi_encode_packed())
    }

    fn executor_address(&self) -> &Bytes {
        &self.executor_address
    }

    fn clone_box(&self) -> Box<dyn SwapEncoder> {
        Box::new(self.clone())
    }
}

fn decode_hints(user_data: &Option<Bytes>) -> (U256, U256) {
    match user_data {
        Some(data) if data.len() == 64 => {
            (U256::from_be_slice(&data.as_ref()[..32]), U256::from_be_slice(&data.as_ref()[32..]))
        }
        _ => (U256::ZERO, U256::ZERO),
    }
}

#[cfg(test)]
mod tests {
    use alloy::hex::encode;
    use num_bigint::BigUint;
    use tycho_common::models::protocol::ProtocolComponent;

    use super::*;
    use crate::encoding::models::default_token;

    const SWAPPER: &str = "0x27775ec38e2b394738b73c0d25f63e20063df054";
    const NECT: &str = "0x1ce0a25d13ce4d52071ae7e02cf1f6606f4c79d3";
    const WBTC: &str = "0x0555e30da8f98308edb960aa94c0db47230d2b9c";
    const MATH: &str = "0x72489064be7b96c56b17b1627f089217b38ad292";
    /// 444444444444444444, as the substreams emits it (minimal big-endian bytes).
    const LEVERAGE_RATIO: &str = "0x062afbde1181c71c";

    fn component() -> ProtocolComponent {
        ProtocolComponent {
            id: SWAPPER.to_owned(),
            protocol_system: "everlong_rebalancer".to_owned(),
            tokens: vec![Bytes::from(NECT), Bytes::from(WBTC)],
            static_attributes: HashMap::from([
                ("coll_rebalancer_math".to_owned(), Bytes::from(MATH)),
                ("leverage_ratio_wad".to_owned(), Bytes::from(LEVERAGE_RATIO)),
            ]),
            ..Default::default()
        }
    }

    const MATH_WORDS: &str = concat!(
        "72489064be7b96c56b17b1627f089217b38ad292",
        "000000000000000000000000000000000000000000000000062afbde1181c71c",
    );

    fn context(token_in: Bytes, token_out: Bytes) -> EncodingContext {
        EncodingContext {
            router_address: Some(Bytes::zero(20)),
            group_token_in: token_in,
            group_token_out: token_out,
        }
    }

    #[test]
    fn deleverage_encodes_direction_and_hints() {
        let mut user_data = vec![0u8; 64];
        user_data[31] = 7; // gross hint = 7
        user_data[63] = 5; // net hint = 5
        let swap = Swap::new(
            component(),
            default_token(Bytes::from(NECT)),
            default_token(Bytes::from(WBTC)),
            BigUint::ZERO,
        )
        .with_user_data(Bytes::from(user_data));
        let encoder =
            EverlongRebalancerSwapEncoder::new(Bytes::zero(20), Chain::Ethereum, None).unwrap();

        let encoded = encode(
            encoder
                .encode_swap(&swap, &context(Bytes::from(NECT), Bytes::from(WBTC)))
                .unwrap(),
        );
        assert_eq!(encoded.len(), 177 * 2);
        assert!(encoded.starts_with(concat!(
            "27775ec38e2b394738b73c0d25f63e20063df054",
            "1ce0a25d13ce4d52071ae7e02cf1f6606f4c79d3",
            "0555e30da8f98308edb960aa94c0db47230d2b9c",
            "01",
            "0000000000000000000000000000000000000000000000000000000000000007",
            "0000000000000000000000000000000000000000000000000000000000000005",
        )));
        // The math library and the ratio it validates against close the payload, so a
        // partial fill can re-derive its gross instead of rescaling the hints above.
        assert!(encoded.ends_with(MATH_WORDS));
    }

    /// The math library has no getter on the rebalancer, so a component without it cannot
    /// produce a fill the venue will honour — encoding must fail rather than emit a zero
    /// address the executor would then call.
    #[test]
    fn a_component_without_the_math_words_is_refused() {
        for missing in ["coll_rebalancer_math", "leverage_ratio_wad"] {
            let mut component = component();
            component
                .static_attributes
                .remove(missing);
            let swap = Swap::new(
                component,
                default_token(Bytes::from(NECT)),
                default_token(Bytes::from(WBTC)),
                BigUint::ZERO,
            );
            let encoder =
                EverlongRebalancerSwapEncoder::new(Bytes::zero(20), Chain::Ethereum, None).unwrap();
            assert!(encoder
                .encode_swap(&swap, &context(Bytes::from(NECT), Bytes::from(WBTC)))
                .is_err());
        }
    }

    #[test]
    fn leverage_without_user_data_encodes_zero_hints() {
        let swap = Swap::new(
            component(),
            default_token(Bytes::from(WBTC)),
            default_token(Bytes::from(NECT)),
            BigUint::ZERO,
        );
        let encoder =
            EverlongRebalancerSwapEncoder::new(Bytes::zero(20), Chain::Ethereum, None).unwrap();

        let encoded = encode(
            encoder
                .encode_swap(&swap, &context(Bytes::from(WBTC), Bytes::from(NECT)))
                .unwrap(),
        );
        assert_eq!(encoded.len(), 177 * 2);
        let direction_byte = &encoded[120..122];
        assert_eq!(direction_byte, "00");
        // Both hints zero: the executor brackets the share count from scratch.
        assert_eq!(&encoded[122..250], &"0".repeat(128));
        assert!(encoded.ends_with(MATH_WORDS));
    }
}
