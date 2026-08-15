use anyhow::Result;
use substreams_ethereum::pb::eth;
use tycho_substreams::prelude as tycho;

use crate::{
    abi::rebalancer::events as rebalancer_events,
    collvault,
    modules::config::{Config, VenueConfig},
};

/// Emits the venue's `ProtocolComponent` in the transaction where the rebalancer's
/// `SettlementSwapperSet` names the configured swapper (the moment the venue becomes
/// routable). `bootstrap_block` is the fallback for replays starting later.
#[substreams::handlers::map]
pub fn map_protocol_components(
    params: String,
    block: eth::v2::Block,
) -> Result<tycho::BlockTransactionProtocolComponents> {
    let config = Config::parse(&params)?;
    let venue = &config.venue;

    let tx = block
        .transactions()
        .find(|tx| names_configured_swapper(tx, venue))
        .or_else(|| {
            (venue.bootstrap_block == Some(block.number))
                .then(|| block.transactions().next())
                .flatten()
        });
    let Some(tx) = tx else {
        return Ok(tycho::BlockTransactionProtocolComponents { tx_components: vec![] });
    };

    let component = collvault::protocol_component(venue.swapper, venue.stable, venue.volatile);
    Ok(tycho::BlockTransactionProtocolComponents {
        tx_components: vec![tycho::TransactionProtocolComponents {
            tx: Some(tx.into()),
            components: vec![component],
        }],
    })
}

fn names_configured_swapper(tx: &eth::v2::TransactionTrace, venue: &VenueConfig) -> bool {
    use substreams_ethereum::Event as _;

    tx.logs_with_calls().any(|(log, _)| {
        log.address == venue.rebalancer &&
            rebalancer_events::SettlementSwapperSet::match_and_decode(log)
                .is_some_and(|event| event.settlement_swapper == venue.swapper)
    })
}
