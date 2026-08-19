//! Native simulation of the Everlong CollateralRebalancer venue (Berachain): NECT<->WBTC swaps
//! settled through the CollateralRebalancerSwapper against a leveraged CDP position
//! priced by a piecewise CR bonding curve. All curve constants are decoded from
//! component attributes — nothing protocol-specific is hardcoded.
mod decoder;
pub mod math;
mod state;

pub use state::EverlongRebalancerState;
