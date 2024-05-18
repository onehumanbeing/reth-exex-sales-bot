use futures::Future;
use reth_exex::{ExExContext,};
use reth_node_api::FullNodeComponents;
use reth_node_ethereum::EthereumNode;
use reth_tracing::tracing::info;
use reth_provider::Chain;
use reth_primitives::{
        SealedBlockWithSenders,
        TransactionSigned, Log
};
use alloy_sol_types::{sol, SolEventInterface};
use serde_json::json;
use redis::AsyncCommands;
use redis::aio::Connection;

sol!(PoolContract, "abi.json");
use PoolContract::{PoolContractEvents};
use crate::PoolContract::Swap;

async fn exex_init<Node: FullNodeComponents>(
    ctx: ExExContext<Node>,
) -> eyre::Result<impl Future<Output = eyre::Result<()>>> {
    let client = redis::Client::open("redis://127.0.0.1/")?;
    let mut connection: Connection = client.get_async_connection().await?;
    Ok(exex(ctx, connection))
}

async fn exex<Node: FullNodeComponents>(mut ctx: ExExContext<Node>, mut connection: Connection) -> eyre::Result<()> {
    while let Some(notification) = ctx.notifications.recv().await {
        if let Some(reverted_chain) = notification.committed_chain() {
            let events = decode_chain_into_events(&reverted_chain);
            for (block, tx, log, event) in events {
                match event {
                    PoolContractEvents::Swap(swap_event) => {
                        let data_str = serialize_swap_event(&log, &swap_event, &block).await?;
                        let _: () = connection.rpush("sales", data_str).await?;
                    }
                    _ => (),
                }
            }    
        }
    }
    Ok(())
}

/// Decode chain of blocks into a flattened list of receipt logs, and filter only
fn decode_chain_into_events(
    chain: &Chain,
) -> impl Iterator<Item = (&SealedBlockWithSenders, &TransactionSigned, &Log, PoolContractEvents)>
{
    chain
        .blocks_and_receipts()
        .flat_map(|(block, receipts)| {
            block
                .body
                .iter()
                .zip(receipts.iter().flatten())
                .map(move |(tx, receipt)| (block, tx, receipt))
        })
        .flat_map(|(block, tx, receipt)| {
            receipt
                .logs
                .iter()
                .map(move |log| (block, tx, log))
        })
        .filter_map(|(block, tx, log)| {
            PoolContractEvents::decode_raw_log(log.topics(), &log.data.data, true)
                .ok()
                .map(|event| (block, tx, log, event))
        })
}

async fn serialize_swap_event(
    log: &Log,
    swap_event: &Swap,
    block: &SealedBlockWithSenders,
) -> eyre::Result<String> {
    let data = json!({
        "address": log.address,
        "sender": swap_event.sender,
        "recipient": swap_event.recipient,
        "amount0": swap_event.amount0,
        "amount1": swap_event.amount1,
        "liquidity": swap_event.liquidity,
        "timestamp": block.timestamp
    });
    let data_str = serde_json::to_string(&data)?;
    Ok(data_str)
}

fn main() -> eyre::Result<()> {
    reth::cli::Cli::parse_args().run(|builder, _| async move {
        let handle = builder
            .node(EthereumNode::default())
            .install_exex("SaleBot", exex_init)
            .launch()
            .await?;
        handle.wait_for_node_exit().await
    })
}