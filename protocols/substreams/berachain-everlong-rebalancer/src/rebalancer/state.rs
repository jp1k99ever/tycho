use tycho_substreams::prelude as tycho;

/// State attributes consumed by the native `everlong_rebalancer` simulation.
///
/// All numeric values are unsigned big-endian bytes. The exchange words update from
/// `LeverageIncreased`/`LeverageDecreased`; the aggregate words (ALM reserves, vault
/// totals, reference reserves, interest) are only exact via the rebalancer's
/// `StateSync` snapshot event. A zero `price_wad` means the reservation price has
/// not been observed yet — the simulation must refuse to quote until it is.
pub mod attrs {
    // Exchange state — `exchangeState()` words.
    pub const COLLATERAL: &str = "collateral";
    pub const DEBT: &str = "debt";
    pub const PRICE_WAD: &str = "price_wad";
    pub const SPREAD_PPM: &str = "spread_ppm";

    // ALM aggregates backing the ERC-4626 leg.
    pub const ALM_STABLE_RESERVE: &str = "alm_stable_reserve";
    pub const ALM_VOLATILE_RESERVE: &str = "alm_volatile_reserve";
    pub const ALM_SUPPLY: &str = "alm_supply";

    // CollVault (ERC-4626) totals.
    pub const CV_TOTAL_ASSETS: &str = "cv_total_assets";
    pub const CV_TOTAL_SUPPLY: &str = "cv_total_supply";
    pub const WITHDRAW_FEE_BP: &str = "withdraw_fee_bp";

    // Interest drift and physical-value reference words.
    pub const INTEREST_RATE: &str = "interest_rate";
    pub const REF_STABLE_RESERVE: &str = "ref_stable_reserve";
    pub const REF_ASSET_RESERVE: &str = "ref_asset_reserve";
    pub const REF_RAW_REFERENCE_WAD: &str = "ref_raw_reference_wad";

    // Deployment-frozen constants, seeded from manifest params (creation only).
    pub const CV_DECIMALS_OFFSET: &str = "cv_decimals_offset";
    pub const MIN_NET_DEBT: &str = "min_net_debt";
    pub const DEBT_GAS_COMPENSATION: &str = "debt_gas_compensation";
    pub const MCR_WAD: &str = "mcr_wad";

    /// 1 byte: 1 = the curve constants below still describe the venue. They live in the
    /// linked `CollRebalancerMath`, which the rebalancer exposes as a `pure`
    /// `leverageCurve()` — so they can only change with a new implementation, and the
    /// proxy's `Upgraded` log is the only notice of one. Cleared there, which stops the
    /// simulation quoting off constants nothing has re-verified.
    pub const CURVE_TRACKED: &str = "curve_tracked";

    // CollRebalancerMath curve constants (creation only, from manifest params).
    pub const LEVERAGE_RATIO_WAD: &str = "leverage_ratio_wad";
    pub const H_ZERO: &str = "h_zero";
    pub const H_JOIN: &str = "h_join";
    pub const H_WALL: &str = "h_wall";
    pub const WIDTH: &str = "width";
    pub const D_JOIN: &str = "d_join";
    pub const D_WALL: &str = "d_wall";
    pub const RESCUE_SPREAD_PPM: &str = "rescue_spread_ppm";
    pub const PHYSICAL_CR_FLOOR_WAD: &str = "physical_cr_floor_wad";
    /// 4 cubic-Bézier control points, 32-byte words concatenated.
    pub const BEZIER_PHI: &str = "bezier_phi";
    /// 5 quartic-Bézier integral knots, 32-byte words concatenated.
    pub const BEZIER_INTEGRAL: &str = "bezier_integral";
}

pub fn attribute(name: &'static str, value: Vec<u8>) -> tycho::Attribute {
    tycho::Attribute { name: name.to_owned(), value, change: tycho::ChangeType::Update.into() }
}

pub fn creation_attribute(name: &'static str, value: Vec<u8>) -> tycho::Attribute {
    tycho::Attribute { name: name.to_owned(), value, change: tycho::ChangeType::Creation.into() }
}
