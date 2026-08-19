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
    // The ALM's ClammFeeHook. Pinned here rather than read, because the fee-law terms
    // below are read from THIS hook: if `FeeHookSet` moves the ALM to another one, the
    // terms stop describing the venue and the simulation must drop them.
    const DEFAULT_FEE_HOOK: &str = "0x14Fd229fB23565986abE05254E2976a259279e78";
    const DEFAULT_A_WAD: &str = "34000000000000000000";
    const DEFAULT_SPAN_UP_WAD: &str = "4000000000000000000";
    const DEFAULT_SPAN_DN_WAD: &str = "6000000000000000000";
    const DEFAULT_INITIAL_FEE_WAD: &str = "5000000000000000";
    // ClammFeeHook immutables for the deployed hook 0x14Fd229fB23565986abE05254E2976a259279e78.
    // These are the fee LAW's terms — the native simulation re-derives the post-fill fee from
    // them instead of carrying the pre-fill sample forward, so a second hop through the same
    // ALM prices on its own leg. Every one is either a hook immutable or a hook constant, so
    // params are the right channel: a hook swap is a new deployment, i.e. a param edit.
    const DEFAULT_MID_FEE_WAD: &str = "30000000000000000"; // inventoryBalancedFeeWad, 0.03e18
    const DEFAULT_OUT_FEE_WAD: &str = "5000000000000000"; // inventoryImbalancedFeeWad, 0.005e18
    const DEFAULT_CURVATURE_WAD: &str = "50000000000000000"; // inventoryFeeCurvatureWad, 0.05e18
                                                             // lpFeeWad selects the law's scalar branch and is only READ when curvature is zero. It
                                                             // ramps on-chain, so a nonzero value here is only meaningful for a zero-curvature hook.
    const DEFAULT_LP_FEE_WAD: &str = "0";
    const DEFAULT_DIR_SKEW_WAD: &str = "200000000000000000"; // dirSkewWad, 0.2e18
    const DEFAULT_INV_SKEW_KAPPA_WAD: &str = "0"; // pinned zero, asserted at deploy
    const DEFAULT_INV_SKEW_BAND_WAD: &str = "0";
    const DEFAULT_VOL_SIGMA_REF_WAD: &str = "400000000000000"; // volSigmaRefWad, 4e14
    const DEFAULT_VOL_BETA_WAD: &str = "4000000000000000000"; // volBetaWad, 4e18
    const DEFAULT_VOL_MIN_WAD: &str = "500000000000000000"; // volMinWad, 5e17
    const DEFAULT_VOL_MAX_WAD: &str = "1600000000000000000"; // volMaxWad, 1.6e18
                                                             // hotFeeFloorWad(dir, type(uint128).max): the FFAD floor at a saturated push rate, which
                                                             // is its ceiling outright (the hook ramps `level * smoothstep(t)` and clamps at `level`).
                                                             // Both are hook CONSTANTS — FFAD_STABLE_IN_FLOOR_WAD / FFAD_VOLATILE_IN_FLOOR_WAD.
    const DEFAULT_FLOOR_STABLE_IN_WAD: &str = "15000000000000000"; // 0.015e18
    const DEFAULT_FLOOR_VOLATILE_IN_WAD: &str = "25000000000000000"; // 0.025e18

    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct AlmConfig {
        pub alm: cvamm::Address,
        pub stable: cvamm::Address,
        pub volatile: cvamm::Address,
        pub bootstrap_block: Option<u64>,
        /// The `ClammFeeHook` the fee-law terms below were read from.
        pub fee_hook: cvamm::Address,
        /// Big-endian bytes of the initial curve/fee constants (decimal params).
        pub a_wad: Vec<u8>,
        pub span_up_wad: Vec<u8>,
        pub span_dn_wad: Vec<u8>,
        pub initial_fee_wad: Vec<u8>,
        /// The fee law's terms, all hook immutables/constants — see [`FeeLawConfig`].
        pub fee_law: FeeLawConfig,
    }

    /// Terms of `CvammFeeLib`'s fee law, sourced from the ALM's `ClammFeeHook`.
    ///
    /// The hook is immutable on the ALM and every one of these is an immutable or a
    /// constant on it, so they belong in the params rather than in an event: a change to
    /// any of them is a new hook, which is a new deployment. `reservationPriceWad` is
    /// deliberately NOT here — it moves with the anchor, and the simulation derives it
    /// from the indexed `anchor_sqrt_curve_x96` instead.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct FeeLawConfig {
        pub mid_fee_wad: Vec<u8>,
        pub out_fee_wad: Vec<u8>,
        pub curvature_wad: Vec<u8>,
        pub lp_fee_wad: Vec<u8>,
        pub dir_skew_wad: Vec<u8>,
        pub inv_skew_kappa_wad: Vec<u8>,
        pub inv_skew_band_wad: Vec<u8>,
        pub vol_sigma_ref_wad: Vec<u8>,
        pub vol_beta_wad: Vec<u8>,
        pub vol_min_wad: Vec<u8>,
        pub vol_max_wad: Vec<u8>,
        pub floor_stable_in_wad: Vec<u8>,
        pub floor_volatile_in_wad: Vec<u8>,
    }

    impl Default for FeeLawConfig {
        fn default() -> Self {
            let d = |value: &str| parse_uint(value).expect("valid default fee-law constant");
            Self {
                mid_fee_wad: d(DEFAULT_MID_FEE_WAD),
                out_fee_wad: d(DEFAULT_OUT_FEE_WAD),
                curvature_wad: d(DEFAULT_CURVATURE_WAD),
                lp_fee_wad: d(DEFAULT_LP_FEE_WAD),
                dir_skew_wad: d(DEFAULT_DIR_SKEW_WAD),
                inv_skew_kappa_wad: d(DEFAULT_INV_SKEW_KAPPA_WAD),
                inv_skew_band_wad: d(DEFAULT_INV_SKEW_BAND_WAD),
                vol_sigma_ref_wad: d(DEFAULT_VOL_SIGMA_REF_WAD),
                vol_beta_wad: d(DEFAULT_VOL_BETA_WAD),
                vol_min_wad: d(DEFAULT_VOL_MIN_WAD),
                vol_max_wad: d(DEFAULT_VOL_MAX_WAD),
                floor_stable_in_wad: d(DEFAULT_FLOOR_STABLE_IN_WAD),
                floor_volatile_in_wad: d(DEFAULT_FLOOR_VOLATILE_IN_WAD),
            }
        }
    }

    impl FeeLawConfig {
        /// Returns false for a key this config does not own, so the caller can keep
        /// matching. Errors only on a key it owns with an unparseable value.
        fn set(&mut self, key: &str, value: &str) -> Result<bool> {
            let field = match key {
                "mid_fee_wad" => &mut self.mid_fee_wad,
                "out_fee_wad" => &mut self.out_fee_wad,
                "curvature_wad" => &mut self.curvature_wad,
                "lp_fee_wad" => &mut self.lp_fee_wad,
                "dir_skew_wad" => &mut self.dir_skew_wad,
                "inv_skew_kappa_wad" => &mut self.inv_skew_kappa_wad,
                "inv_skew_band_wad" => &mut self.inv_skew_band_wad,
                "vol_sigma_ref_wad" => &mut self.vol_sigma_ref_wad,
                "vol_beta_wad" => &mut self.vol_beta_wad,
                "vol_min_wad" => &mut self.vol_min_wad,
                "vol_max_wad" => &mut self.vol_max_wad,
                "floor_stable_in_wad" => &mut self.floor_stable_in_wad,
                "floor_volatile_in_wad" => &mut self.floor_volatile_in_wad,
                _ => return Ok(false),
            };
            *field = parse_uint(value)?;
            Ok(true)
        }
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
                    fee_hook: parse_address(DEFAULT_FEE_HOOK).expect("valid default fee hook"),
                    a_wad: parse_uint(DEFAULT_A_WAD).expect("valid default a_wad"),
                    span_up_wad: parse_uint(DEFAULT_SPAN_UP_WAD).expect("valid default span_up"),
                    span_dn_wad: parse_uint(DEFAULT_SPAN_DN_WAD).expect("valid default span_dn"),
                    initial_fee_wad: parse_uint(DEFAULT_INITIAL_FEE_WAD)
                        .expect("valid default fee"),
                    fee_law: FeeLawConfig::default(),
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
                    "fee_hook" => single.fee_hook = parse_address(value)?,
                    "a_wad" => single.a_wad = parse_uint(value)?,
                    "span_up_wad" => single.span_up_wad = parse_uint(value)?,
                    "span_dn_wad" => single.span_dn_wad = parse_uint(value)?,
                    "initial_fee_wad" => single.initial_fee_wad = parse_uint(value)?,
                    "alms" => multi = Some(parse_alms(value)?),
                    _ if single.fee_law.set(key, value)? => {}
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
                        fee_hook: single.fee_hook,
                        fee_law: single.fee_law.clone(),
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

    /// The fee-law terms are the configurable-constants channel for the hook, so they
    /// must both default to the deployed values and be overridable per deployment.
    #[test]
    fn fee_law_params_default_and_override() {
        let uint = |value: &str| {
            num_bigint::BigUint::parse_bytes(value.as_bytes(), 10)
                .expect("valid decimal")
                .to_bytes_be()
        };
        let config = Config::parse("").expect("empty params are valid");
        let law = &config.pools[0].fee_law;
        assert_eq!(law.mid_fee_wad, uint("30000000000000000"));
        assert_eq!(law.out_fee_wad, uint("5000000000000000"));
        assert_eq!(law.curvature_wad, uint("50000000000000000"));
        assert_eq!(law.dir_skew_wad, uint("200000000000000000"));
        assert_eq!(law.floor_stable_in_wad, uint("15000000000000000"));
        assert_eq!(law.floor_volatile_in_wad, uint("25000000000000000"));

        let swapped = Config::parse("mid_fee_wad=1&vol_max_wad=2&floor_volatile_in_wad=3")
            .expect("fee-law overrides are valid");
        let law = &swapped.pools[0].fee_law;
        assert_eq!(law.mid_fee_wad, vec![1u8]);
        assert_eq!(law.vol_max_wad, vec![2u8]);
        assert_eq!(law.floor_volatile_in_wad, vec![3u8]);
        // Untouched terms keep the deployed defaults.
        assert_eq!(law.out_fee_wad, uint("5000000000000000"));
    }

    /// A key the fee law does not own must still be rejected — the `set` fall-through
    /// is a fast path, not a catch-all.
    #[test]
    fn unknown_param_is_still_rejected() {
        assert!(Config::parse("mid_fee_waaad=1").is_err());
    }
}
