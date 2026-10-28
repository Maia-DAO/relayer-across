mod utils {
    pub mod constants;
    pub mod op_stack {
        pub mod l1_fee {
            pub mod l1_block_info;
            pub mod l1_block_oracle;
            pub mod l1_fee_oracle;
        }
    }
    pub mod arbitrum {
        pub mod l1_fee {
            pub mod l1_gas_limit;
        }
    }
    pub mod fill;
    pub mod http {
        pub mod alchemy;
        pub mod kairos_data;
        pub mod kairos_stats;
        pub mod kairos_submit;
    }
    pub mod sleep;
    pub mod time;
    pub mod tokio_spawn_helper;
    pub mod v3_spoke_pool;
}
mod types {
    pub mod providers;
}
mod configs {
    pub mod chain_config;
    pub mod connection_config;
    pub mod fill_config;
}

use configs::{
    chain_config::ChainConfig, connection_config::ConnectionConfig,
    fill_config::new_static_fill_config,
};
use dotenv::dotenv;
use eyre::Result;
use futures::{stream::FuturesUnordered, StreamExt};
use std::{collections::hash_map, env};
use tokio_util::{sync::CancellationToken, task::TaskTracker};
use utils::{constants::CHAINS, tokio_spawn_helper::tracker_tokio_spawn_with_cancel_token};

fn get_config(
    origin_chain: &'static ChainConfig,
    dest_chain: &'static ChainConfig,
    simulate: bool,
) -> &'static ConnectionConfig {
    let fill_config = new_static_fill_config(origin_chain, dest_chain, simulate);
    return Box::leak(Box::new(ConnectionConfig::new(
        origin_chain,
        dest_chain,
        fill_config,
    )));
}

async fn create_connection(
    origin_chain: &'static ChainConfig,
    dest_chain: &'static ChainConfig,
    simulate: bool,
) -> &'static ConnectionConfig {
    return get_config(origin_chain, dest_chain, simulate);
}

fn get_chain_from_map(
    chains: &hash_map::HashMap<&u32, &'static ChainConfig>,
    chain: &u32,
) -> &'static ChainConfig {
    return *(chains.get(chain).unwrap());
}

fn add_to_unique_vec(unique_vec: &mut Vec<&'static u32>, value: &'static u32) {
    if !unique_vec.contains(&value) {
        unique_vec.push(value);
    }
}

async fn cleanup(
    chains: &hash_map::HashMap<&u32, &'static ChainConfig>,
    unique_chains: &Vec<&'static u32>,
    unique_dest_chains: &Vec<&'static u32>,
    connections: &Vec<&'static ConnectionConfig>,
) {
    for chain in unique_dest_chains {
        get_chain_from_map(chains, chain)
            .unsub_self_subscriptions()
            .await;
    }
    for connection_config in connections {
        connection_config.unsub_self_subscriptions().await;
    }
    for chain in unique_chains {
        get_chain_from_map(chains, chain)
            .restart_wss_providers()
            .await;
    }
}

async fn run(
    chains: &hash_map::HashMap<&u32, &'static ChainConfig>,
    unique_chains: &Vec<&'static u32>,
    unique_dest_chains: &Vec<&'static u32>,
    connections: &Vec<&'static ConnectionConfig>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    cleanup(chains, unique_chains, unique_dest_chains, connections).await;

    let mut tasks = FuturesUnordered::new();
    let tracker = TaskTracker::new();
    let token = CancellationToken::new();

    for chain in unique_dest_chains {
        get_chain_from_map(chains, chain).start_tasks(&tasks, &tracker, &token);
    }

    // TODO: IF MAINNET IS USED AS AN ORIGIN CHAIN, A BLOCK UDPATER USING AN RPC IS NEEDED TO CHECK FOR RE_ORGS.
    // TODO: Have an array for re-org origin chains to spin up rpc block updater.

    for connection_config in connections {
        let task = tracker_tokio_spawn_with_cancel_token(
            &tracker,
            connection_config.listen(tracker.clone(), token.clone()),
            &token,
        );
        tasks.push(task);
    }

    // Wait until the first task ends or fails
    while let Some(result) = tasks.next().await {
        eprintln!("A task ended: {:?}", result);
        break;
    }

    token.cancel();

    // If a task ends we close the tracker.
    tracker.close();
    // Wait for everything to finish.
    tracker.wait().await;

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load the .env file
    dotenv().ok();

    let simulate = env::var("SIMULATE")
        .expect("SIMULATE not set")
        .parse::<bool>()?;
    println!("Simulating txs: {:?}", simulate);

    let mut unique_chains: Vec<&'static u32> = Vec::new();
    let mut unique_dest_chains: Vec<&'static u32> = Vec::new();

    for (origin_chain_id, dest_chain_id) in CHAINS {
        add_to_unique_vec(&mut unique_chains, origin_chain_id);
        add_to_unique_vec(&mut unique_chains, dest_chain_id);
        add_to_unique_vec(&mut unique_dest_chains, dest_chain_id);
    }

    let mut chains: hash_map::HashMap<&u32, &'static ChainConfig> = hash_map::HashMap::new();

    for chain_id in unique_chains.clone() {
        chains.insert(
            chain_id,
            Box::leak(Box::new(ChainConfig::new(chain_id).await.unwrap())),
        );
    }

    let mut connections: Vec<&'static ConnectionConfig> = Vec::new();

    for (origin_chain_id, dest_chain_id) in CHAINS {
        let origin_chain = get_chain_from_map(&chains, origin_chain_id);
        let dest_chain = get_chain_from_map(&chains, dest_chain_id);

        connections.push(create_connection(origin_chain, dest_chain, simulate).await);
    }

    loop {
        match run(&chains, &unique_chains, &unique_dest_chains, &connections).await {
            Ok(_) => eprintln!("Main terminated for some reason.\n\n\n\n"),
            Err(e) => eprintln!("Error: {:?}\n\n\n\n", e),
        };
        // Wait some time before restarting
        tokio::time::sleep(tokio::time::Duration::from_secs(20)).await;
    }
}
