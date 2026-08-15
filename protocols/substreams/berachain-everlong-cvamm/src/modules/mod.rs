pub use map_protocol_changes::map_protocol_changes;
pub use map_protocol_components::map_protocol_components;
pub use store_protocol_components::store_protocol_components;

pub(crate) mod config {
    use anyhow::{anyhow, Result};

    use crate::cvamm;

    // WBTC-KAI CVAMM 01 on Berachain (chain id 80094).
    const LIVE_ALM: &str = "0xF5124F5605ce1e91A7429B837b7daC8f9E5378dd";
    const NECT: &str = "0x1cE0a25D13CE4d52071aE7e02Cf1F6606F4C79d3";
    const WBTC: &str = "0x0555E30da8f98308EdB960aa94C0Db47230d2B9c";
    // Deployed initialize-time constants; overridable via params. These seed the
    // component attributes because base blocks cannot read initialize-time storage;
    // `CurveRetuned`/`StateSync` events correct them afterwards.
    const DEFAULT_A_WAD: &str = "34000000000000000000";
    const DEFAULT_SPAN_UP_WAD: &str = "4000000000000000000";
    const DEFAULT_SPAN_DN_WAD: &str = "6000000000000000000";
    const DEFAULT_INITIAL_FEE_WAD: &str = "5000000000000000";

    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct AlmConfig {
        pub alm: cvamm::Address,
        pub stable: cvamm::Address,
        pub volatile: cvamm::Address,
        pub bootstrap_block: Option<u64>,
        /// Big-endian bytes of the initial curve/fee constants (decimal params).
        pub a_wad: Vec<u8>,
        pub span_up_wad: Vec<u8>,
        pub span_dn_wad: Vec<u8>,
        pub initial_fee_wad: Vec<u8>,
    }

    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct Config {
        pub pools: Vec<AlmConfig>,
    }

    impl Default for Config {
        fn default() -> Self {
            Self {
                pools: vec![AlmConfig {
                    alm: parse_address(LIVE_ALM).expect("valid default ALM address"),
                    stable: parse_address(NECT).expect("valid default stable address"),
                    volatile: parse_address(WBTC).expect("valid default volatile address"),
                    bootstrap_block: None,
                    a_wad: parse_uint(DEFAULT_A_WAD).expect("valid default a_wad"),
                    span_up_wad: parse_uint(DEFAULT_SPAN_UP_WAD).expect("valid default span_up"),
                    span_dn_wad: parse_uint(DEFAULT_SPAN_DN_WAD).expect("valid default span_dn"),
                    initial_fee_wad: parse_uint(DEFAULT_INITIAL_FEE_WAD)
                        .expect("valid default fee"),
                }],
            }
        }
    }

    impl Config {
        /// `alm`, `stable`, `volatile`, `bootstrap_block`, `a_wad`, `span_up_wad`,
        /// `span_dn_wad`, `initial_fee_wad` configure the (currently single) pool;
        /// `alms=alm:stable:volatile[:bootstrap_block],...` configures several pools
        /// sharing the scalar constants.
        pub fn parse(params: &str) -> Result<Self> {
            let mut config = Self::default();
            let mut single = config.pools[0].clone();
            let mut multi: Option<Vec<AlmConfig>> = None;

            for pair in params
                .split('&')
                .filter(|part| !part.is_empty())
            {
                let Some((key, value)) = pair.split_once('=') else {
                    return Err(anyhow!("invalid param pair `{pair}`"));
                };
                match key {
                    "alm" => single.alm = parse_address(value)?,
                    "stable" => single.stable = parse_address(value)?,
                    "volatile" => single.volatile = parse_address(value)?,
                    "bootstrap_block" => single.bootstrap_block = Some(value.parse()?),
                    "a_wad" => single.a_wad = parse_uint(value)?,
                    "span_up_wad" => single.span_up_wad = parse_uint(value)?,
                    "span_dn_wad" => single.span_dn_wad = parse_uint(value)?,
                    "initial_fee_wad" => single.initial_fee_wad = parse_uint(value)?,
                    "alms" => multi = Some(parse_alms(value)?),
                    _ => return Err(anyhow!("unknown Everlong CVAMM Substreams param `{key}`")),
                }
            }

            config.pools = match multi {
                Some(pools) => pools
                    .into_iter()
                    .map(|pool| AlmConfig {
                        a_wad: single.a_wad.clone(),
                        span_up_wad: single.span_up_wad.clone(),
                        span_dn_wad: single.span_dn_wad.clone(),
                        initial_fee_wad: single.initial_fee_wad.clone(),
                        ..pool
                    })
                    .collect(),
                None => vec![single],
            };
            Ok(config)
        }
    }

    impl AlmConfig {
        pub fn component_id(&self) -> String {
            cvamm::component_id(self.alm)
        }
    }

    fn parse_alms(value: &str) -> Result<Vec<AlmConfig>> {
        let defaults = Config::default().pools.remove(0);
        let pools = value
            .split(',')
            .filter(|entry| !entry.is_empty())
            .map(|entry| parse_alm(entry, &defaults))
            .collect::<Result<Vec<_>>>()?;
        if pools.is_empty() {
            return Err(anyhow!("`alms` param must contain at least one pool"));
        }
        Ok(pools)
    }

    fn parse_alm(value: &str, defaults: &AlmConfig) -> Result<AlmConfig> {
        let mut parts = value.split(':');
        let alm = parts
            .next()
            .ok_or_else(|| anyhow!("missing ALM address in `{value}`"))
            .and_then(parse_address)?;
        let stable = parts
            .next()
            .ok_or_else(|| anyhow!("missing stable address in `{value}`"))
            .and_then(parse_address)?;
        let volatile = parts
            .next()
            .ok_or_else(|| anyhow!("missing volatile address in `{value}`"))
            .and_then(parse_address)?;
        let bootstrap_block = parts
            .next()
            .map(str::parse)
            .transpose()?;
        if parts.next().is_some() {
            return Err(anyhow!("invalid `alms` tuple `{value}`"));
        }
        Ok(AlmConfig { alm, stable, volatile, bootstrap_block, ..defaults.clone() })
    }

    fn parse_address(value: &str) -> Result<cvamm::Address> {
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
    fn default_config_targets_live_alm() {
        let config = Config::parse("").expect("empty params are valid");
        assert_eq!(config.pools.len(), 1);
        assert_eq!(config.pools[0].alm[0], 0xF5);
        let expected = num_bigint::BigUint::parse_bytes(b"34000000000000000000", 10)
            .expect("valid decimal")
            .to_bytes_be();
        assert_eq!(config.pools[0].a_wad, expected);
    }

    #[test]
    fn scalar_params_override_defaults() {
        let config = Config::parse(
            "alm=0x0000000000000000000000000000000000000001&\
             stable=0x0000000000000000000000000000000000000002&\
             volatile=0x0000000000000000000000000000000000000003&\
             bootstrap_block=42&a_wad=7",
        )
        .expect("valid config");
        assert_eq!(config.pools[0].bootstrap_block, Some(42));
        assert_eq!(config.pools[0].a_wad, vec![7u8]);
    }
}
