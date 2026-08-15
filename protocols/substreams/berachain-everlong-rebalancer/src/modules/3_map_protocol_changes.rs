use std::collections::HashMap;

use anyhow::Result;
use substreams::store::{StoreGet, StoreGetProto};
use substreams_ethereum::pb::eth;
use tycho_substreams::prelude as tycho;

use crate::{
    modules::{config::Config, store_protocol_components::component_key},
    rebalancer,
};

#[substreams::handlers::map]
pub fn map_protocol_changes(
    params: String,
    block: eth::v2::Block,
    new_components: tycho::BlockTransactionProtocolComponents,
    component_store: StoreGetProto<tycho::ProtocolComponent>,
) -> Result<tycho::BlockChanges> {
    let config = Config::parse(&params)?;
    let venue = &config.venue;
    let component_id = venue.component_id();
    let mut component_known = component_store
        .get_last(component_key(&component_id))
        .is_some();
    let mut transaction_changes = HashMap::<u64, tycho::TransactionChangesBuilder>::new();

    for tx_components in new_components.tx_components.iter() {
        let Some(tx) = tx_components.tx.as_ref() else {
            continue;
        };
        let builder = transaction_changes
            .entry(tx.index)
            .or_insert_with(|| tycho::TransactionChangesBuilder::new(tx));
        for component in tx_components.components.iter() {
            if component.id != component_id {
                continue;
            }
            component_known = true;
            builder.add_protocol_component(component);
            builder.add_entity_change(&rebalancer::indexed::initial_entity_change(
                &component.id,
                venue,
            ));
        }
    }

    if component_known {
        for tx in block.transactions() {
            for (log, _) in tx.logs_with_calls() {
                let event = if log.address == venue.rebalancer {
                    rebalancer::events::decode_rebalancer_log(log)
                } else if log.address == venue.alm {
                    rebalancer::events::decode_alm_log(log)
                } else {
                    None
                };
                let Some(event) = event else {
                    continue;
                };

                let tx: tycho::Transaction = tx.into();
                let builder = transaction_changes
                    .entry(tx.index)
                    .or_insert_with(|| tycho::TransactionChangesBuilder::new(&tx));
                if let Some(entity_change) =
                    rebalancer::indexed::entity_change_for_event(&component_id, venue, &event)
                {
                    builder.add_entity_change(&entity_change);
                }
                for balance_change in rebalancer::indexed::balance_changes_for_event(
                    &component_id,
                    &venue.stable,
                    &venue.volatile,
                    &event,
                ) {
                    builder.add_balance_change(&balance_change);
                }
            }
        }
    }

    let mut changes = transaction_changes
        .into_values()
        .filter_map(tycho::TransactionChangesBuilder::build)
        .collect::<Vec<_>>();
    changes.sort_unstable_by_key(|changes| {
        changes
            .tx
            .as_ref()
            .map(|tx| tx.index)
            .unwrap_or_default()
    });

    Ok(tycho::BlockChanges { block: Some((&block).into()), changes, ..Default::default() })
}
