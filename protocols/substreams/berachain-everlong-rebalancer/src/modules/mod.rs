pub use map_protocol_changes::map_protocol_changes;
pub use map_protocol_components::map_protocol_components;
pub use store_protocol_components::store_protocol_components;

pub(crate) mod config {
    use anyhow::{anyhow, Result};

    use crate::rebalancer;

    // Live venue on Berachain (chain id 80094).
    const SWAPPER: &str = "0x27775EC38E2b394738B73C0D25f63e20063DF054";
    const REBALANCER: &str = "0xA6b848d899189d263a9398F1DF4534Af7B06d6b3";
    const ALM: &str = "0xF5124F5605ce1e91A7429B837b7daC8f9E5378dd";
    // CollRebalancerMath, the linked library the venue's own quotes go through. It has no
    // getter on the rebalancer, so a partial deleverage fill can only re-derive its gross
    // against a CONFIGURED address — which is why it travels to the executor as a component
    // static attribute rather than being read.
    const MATH: &str = "0x72489064be7b96c56b17b1627f089217b38ad292";
    const NECT: &str = "0x1cE0a25D13CE4d52071aE7e02Cf1F6606F4C79d3";
    const WBTC: &str = "0x0555E30da8f98308EdB960aa94C0Db47230d2B9c";

    // Deployed CollRebalancerMath constants ("champion-v3", 155% wall) and frozen
    // deployment words. All overridable via params; emitted as creation attributes
    // so the native simulation reads them from state, not from code.
    const DEFAULTS: &[(&str, &str)] = &[
        ("leverage_ratio_wad", "444444444444444444"),
        ("h_zero", "562500000000000000"),
        ("h_join", "1010000000000000000"),
        ("h_wall", "1882448291726770582"),
        ("width", "872448291726770582"),
        ("d_join", "509975124224178054"),
        ("d_wall", "1214482768855981020"),
        ("rescue_spread_ppm", "13000"),
        ("physical_cr_floor_wad", "1820000000000000000"),
        // Live CollVault assetDecimals() is 18 → offset 0 (verified in block-24710262 snapshot).
        ("cv_decimals_offset", "0"),
        ("min_net_debt", "0"),
        ("debt_gas_compensation", "0"),
        ("mcr_wad", "0"),
    ];
    const DEFAULT_BEZIER_PHI: &str =
        "995037190209989135,851783312849706840,738044106433170508,645161290322580645";
    const DEFAULT_BEZIER_INTEGRAL: &str =
        "0,248759297552497283,461705125764923993,646216152373216620,807506474953861782";

    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct VenueConfig {
        pub swapper: rebalancer::Address,
        pub rebalancer: rebalancer::Address,
        pub alm: rebalancer::Address,
        pub math: rebalancer::Address,
        pub stable: rebalancer::Address,
        pub volatile: rebalancer::Address,
        pub bootstrap_block: Option<u64>,
        /// The rebalancer implementation the curve constants below were read from. Unset
        /// (zero) means unpinned, and then ANY `Upgraded` clears `curve_tracked`.
        pub rebalancer_impl: rebalancer::Address,
        // Big-endian bytes of decimal params; the Bézier lists are 32-byte words
        // concatenated in order.
        pub leverage_ratio_wad: Vec<u8>,
        pub h_zero: Vec<u8>,
        pub h_join: Vec<u8>,
        pub h_wall: Vec<u8>,
        pub width: Vec<u8>,
        pub d_join: Vec<u8>,
        pub d_wall: Vec<u8>,
        pub rescue_spread_ppm: Vec<u8>,
        pub physical_cr_floor_wad: Vec<u8>,
        pub cv_decimals_offset: Vec<u8>,
        pub min_net_debt: Vec<u8>,
        pub debt_gas_compensation: Vec<u8>,
        pub mcr_wad: Vec<u8>,
        pub bezier_phi: Vec<u8>,
        pub bezier_integral: Vec<u8>,
    }

    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct Config {
        pub venue: VenueConfig,
    }

    impl Default for Config {
        fn default() -> Self {
            let default_for = |key: &str| {
                let (_, value) = DEFAULTS
                    .iter()
                    .find(|(name, _)| *name == key)
                    .expect("default key present");
                parse_uint(value).expect("valid default constant")
            };
            Self {
                venue: VenueConfig {
                    swapper: parse_address(SWAPPER).expect("valid default swapper"),
                    rebalancer: parse_address(REBALANCER).expect("valid default rebalancer"),
                    alm: parse_address(ALM).expect("valid default alm"),
                    math: parse_address(MATH).expect("valid default math"),
                    stable: parse_address(NECT).expect("valid default stable"),
                    volatile: parse_address(WBTC).expect("valid default volatile"),
                    bootstrap_block: None,
                    rebalancer_impl: [0u8; 20],
                    leverage_ratio_wad: default_for("leverage_ratio_wad"),
                    h_zero: default_for("h_zero"),
                    h_join: default_for("h_join"),
                    h_wall: default_for("h_wall"),
                    width: default_for("width"),
                    d_join: default_for("d_join"),
                    d_wall: default_for("d_wall"),
                    rescue_spread_ppm: default_for("rescue_spread_ppm"),
                    physical_cr_floor_wad: default_for("physical_cr_floor_wad"),
                    cv_decimals_offset: default_for("cv_decimals_offset"),
                    min_net_debt: default_for("min_net_debt"),
                    debt_gas_compensation: default_for("debt_gas_compensation"),
                    mcr_wad: default_for("mcr_wad"),
                    bezier_phi: parse_word_list(DEFAULT_BEZIER_PHI, 4)
                        .expect("valid default bezier phi"),
                    bezier_integral: parse_word_list(DEFAULT_BEZIER_INTEGRAL, 5)
                        .expect("valid default bezier integral"),
                },
            }
        }
    }

