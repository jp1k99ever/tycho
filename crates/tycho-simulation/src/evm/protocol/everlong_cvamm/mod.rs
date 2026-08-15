//! Native simulation of the Everlong CVAMM venue (Berachain): a single-LP AMM on a
//! closed-form reservation curve where the ALM contract is the pool, the swap
//! entrypoint and the LP share token at once. All curve and fee constants are
//! decoded from component attributes — nothing protocol-specific is hardcoded.
mod decoder;
pub mod fee_law;
pub mod fee_law_exact;
pub mod math;
mod state;

pub use state::EverlongCvammState;
