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
}

pub fn attribute(name: &'static str, value: Vec<u8>) -> tycho::Attribute {
    tycho::Attribute { name: name.to_owned(), value, change: tycho::ChangeType::Update.into() }
}

pub fn creation_attribute(name: &'static str, value: Vec<u8>) -> tycho::Attribute {
    tycho::Attribute { name: name.to_owned(), value, change: tycho::ChangeType::Creation.into() }
}
