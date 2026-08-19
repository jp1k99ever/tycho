use tycho_substreams::prelude as tycho;

/// State attributes consumed by the native `everlong_cvamm` simulation.
///
/// All numeric values are unsigned big-endian bytes. `x_wad` is the authoritative
/// inventory coordinate — the simulation must never re-derive it from a sqrt price
/// (the on-chain sqrt is a lossy floored root).
pub mod attrs {
    /// Inventory coordinate on the curve (WAD). Exact via `StateSync`/`Recentered`.
    pub const X_WAD: &str = "x_wad";
    /// Scale factor tying normalized curve units to token amounts.
    pub const KAPPA: &str = "kappa";
    /// Anchor sqrt price in the CURVE frame (Q96). Pool frame = 2^192 / this.
    pub const ANCHOR_SQRT_CURVE_X96: &str = "anchor_sqrt_curve_x96";
    /// Curve concentration A (WAD) — `getConfig().concentrationWad`.
    pub const A_WAD: &str = "a_wad";
    /// Curve span above the anchor (WAD).
    pub const SPAN_UP_WAD: &str = "span_up_wad";
    /// Curve span below the anchor (WAD).
    pub const SPAN_DN_WAD: &str = "span_dn_wad";
    /// Accounted tradeable stable reserve (token units) — the solvency clamp.
    pub const RESERVE_STABLE: &str = "reserve_stable";
    /// Accounted tradeable volatile reserve (token units) — the solvency clamp.
    pub const RESERVE_VOLATILE: &str = "reserve_volatile";
    /// Directional fee for stable-in swaps (WAD). Exact only via `StateSync`;
    /// interim approximation from `SwapFeeCharged` (stale between swaps).
    pub const FEE_STABLE_IN_WAD: &str = "fee_stable_in_wad";
    /// Directional fee for volatile-in swaps (WAD). Same caveat as above.
    pub const FEE_VOLATILE_IN_WAD: &str = "fee_volatile_in_wad";
    /// 1 byte: 1 = paused (stop quoting).
    pub const PAUSED: &str = "paused";
    /// 1 byte: 1 = reserves retracted (kappa is 0, stop quoting permanently
    /// until the book is redeployed).
    pub const RETRACTED: &str = "retracted";

    // --- Fee-law terms (creation only, from manifest params) ---
    //
    // `CvammFeeLib`'s law, whose terms all live on the ALM's immutable `ClammFeeHook`.
    // They let the simulation RE-DERIVE the fee at the post-fill book instead of
    // carrying the pre-fill sample, which is what keeps a second hop through the same
    // ALM priced on its own leg. The law's one unobservable input, realized variance,
    // is never indexed: it reaches the fee only through a scalar a swap cannot move,
    // so the simulation solves it from the sampled fees instead.

    /// 1 byte: 1 = the terms below still describe the ALM's live fee hook. `FeeHookSet`
    /// clears it when the ALM moves to a hook these terms were not read from, so the
    /// simulation drops the re-derivation rather than pricing off the wrong law.
    pub const FEE_LAW_TRACKED: &str = "fee_law_tracked";
    /// `inventoryBalancedFeeWad` — the law's base cap.
    pub const MID_FEE_WAD: &str = "mid_fee_wad";
    /// `inventoryImbalancedFeeWad` — the law's base floor.
    pub const OUT_FEE_WAD: &str = "out_fee_wad";
    /// `inventoryFeeCurvatureWad`. Zero selects the law's scalar branch.
    pub const CURVATURE_WAD: &str = "curvature_wad";
    /// `lpFeeWad` — read only on the zero-curvature branch.
    pub const LP_FEE_WAD: &str = "lp_fee_wad";
    /// `dirSkewWad` — the restoring/widening multiplier.
    pub const DIR_SKEW_WAD: &str = "dir_skew_wad";
    /// `invSkewKappaWad` — inventory-displacement surcharge slope.
    pub const INV_SKEW_KAPPA_WAD: &str = "inv_skew_kappa_wad";
    /// `invSkewBandWad` — deadband on that surcharge.
    pub const INV_SKEW_BAND_WAD: &str = "inv_skew_band_wad";
    /// `volSigmaRefWad`. Zero disables the vol term outright.
    pub const VOL_SIGMA_REF_WAD: &str = "vol_sigma_ref_wad";
    /// `volBetaWad` — dislocation boost on the vol multiplier.
    pub const VOL_BETA_WAD: &str = "vol_beta_wad";
    /// `volMinWad` / `volMaxWad` — the vol multiplier's clamp.
    pub const VOL_MIN_WAD: &str = "vol_min_wad";
    pub const VOL_MAX_WAD: &str = "vol_max_wad";
    /// `hotFeeFloorWad(true|false, uint128 max)` — the FFAD floor at a SATURATED push
    /// rate, which bounds it at every live rate. Applied last on-chain, and discarded
    /// there if it exceeds WAD.
    pub const FLOOR_STABLE_IN_WAD: &str = "floor_stable_in_wad";
    pub const FLOOR_VOLATILE_IN_WAD: &str = "floor_volatile_in_wad";
}

pub fn attribute(name: &'static str, value: Vec<u8>) -> tycho::Attribute {
    tycho::Attribute { name: name.to_owned(), value, change: tycho::ChangeType::Update.into() }
}

pub fn creation_attribute(name: &'static str, value: Vec<u8>) -> tycho::Attribute {
    tycho::Attribute { name: name.to_owned(), value, change: tycho::ChangeType::Creation.into() }
}