    impl Config {
        pub fn parse(params: &str) -> Result<Self> {
            let mut config = Self::default();
            let venue = &mut config.venue;
            for pair in params
                .split('&')
                .filter(|part| !part.is_empty())
            {
                let Some((key, value)) = pair.split_once('=') else {
                    return Err(anyhow!("invalid param pair `{pair}`"));
                };
                match key {
                    "swapper" => venue.swapper = parse_address(value)?,
                    "rebalancer" => venue.rebalancer = parse_address(value)?,
                    "alm" => venue.alm = parse_address(value)?,
                    "math" => venue.math = parse_address(value)?,
                    "stable" => venue.stable = parse_address(value)?,
                    "volatile" => venue.volatile = parse_address(value)?,
                    "bootstrap_block" => venue.bootstrap_block = Some(value.parse()?),
                    "rebalancer_impl" => venue.rebalancer_impl = parse_address(value)?,
                    "leverage_ratio_wad" => venue.leverage_ratio_wad = parse_uint(value)?,
                    "h_zero" => venue.h_zero = parse_uint(value)?,
                    "h_join" => venue.h_join = parse_uint(value)?,
                    "h_wall" => venue.h_wall = parse_uint(value)?,
                    "width" => venue.width = parse_uint(value)?,
                    "d_join" => venue.d_join = parse_uint(value)?,
                    "d_wall" => venue.d_wall = parse_uint(value)?,
                    "rescue_spread_ppm" => venue.rescue_spread_ppm = parse_uint(value)?,
                    "physical_cr_floor_wad" => venue.physical_cr_floor_wad = parse_uint(value)?,
                    "cv_decimals_offset" => venue.cv_decimals_offset = parse_uint(value)?,
                    "min_net_debt" => venue.min_net_debt = parse_uint(value)?,
                    "debt_gas_compensation" => venue.debt_gas_compensation = parse_uint(value)?,
                    "mcr_wad" => venue.mcr_wad = parse_uint(value)?,
                    "bezier_phi" => venue.bezier_phi = parse_word_list(value, 4)?,
                    "bezier_integral" => venue.bezier_integral = parse_word_list(value, 5)?,
                    _ => {
                        return Err(anyhow!("unknown Everlong Rebalancer Substreams param `{key}`"))
                    }
                }
            }
            Ok(config)
        }
    }

    impl VenueConfig {
        pub fn component_id(&self) -> String {
            rebalancer::component_id(self.swapper)
        }
    }

    fn parse_address(value: &str) -> Result<rebalancer::Address> {
        let trimmed = value
            .strip_prefix("0x")
            .unwrap_or(value);
        let decoded = hex::decode(trimmed)?;
        decoded
            .as_slice()
            .try_into()
            .map_err(|_| anyhow!("address `{value}` is not 20 bytes"))
    }

    fn parse_uint(value: &str) -> Result<Vec<u8>> {
        let parsed = num_bigint::BigUint::parse_bytes(value.as_bytes(), 10)
            .ok_or_else(|| anyhow!("`{value}` is not a decimal unsigned integer"))?;
        Ok(parsed.to_bytes_be())
    }

    /// Comma-separated decimals packed as consecutive 32-byte big-endian words.
    fn parse_word_list(value: &str, expected: usize) -> Result<Vec<u8>> {
        let entries = value
            .split(',')
            .map(str::trim)
            .filter(|entry| !entry.is_empty())
            .collect::<Vec<_>>();
        if entries.len() != expected {
            return Err(anyhow!("expected {expected} entries, got {} in `{value}`", entries.len()));
        }
        let mut packed = Vec::with_capacity(expected * 32);
        for entry in entries {
            let bytes = parse_uint(entry)?;
            if bytes.len() > 32 {
                return Err(anyhow!("`{entry}` does not fit a 32-byte word"));
            }
            packed.extend(std::iter::repeat_n(0u8, 32 - bytes.len()));
            packed.extend(bytes);
        }
        Ok(packed)
    }
}

#[path = "3_map_protocol_changes.rs"]
mod map_protocol_changes;
#[path = "1_map_protocol_components.rs"]
mod map_protocol_components;
#[path = "2_store_protocol_components.rs"]
mod store_protocol_components;

#[cfg(test)]
mod tests {
    use super::config::Config;

    #[test]
    fn default_config_targets_live_venue() {
        let config = Config::parse("").expect("empty params are valid");
        assert_eq!(config.venue.swapper[0], 0x27);
        assert_eq!(config.venue.bezier_phi.len(), 4 * 32);
        assert_eq!(config.venue.bezier_integral.len(), 5 * 32);
        // First integral knot is zero → a full zero word.
        assert_eq!(&config.venue.bezier_integral[..32], &[0u8; 32]);
    }

    #[test]
    fn word_list_length_is_validated() {
        assert!(Config::parse("bezier_phi=1,2,3").is_err());
    }
}
