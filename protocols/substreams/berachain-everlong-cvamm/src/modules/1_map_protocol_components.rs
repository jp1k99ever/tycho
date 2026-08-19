use anyhow::Result;
use substreams_ethereum::pb::eth;
use tycho_substreams::prelude as tycho;

use crate::{
    abi::cvamm_alm::events as alm_events,
    cvamm,
    modules::config::{AlmConfig, Config},
};

/// Emits a `ProtocolComponent` for each configured ALM, in the transaction where its
/// `Initialized(token0, token1, anchorPoolX96)` log is witnessed. `bootstrap_block`
/// serves as a fallback for replays that start after the ALM's deployment: at that
/// block the component is claimed from params without needing the creation log.
#[substreams::handlers::map]
pub fn map_protocol_components(
    params: String,
    block: eth::v2::Block,
) -> Result<tycho::BlockTransactionProtocolComponents> {
    let config = Config::parse(&params)?;
    let mut tx_components = Vec::<tycho::TransactionProtocolComponents>::new();

    for pool in config.pools.iter() {
        let tx = block
            .transactions()
            .find(|tx| has_initialized_log(tx, pool))
            .or_else(|| {
                (pool.bootstrap_block == Some(block.number))
                    .then(|| block.transactions().next())
                    .flatten()
            });
        let Some(tx) = tx else {
            continue;
        };

        let component = cvamm::protocol_component(pool.alm, pool.stable, pool.volatile);
        if let Some(existing) = tx_components
            .iter_mut()
            .find(|tx_components| {
                tx_components
                    .tx
                    .as_ref()
                    .is_some_and(|known| known.hash == tx.hash)
            })
        {
            existing.components.push(component);
        } else {
            tx_components.push(tycho::TransactionProtocolComponents {
                tx: Some(tx.into()),
                components: vec![component],
            });
        }
    }

    Ok(tycho::BlockTransactionProtocolComponents { tx_components })
}

fn has_initialized_log(tx: &eth::v2::TransactionTrace, pool: &AlmConfig) -> bool {
    tx.logs_with_calls()
        .any(|(log, _)| log.address == pool.alm && alm_events::Initialized::match_log(log))
}
