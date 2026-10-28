use super::super::types::providers::ChainProvider;
use super::super::utils::{
    arbitrum::l1_fee::l1_gas_limit::L1GasLimit,
    constants::{
        get_block_interval, get_chain_id_u256, get_chain_id_u64, get_gas_update_interval,
        get_tx_estimate_limit, get_tx_process_ms_interval, CHAIN_ID_ARB, CHAIN_ID_BASE,
        CHAIN_ID_ETH, CHAIN_ID_OP, KAIROS_BRIBER_ADDRESS, RELAYER_ADDRESS, RELAYER_BYTES32,
    },
    fill::Fill,
    http::kairos_data::kairos_express_lane_status,
    op_stack::l1_fee::l1_fee_oracle::L1FeeOracle,
    sleep::{fixed_interval_loop, fixed_interval_loop_async, to_millis, wait_for_timestamp_ms},
    time::rfc3339_to_unix,
    tokio_spawn_helper::tracker_tokio_spawn,
    tokio_spawn_helper::tracker_tokio_spawn_with_cancel_token,
    v3_spoke_pool::{build_v3_spoke_pool_calldata, IV3SpokePool},
};
use alloy::consensus::{EthereumTxEnvelope, TxEip4844Variant};
use alloy::eips::BlockId;
use alloy::providers::RootProvider;
use alloy::rpc::types::Block;
use alloy::{
    network::{EthereumWallet, TransactionBuilder},
    primitives::{address, ruint::uint, Address, B256, U256},
    providers::{Provider, ProviderBuilder, WsConnect},
    pubsub::Subscription,
    rpc::types::{BlockNumberOrTag, Filter, Log, TransactionRequest},
    signers::local::PrivateKeySigner,
    sol,
    sol_types::SolEvent,
    transports::TransportError,
};
use eyre::Result;
use futures::{stream::FuturesUnordered, StreamExt};
use std::collections::BinaryHeap;
use std::time::{Duration, SystemTime};
use std::{
    env,
    sync::{Arc, RwLock},
};
use time::format_description::well_known::Iso8601;
use time::OffsetDateTime;
use tokio::sync::{
    watch::{self, Receiver, Sender},
    Mutex,
};
use tokio::task::JoinHandle;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;
use IV3SpokePool::IV3SpokePoolInstance;
use IERC20::IERC20Instance;

// Codegen from ABI file to interact with the contract.
sol!(
    #[allow(missing_docs)]
    #[sol(rpc)]
    IERC20,
    "src/abis/ERC20.json"
);

fn get_fromatted_time() -> String {
    let now: OffsetDateTime = SystemTime::now().into();
    return now.format(&Iso8601::DEFAULT).unwrap();
}

const KAIROS_ROUND_DURATION: Duration = Duration::from_secs(60);
const KAIROS_AUCTION_ADDRESS: Address = address!("2b38a73dd32a2eafe849825a4b515ae5187eda42");
pub const KAIROS_PAYMENT_ADDRESS: Address = address!("60E6a31591392f926e627ED871e670C3e81f1AB8");

const SKIP_NON_EXCLUSIVE_FILLS_THRESHOLD_ETH: U256 = uint!(100_000_000_000_000_U256);
const SKIP_NON_EXCLUSIVE_FILLS_THRESHOLD_ARB: U256 = uint!(50_000_000_000_000_000_U256);
const SKIP_NON_EXCLUSIVE_FILLS_THRESHOLD_OP_STACK: U256 = uint!(100_000_000_000_000_000_U256);
const SKIP_NON_EXCLUSIVE_FILLS_THRESHOLD_BASE: U256 = uint!(500_000_000_000_000_000_U256);

fn get_skip_non_exclusive_fills_threshold(chain_id: &'static u32) -> &'static U256 {
    match *chain_id {
        CHAIN_ID_ETH => &SKIP_NON_EXCLUSIVE_FILLS_THRESHOLD_ETH,
        CHAIN_ID_ARB => &SKIP_NON_EXCLUSIVE_FILLS_THRESHOLD_ARB,
        CHAIN_ID_OP => &SKIP_NON_EXCLUSIVE_FILLS_THRESHOLD_OP_STACK,
        CHAIN_ID_BASE => &SKIP_NON_EXCLUSIVE_FILLS_THRESHOLD_BASE,
        _ => panic!("Invalid chain id: {}", chain_id),
    }
}

const TOKEN_ADDRESS_1: Address = address!("c02aaa39b223fe8d0a0e5c4f27ead9083c756cc2");
const TOKEN_ADDRESS_42161: Address = address!("82af49447d8a07e3bd95bd0d56f35241523fbab1");
const TOKEN_ADDRESS_10: Address = address!("4200000000000000000000000000000000000006");
const TOKEN_ADDRESS_8453: Address = TOKEN_ADDRESS_10;

fn get_token_address(chain_id: &'static u32) -> &'static Address {
    match *chain_id {
        CHAIN_ID_ETH => &TOKEN_ADDRESS_1,
        CHAIN_ID_ARB => &TOKEN_ADDRESS_42161,
        CHAIN_ID_OP => &TOKEN_ADDRESS_10,
        CHAIN_ID_BASE => &TOKEN_ADDRESS_8453,
        _ => panic!("Invalid chain id: {}", chain_id),
    }
}

const SEQUENCER_URL_1: &str = "https://rpc.flashbots.net/fast?blockRange=1";
const SEQUENCER_URL_42161: &str = "https://arb1-sequencer.arbitrum.io/rpc";
const SEQUENCER_URL_10: &str = "https://mainnet-sequencer.optimism.io";
const SEQUENCER_URL_8453: &str = "https://mainnet-sequencer.base.org";

fn get_sequencer_url(chain_id: &'static u32) -> &'static str {
    match *chain_id {
        CHAIN_ID_ETH => SEQUENCER_URL_1,
        CHAIN_ID_ARB => SEQUENCER_URL_42161,
        CHAIN_ID_OP => SEQUENCER_URL_10,
        CHAIN_ID_BASE => SEQUENCER_URL_8453,
        _ => panic!("Invalid chain id: {}", chain_id),
    }
}

const ALCHEMY_URL_1: &str = "://eth-mainnet.g.alchemy.com/v2/";
const ALCHEMY_URL_42161: &str = "://arb-mainnet.g.alchemy.com/v2/";
const ALCHEMY_URL_10: &str = "://opt-mainnet.g.alchemy.com/v2/";
const ALCHEMY_URL_8543: &str = "://base-mainnet.g.alchemy.com/v2/";

fn get_alchemy_wss(chain_id: &'static u32) -> String {
    return "wss".to_owned() + get_alchemy_url(chain_id).as_str();
}

fn get_alchemy_http(chain_id: &'static u32) -> String {
    return "https".to_owned() + get_alchemy_url(chain_id).as_str();
}

fn get_alchemy_url(chain_id: &'static u32) -> String {
    let alchemy_key: &'static String = Box::leak(Box::new(
        env::var("ALCHEMY_KEY").expect("ALCHEMY_KEY not set"),
    ));

    let url = match *chain_id {
        CHAIN_ID_ETH => ALCHEMY_URL_1,
        CHAIN_ID_ARB => ALCHEMY_URL_42161,
        CHAIN_ID_OP => ALCHEMY_URL_10,
        CHAIN_ID_BASE => ALCHEMY_URL_8543,
        _ => panic!("Invalid chain id: {}", chain_id),
    };

    return url.to_string() + alchemy_key;
}

const BLAST_URL_1: &str = "://eth-mainnet.blastapi.io/";
const BLAST_URL_42161: &str = "://arbitrum-one.blastapi.io/";
const BLAST_URL_10: &str = "://optimism-mainnet.blastapi.io/";
const BLAST_URL_8543: &str = "://base-mainnet.blastapi.io/";

fn get_blast_wss(chain_id: &'static u32) -> String {
    return "wss".to_owned() + get_blast_url(chain_id).as_str();
}

fn get_blast_http(chain_id: &'static u32) -> String {
    return "https".to_owned() + get_blast_url(chain_id).as_str();
}

fn get_blast_url(chain_id: &'static u32) -> String {
    let blast_key: &'static String =
        Box::leak(Box::new(env::var("BLAST_KEY").expect("BLAST_KEY not set")));

    let url = match *chain_id {
        CHAIN_ID_ETH => BLAST_URL_1,
        CHAIN_ID_ARB => BLAST_URL_42161,
        CHAIN_ID_OP => BLAST_URL_10,
        CHAIN_ID_BASE => BLAST_URL_8543,
        _ => panic!("Invalid chain id: {}", chain_id),
    };

    return url.to_string() + blast_key;
}

const SPOKE_POOL_1: Address = address!("5c7BCd6E7De5423a257D81B442095A1a6ced35C5");
const SPOKE_POOL_42161: Address = address!("e35e9842fceaCA96570B734083f4a58e8F7C5f2A");
const SPOKE_POOL_10: Address = address!("6f26Bf09B1C792e3228e5467807a900A503c0281");
const SPOKE_POOL_8453: Address = address!("09aea4b2242abC8bb4BB78D537A67a245A7bEC64");

fn get_spoke_pool(chain_id: &'static u32) -> &'static Address {
    match *chain_id {
        CHAIN_ID_ETH => &SPOKE_POOL_1,
        CHAIN_ID_ARB => &SPOKE_POOL_42161,
        CHAIN_ID_OP => &SPOKE_POOL_10,
        CHAIN_ID_BASE => &SPOKE_POOL_8453,
        _ => panic!("Invalid chain id: {}", chain_id),
    }
}

async fn build_wss_provider(wss: &String) -> Result<ChainProvider, TransportError> {
    // Set up the provider
    let ws = WsConnect::new(wss);
    return ProviderBuilder::new().connect_ws(ws).await;
}

fn build_http_provider(http: &String) -> ChainProvider {
    // Set up the provider
    let rpc_url = http.parse().unwrap();
    return ProviderBuilder::new().connect_http(rpc_url);
}

fn build_sequencer_provider(sequencer_rpc: &String) -> RootProvider {
    // Set up the provider
    let rpc_url = sequencer_rpc.parse().unwrap();
    return ProviderBuilder::new()
        .disable_recommended_fillers()
        .connect_http(rpc_url);
}

fn build_wallet() -> EthereumWallet {
    let pk: &'static String = Box::leak(Box::new(env::var("PK").expect("PK not set")));
    let signer: PrivateKeySigner = pk.parse().expect("should parse private key");
    return EthereumWallet::from(signer);
}

fn build_kairos_wallet() -> EthereumWallet {
    let pk: &'static String =
        Box::leak(Box::new(env::var("PK_KAIROS").expect("PK_KAIROS not set")));
    let signer: PrivateKeySigner = pk.parse().expect("should parse kairos private key");
    return EthereumWallet::from(signer);
}

async fn get_balance_of(token: &IERC20Instance<ChainProvider>, relayer: &Address) -> U256 {
    return token.balanceOf(*relayer).call().await.unwrap();
}

fn build_erc20_contract(
    chain_id: &'static u32,
    http_provider: &ChainProvider,
) -> IERC20Instance<ChainProvider> {
    return IERC20::new(*get_token_address(chain_id), http_provider.clone());
}

async fn get_gas_price(
    http_provider: &ChainProvider,
) -> Result<U256, Box<dyn std::error::Error + Send + Sync>> {
    return Ok(U256::from(http_provider.get_gas_price().await?));
}

async fn get_latest_block(
    http_provider: &ChainProvider,
) -> Result<Option<Block>, Box<dyn std::error::Error + Send + Sync>> {
    Ok(http_provider
        .get_block(BlockId::Number(BlockNumberOrTag::Latest))
        .await?)
}

// Increasing this value takes into account for more failed txs but less txs are being considered profitable.
// NOTE: This does not take into account addresses with no prev balance (gas used is 128k).
pub const GAS_USED_STANDARD: u64 = 105_000;
const GAS_USED_MAINNET: u64 = 130_000;

fn get_gas_used(chain_id: &'static u32) -> &'static u64 {
    match *chain_id {
        CHAIN_ID_ETH => &GAS_USED_MAINNET,
        _ => &GAS_USED_STANDARD,
    }
}

const GAS_LIMIT_STANDARD: u64 = 150_000;
const GAS_LIMIT_ARB: u64 = 200_000;

fn get_gas_limit(chain_id: &'static u32) -> &'static u64 {
    match *chain_id {
        CHAIN_ID_ARB => &GAS_LIMIT_ARB,
        _ => &GAS_LIMIT_STANDARD,
    }
}

async fn unsubscribe_logs(
    wss_provider: &ChainProvider,
    local_id: &B256,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if *local_id != B256::ZERO {
        (*wss_provider).unsubscribe(*local_id).await?;
    }
    Ok(())
}

async fn subscribe_logs(
    wss_provider: &ChainProvider,
    local_id: &Mutex<B256>,
    filter: &Filter,
) -> Result<Subscription<Log>, Box<dyn std::error::Error + Send + Sync>> {
    let mut local_id = local_id.lock().await;
    let _ = unsubscribe_logs(wss_provider, &(*local_id)).await;

    let sub = (*wss_provider).subscribe_logs(filter).await?;
    *local_id = *sub.local_id();

    return Ok(sub);
}

async fn restart_wss(
    wss: &String,
    provider: &Arc<RwLock<ChainProvider>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let new_wss = build_wss_provider(wss).await?;
    // If the line above fails, we should not reach here
    *provider.write().unwrap() = new_wss;
    Ok(())
}

pub struct ChainConfig {
    chain_id: &'static U256,
    pub chain_id_u64: &'static u64,
    pub contract_address: &'static Address,
    contract_instance: IV3SpokePoolInstance<ChainProvider>,

    pub is_arbitrum: bool,
    is_op_stack: bool,
    pub supports_flashbots: bool,

    pub block_interval: &'static Duration,
    gas_update_interval: &'static Duration,
    tx_process_ms_interval: &'static u64,
    tx_estimate_limit: &'static Duration,

    fills_queued: Arc<Mutex<Vec<Arc<Fill>>>>,

    blast_wss: String,
    alchemy_wss: String,
    pub wss_provider: Arc<RwLock<ChainProvider>>,
    pub wss_fallback: Arc<RwLock<ChainProvider>>,

    block_subscription: Mutex<B256>,

    balance_in_subscription: Mutex<B256>,
    balance_out_subscription: Mutex<B256>,

    pub simulation_http: String,
    pub http_provider: ChainProvider,

    pub sequencer_provider: RootProvider,
    signer: EthereumWallet,
    kairos_signer: EthereumWallet,

    pub relayer_bytes32: &'static B256,
    pub relayer_address: &'static Address,
    pub nonce: Arc<Mutex<u64>>,

    kairos_briber_address: &'static Address,
    kairos_briber_nonce: Arc<Mutex<u64>>,

    token: IERC20Instance<ChainProvider>,
    pub balance: Arc<RwLock<U256>>,
    skip_non_exclusive_threshold: &'static U256,
    pub skip_non_exclusive: Arc<RwLock<bool>>,

    start_block: Block,

    gas_limit: &'static u64,
    pub gas_used: &'static u64,
    pub base_fee: Arc<RwLock<U256>>,
    l1_fee_oracle: Option<&'static L1FeeOracle>,
    l1_gas_limit: Option<&'static L1GasLimit>,

    pub block_timestamp_receiver: Receiver<u64>,
    block_timestamp_sender: Sender<u64>,

    kairos_round: Arc<RwLock<u32>>,
    use_kairos: Arc<RwLock<bool>>,
}

impl ChainConfig {
    pub async fn new(
        chain_id: &'static u32,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let blast_wss = get_blast_wss(chain_id);
        let alchemy_wss = get_alchemy_wss(chain_id);

        let simulation_http = get_alchemy_http(chain_id).to_string();

        let contract_address = get_spoke_pool(chain_id);

        let wss_provider = build_wss_provider(&blast_wss).await?;
        let wss_fallback = build_wss_provider(&alchemy_wss).await?;

        let blast_http = get_blast_http(chain_id).to_string();
        let http_provider = build_http_provider(&blast_http);

        let nonce = http_provider.get_transaction_count(RELAYER_ADDRESS).await?;
        let kairos_briber_nonce = http_provider
            .get_transaction_count(KAIROS_BRIBER_ADDRESS)
            .await?;

        let token = build_erc20_contract(chain_id, &http_provider);
        let balance = get_balance_of(&token, &RELAYER_ADDRESS).await;
        let skip_non_exclusive_threshold = get_skip_non_exclusive_fills_threshold(chain_id);
        let skip_non_exclusive = Arc::new(RwLock::new(balance < *skip_non_exclusive_threshold));
        let balance = Arc::new(RwLock::new(balance));

        let l1_fee_oracle = L1FeeOracle::new(chain_id, &http_provider).await;
        let l1_gas_limit = L1GasLimit::new(chain_id, &http_provider, contract_address).await;

        let base_fee = get_gas_price(&http_provider).await?;
        let latest_block = get_latest_block(&http_provider).await?.unwrap();

        let (sender, receiver) = watch::channel(0);

        Ok(Self {
            chain_id: get_chain_id_u256(chain_id),
            chain_id_u64: get_chain_id_u64(chain_id),
            contract_address,
            contract_instance: IV3SpokePool::new(*contract_address, wss_provider.clone()),

            is_arbitrum: *chain_id == CHAIN_ID_ARB,
            is_op_stack: *chain_id == CHAIN_ID_OP || *chain_id == CHAIN_ID_BASE,
            supports_flashbots: *chain_id == CHAIN_ID_ETH,

            block_interval: get_block_interval(chain_id),
            gas_update_interval: get_gas_update_interval(chain_id),
            tx_process_ms_interval: get_tx_process_ms_interval(chain_id),
            tx_estimate_limit: get_tx_estimate_limit(chain_id),

            fills_queued: Arc::new(Mutex::new(Vec::new())),

            blast_wss: blast_wss.clone(),
            alchemy_wss: alchemy_wss.clone(),
            wss_provider: Arc::new(RwLock::new(wss_provider)),
            wss_fallback: Arc::new(RwLock::new(wss_fallback)),

            block_subscription: Mutex::new(B256::ZERO),

            balance_in_subscription: Mutex::new(B256::ZERO),
            balance_out_subscription: Mutex::new(B256::ZERO),

            simulation_http,
            http_provider,

            sequencer_provider: build_sequencer_provider(&get_sequencer_url(chain_id).to_string()),
            signer: build_wallet(),
            kairos_signer: build_kairos_wallet(),

            relayer_bytes32: &RELAYER_BYTES32,
            relayer_address: &RELAYER_ADDRESS,
            nonce: Arc::new(Mutex::new(nonce)),

            kairos_briber_address: &KAIROS_BRIBER_ADDRESS,
            kairos_briber_nonce: Arc::new(Mutex::new(kairos_briber_nonce)),

            token,
            balance,
            skip_non_exclusive_threshold,
            skip_non_exclusive,

            start_block: latest_block,

            gas_limit: get_gas_limit(chain_id),
            gas_used: get_gas_used(chain_id),
            base_fee: Arc::new(RwLock::new(base_fee)),
            l1_fee_oracle: match l1_fee_oracle {
                Some(fee_oracle) => Some(Box::leak(Box::new(fee_oracle))),
                None => None,
            },
            l1_gas_limit: match l1_gas_limit {
                Some(l1_gas_limit_oracle) => Some(Box::leak(Box::new(l1_gas_limit_oracle))),
                None => None,
            },

            block_timestamp_receiver: receiver,
            block_timestamp_sender: sender,

            kairos_round: Arc::new(RwLock::new(0)),
            use_kairos: Arc::new(RwLock::new(false)),
        })
    }

    pub fn start_tasks(
        &'static self,
        tasks: &FuturesUnordered<JoinHandle<()>>,
        tracker: &TaskTracker,
        token: &CancellationToken,
    ) {
        let balance_in_task =
            tracker_tokio_spawn_with_cancel_token(tracker, self.balance_in_updater(), token);
        let balance_out_task =
            tracker_tokio_spawn_with_cancel_token(tracker, self.balance_out_updater(), token);
        let gas_task = tracker_tokio_spawn_with_cancel_token(tracker, self.gas_updater(), token);
        tasks.push(balance_in_task);
        tasks.push(balance_out_task);
        tasks.push(gas_task);

        if self.is_arbitrum {
            let timestamp_task = tracker_tokio_spawn_with_cancel_token(
                tracker,
                self.block_timestamp_updater(),
                token,
            );
            tasks.push(timestamp_task);
            self.start_kairos_task(tasks, tracker, token);
        } else {
            let tx_submit_task: tokio::task::JoinHandle<()> =
                tracker_tokio_spawn_with_cancel_token(tracker, self.tx_submit_updater(), token);
            tasks.push(tx_submit_task);
        }
    }

    fn start_kairos_task(
        &'static self,
        tasks: &FuturesUnordered<JoinHandle<()>>,
        tracker: &TaskTracker,
        token: &CancellationToken,
    ) {
        match env::var("KAIROS_ENABLED") {
            Ok(kairos_enabled) => {
                let kairos_enabled = kairos_enabled.to_lowercase();

                if kairos_enabled == "true" {
                    let kairos_task = tracker_tokio_spawn_with_cancel_token(
                        tracker,
                        self.kairos_auction_stream_updater(),
                        token,
                    );
                    tasks.push(kairos_task);
                }
            }
            Err(e) => self.log(
                format!(
                    "KAIROS_ENABLED not set (will not be enabled by default), {:?}",
                    e
                )
                .as_str(),
            ),
        }
    }

    pub fn log(&'static self, message: &str) {
        println!(
            "{} {} {}",
            &get_fromatted_time(),
            self.chain_id_u64,
            message
        );
    }

    pub async fn kairos_auction_stream_updater(
        &'static self,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.log("Listening for Kairo Auction Stream");

        fixed_interval_loop_async(
            &self.get_kairo_start_timestamp(),
            &KAIROS_ROUND_DURATION, // 60s
            self,
            process_kairos_auction_stream,
        )
        .await;

        Ok(())
    }

    fn get_kairo_start_timestamp(&'static self) -> u64 {
        // https://kairos-timeboost.xyz/
        // Kairo rounds start every 60s at the 51s of every min (ex: 14:09:51)
        // Get update 5s before the new rounds
        to_millis(&((self.start_block.header.timestamp / 60) * 60 - 14))
    }

    pub fn use_kairos(&'static self) -> bool {
        *self.use_kairos.read().unwrap()
    }

    fn set_use_kairos_true(&'static self) {
        *self.use_kairos.write().unwrap() = true;
    }

    fn set_use_kairos_false(&'static self) {
        *self.use_kairos.write().unwrap() = false;
    }

    pub async fn kairos_auction_stream(&'static self, next_timestamp: &u64) {
        let prev_kairos_round = *self.kairos_round.read().unwrap();

        // Kairo rounds start every 60s at the 51s of every min (ex: 14:09:51)
        // `next_timestamp` is at 46s of the next min. Round ends at (including) 50s of every min (ex: 14:09:50).
        let round_end = next_timestamp + 4;

        let mut counter = 0;

        loop {
            let value = kairos_express_lane_status().await.unwrap();

            if value.round > prev_kairos_round {
                match rfc3339_to_unix(&value.round_end_timestamp) {
                    Ok(round_end_unix) => {
                        if round_end == round_end_unix {
                            *self.kairos_round.write().unwrap() = value.round;

                            if value.next_holder.address == KAIROS_AUCTION_ADDRESS {
                                let kairos_submit_end_ms = to_millis(&round_end);
                                let start_timestamp_ms = kairos_submit_end_ms - 59_000;

                                self.log(
                                    format!(
                                        "🥳 Kairos won round {:?} starting at: {:?}",
                                        value.round, value.round_start_timestamp
                                    )
                                    .as_str(),
                                );

                                wait_for_timestamp_ms(&start_timestamp_ms).await;

                                // Start adding fills to fills_queued
                                self.set_use_kairos_true();

                                wait_for_timestamp_ms(&kairos_submit_end_ms).await;
                                // Stop adding fills to fills_queued
                                self.set_use_kairos_false();

                                self.log(format!("😫 Stopped using kairos",).as_str());

                                self.reset_kairos_briber_nonce().await;
                            }

                            return;
                        }
                    }
                    Err(e) => eprintln!("Error parsing time: {}", e),
                }
            }

            // 20 max tries, so we don't enter endless loops here. 20 should be more than enough
            if counter > 19 {
                self.log(
                    format!(
                        "Max tries on kairos, did not get new round, prev was: {:?} {:?}",
                        value.round, value.next_holder.address
                    )
                    .as_str(),
                );
                return;
            }

            counter += 1;
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        }
    }

    fn log_skip_reason(&'static self, decoded_data: &IV3SpokePool::FundsDeposited, skip: &str) {
        self.log(format!("{} Deposit skipped: {}", decoded_data.depositId, skip).as_str());
    }

    fn log_fill(&'static self, fill: &Fill, skip: &str) {
        fill.log(format!("{} {}", fill.deposit_id, skip).as_str());
    }

    pub async fn unsub_subscription(&'static self, subscription: &Mutex<B256>) {
        let wss_provider = (*self.wss_provider.read().unwrap()).clone();
        let mut local_id = subscription.lock().await;
        let _ = unsubscribe_logs(&wss_provider, &(*local_id)).await;
        *local_id = B256::ZERO;
    }

    pub async fn unsub_self_subscriptions(&'static self) {
        self.unsub_subscription(&self.balance_in_subscription).await;
        self.unsub_subscription(&self.balance_out_subscription)
            .await;
        self.unsub_subscription(&self.block_subscription).await;
    }

    pub async fn restart_wss_providers(&'static self) {
        let _ = restart_wss(&self.blast_wss, &self.wss_provider).await;
        let _ = restart_wss(&self.alchemy_wss, &self.wss_fallback).await;
    }

    pub fn get_l1_fee(&'static self, tx: &TransactionRequest) -> U256 {
        match self.l1_fee_oracle {
            Some(fee_oracle) => fee_oracle.get_l1_fee(tx),
            None => U256::ZERO,
        }
    }

    pub fn get_l1_gas_limit_buffer(&'static self) -> u64 {
        match self.l1_gas_limit {
            Some(l1_gas_limit) => *l1_gas_limit.l1_gas_limit_buffer.read().unwrap(),
            None => 0,
        }
    }

    pub fn get_l1_gas_limit_buffer_eth_transfer(&'static self) -> u64 {
        match self.l1_gas_limit {
            Some(l1_gas_limit) => *l1_gas_limit
                .l1_gas_limit_buffer_eth_transfer
                .read()
                .unwrap(),
            None => 0,
        }
    }

    fn update_base_fee_per_gas(&'static self, base_fee_per_gas: U256) {
        *self.base_fee.write().unwrap() = base_fee_per_gas;
    }

    fn propagate_block_timestamp(&'static self, timestamp: u64) {
        self.block_timestamp_sender.send(timestamp).unwrap();
    }

    // Only used for Arbitrum (has FIFO sequencing).
    pub async fn wait_for_exclusive_deadline(&'static self, exclusive_deadline: &u64) -> bool {
        let mut rx = self.block_timestamp_receiver.clone();

        // Use the equivalent of a "do-while" loop so the initial value is
        // processed before awaiting the `changed()` future.
        loop {
            let timestamp = *rx.borrow_and_update();
            if timestamp > *exclusive_deadline {
                return false;
            }
            let remaining_time = exclusive_deadline - timestamp;
            // NOTE: Consider setting a value per origin chain or connection
            if remaining_time > 40 {
                return true;
            }
            if rx.changed().await.is_err() {
                self.log("🤔🤔🤔 timestamp fetch failed");
                return true;
            }
        }
    }

    fn update_l1_block_info(&'static self) {
        if self.is_op_stack {
            tokio::spawn(async move {
                match self.l1_fee_oracle {
                    Some(l1_fee_oracle) => l1_fee_oracle.update_l1_block_info().await,
                    None => (),
                }
            });
        }
    }

    fn update_l1_gas_limit(&'static self) {
        if self.is_arbitrum {
            tokio::spawn(async move {
                match self.l1_gas_limit {
                    Some(l1_gas_limit) => l1_gas_limit.update_l1_gas_limit_buffer().await,
                    None => (),
                }
            });
        }
    }

    fn update_l1_gas_limit_eth_transfer(&'static self) {
        if self.is_arbitrum && self.use_kairos() {
            tokio::spawn(async move {
                match self.l1_gas_limit {
                    Some(l1_gas_limit) => {
                        l1_gas_limit.update_l1_gas_limit_buffer_eth_transfer().await
                    }
                    None => (),
                }
            });
        }
    }

    pub async fn block_timestamp_updater(&'static self) {
        fixed_interval_loop(
            &to_millis(&self.start_block.header.timestamp),
            self.block_interval,
            self,
            process_update_block_timestamp,
            &0,
        )
        .await;
    }

    fn update_block_timestamp(&'static self, next_timestamp: &u64) {
        self.propagate_block_timestamp(*next_timestamp);
    }

    pub async fn gas_updater(&'static self) {
        fixed_interval_loop_async(
            &(to_millis(&self.start_block.header.timestamp) + 50),
            self.gas_update_interval,
            self,
            process_update_gas,
        )
        .await;
    }

    async fn update_gas(&'static self) {
        self.update_l1_block_info();
        self.update_l1_gas_limit();

        match get_gas_price(&self.http_provider).await {
            Ok(base_fee) => self.update_base_fee_per_gas(base_fee),
            _ => self.log("Error fetching gas price"),
        }

        self.update_l1_gas_limit_eth_transfer();
    }

    async fn subscribe_logs(
        &'static self,
        local_id: &Mutex<B256>,
        filter: &Filter,
    ) -> Result<Subscription<Log>, Box<dyn std::error::Error + Send + Sync>> {
        let wss_provider = (*self.wss_provider.read().unwrap()).clone();
        return subscribe_logs(&wss_provider, local_id, filter).await;
    }

    pub async fn listen_token_transfers_in(&'static self) -> Subscription<Log> {
        // Create a filter to watch for spoke pool deposits
        let deposit_filter = self
            .token
            .Transfer_filter()
            .topic2(*self.relayer_bytes32)
            .from_block(BlockNumberOrTag::Latest);

        return self
            .subscribe_logs(&self.balance_in_subscription, &deposit_filter.filter)
            .await
            .unwrap();
    }

    pub async fn listen_token_transfers_out(&'static self) -> Subscription<Log> {
        // Create a filter to watch for spoke pool deposits
        let deposit_filter = self
            .token
            .Transfer_filter()
            .topic1(*self.relayer_bytes32)
            .from_block(BlockNumberOrTag::Latest);

        return self
            .subscribe_logs(&self.balance_out_subscription, &deposit_filter.filter)
            .await
            .unwrap();
    }

    fn increase_balance(&'static self, diff: U256) {
        let mut balance = self.balance.write().unwrap();
        let new_balance = *balance + diff;
        *balance = new_balance;
        drop(balance);
        if new_balance >= *self.skip_non_exclusive_threshold {
            *self.skip_non_exclusive.write().unwrap() = false;
        }
    }

    fn decrease_balance(&'static self, diff: U256) {
        let mut balance = self.balance.write().unwrap();
        let new_balance = *balance - diff;
        *balance = new_balance;
        drop(balance);
        if new_balance < *self.skip_non_exclusive_threshold {
            *self.skip_non_exclusive.write().unwrap() = true;
        }
    }

    pub async fn balance_in_updater(&'static self) {
        // Subscribe to logs.
        let transfer_in_sub = self.listen_token_transfers_in().await;
        let mut transfer_in_stream = transfer_in_sub.into_stream();

        while let Some(log) = transfer_in_stream.next().await {
            tokio::spawn(async move {
                let data = log.data();
                let transfer = IERC20::Transfer::decode_log_data(data).unwrap();
                let amount = transfer.value;

                self.increase_balance(amount);
            });
        }
    }

    pub async fn balance_out_updater(&'static self) {
        // Subscribe to logs.
        let transfer_out_sub = self.listen_token_transfers_out().await;
        let mut transfer_out_stream = transfer_out_sub.into_stream();

        while let Some(log) = transfer_out_stream.next().await {
            tokio::spawn(async move {
                let data = log.data();
                let transfer = IERC20::Transfer::decode_log_data(data).unwrap();
                let amount = transfer.value;

                self.decrease_balance(amount);
            });
        }
    }

    pub async fn listen_spoke_pool_fills(
        &'static self,
        origin_chain: &'static ChainConfig,
        wss_provider: ChainProvider,
        local_id: Arc<Mutex<B256>>,
    ) -> Result<Subscription<Log>, Box<dyn std::error::Error + Send + Sync>> {
        // Create a filter to watch for spoke pool deposits
        let deposit_filter = self
            .contract_instance
            .FilledRelay_filter()
            .topic1(*origin_chain.chain_id)
            .from_block(BlockNumberOrTag::Latest);

        // Subscribe to logs.
        return subscribe_logs(&wss_provider, &local_id, &deposit_filter.filter).await;
    }

    pub async fn listen_spoke_pool_deposits(
        &'static self,
        dest_chain: &'static ChainConfig,
        wss_provider: ChainProvider,
        local_id: Arc<Mutex<B256>>,
    ) -> Result<Subscription<Log>, Box<dyn std::error::Error + Send + Sync>> {
        // Create a filter to watch for spoke pool deposits
        let deposit_filter = self
            .contract_instance
            .FundsDeposited_filter()
            .topic1(*dest_chain.chain_id)
            .from_block(BlockNumberOrTag::Latest);

        // Subscribe to logs.
        return subscribe_logs(&wss_provider, &local_id, &deposit_filter.filter).await;
    }

    pub async fn estimate_gas(&'static self, tx: &TransactionRequest) -> Option<u64> {
        let estimation_tx = tx.clone().with_gas_limit(20_000_000_u64);
        match self.http_provider.estimate_gas(estimation_tx).await {
            Ok(gas) => Some(gas),
            Err(e) => {
                self.log(format!("Error estimating gas: {:?}", e).as_str());
                return None;
            }
        }
    }

    async fn reset_nonce(&'static self) {
        let updated_nonce = self
            .http_provider
            .get_transaction_count(*self.relayer_address)
            .await
            .unwrap();
        *self.nonce.lock().await = updated_nonce;
    }

    async fn reset_kairos_briber_nonce(&'static self) {
        let updated_kairos_briber_nonce = self
            .http_provider
            .get_transaction_count(*self.kairos_briber_address)
            .await
            .unwrap();
        *self.kairos_briber_nonce.lock().await = updated_kairos_briber_nonce;
    }

    pub fn build_tx_calldata(
        &'static self,
        decoded_data: &IV3SpokePool::FundsDeposited,
        output_token: &'static B256,
    ) -> Vec<u8> {
        build_v3_spoke_pool_calldata(
            self.chain_id,
            decoded_data,
            output_token,
            self.relayer_bytes32,
        )
    }

    pub fn build_tx(
        &'static self,
        origin_chain: &'static ChainConfig,
        decoded_data: &IV3SpokePool::FundsDeposited,
        output_token: &'static B256,
    ) -> TransactionRequest {
        let calldata: Vec<u8> = origin_chain.build_tx_calldata(decoded_data, output_token);
        let base_fee = *self.base_fee.read().unwrap() * uint!(2_U256); // Double this for gas estimation

        return TransactionRequest::default()
            .with_input(calldata)
            .with_from(*origin_chain.relayer_address)
            .with_to(*self.contract_address)
            .with_chain_id(*self.chain_id_u64)
            .with_gas_limit(*self.gas_limit)
            .with_gas_price(base_fee.try_into().unwrap())
            .with_nonce(1_000_000_000); // Use a mock nonce for calculating the tx cost
    }

    pub async fn build_tx_envelope(
        &'static self,
        tx: &TransactionRequest,
        gas_price: &U256,
        gas_limit: &u64,
        nonce: &u64,
    ) -> EthereumTxEnvelope<TxEip4844Variant> {
        let submit_gas_limit = self.get_submit_gas_limit(gas_limit);
        let tx = tx
            .clone()
            .with_gas_price(gas_price.try_into().unwrap())
            .with_gas_limit(submit_gas_limit)
            .with_nonce(*nonce);

        // Build and sign the transaction using the `EthereumWallet` with the provided wallet.
        return tx.build(&self.signer).await.unwrap();
    }

    fn get_submit_gas_limit(&'static self, gas_limit: &u64) -> u64 {
        if *gas_limit > *self.gas_used {
            return self.gas_limit + (gas_limit - self.gas_used);
        } else {
            return *self.gas_limit;
        }
    }

    pub async fn build_bribe_kairos_tx_envelope(
        &'static self,
        gas_price: &U256,
    ) -> EthereumTxEnvelope<TxEip4844Variant> {
        let mut nonce = self.kairos_briber_nonce.lock().await;

        // 21k + arb gas limit buffer
        let l1_gas_limit_buffer_eth_transfer = 21_000 + self.get_l1_gas_limit_buffer_eth_transfer();

        let tx = TransactionRequest::default()
            .with_from(*self.kairos_briber_address)
            .with_to(KAIROS_PAYMENT_ADDRESS)
            .with_gas_price(gas_price.try_into().unwrap())
            .with_gas_limit(l1_gas_limit_buffer_eth_transfer)
            .with_nonce(*nonce)
            .with_value(uint!(2_U256));
        *nonce += 1;
        drop(nonce);

        // Build and sign the transaction using the `EthereumWallet` with the provided wallet.
        return tx.build(&self.kairos_signer).await.unwrap();
    }

    pub async fn decrease_kairos_briber_nonce(&'static self) {
        let mut nonce = self.kairos_briber_nonce.lock().await;
        *nonce += 1;
        drop(nonce);
    }

    pub fn tx_too_old(&'static self, deposit_timestamp: &u64, next_timestamp: &u64) -> bool {
        // Drop deposits that are older than 120s
        *next_timestamp > (*deposit_timestamp + 120)
    }

    pub fn not_enough_balance(&'static self, output_amount: &U256) -> bool {
        let balance = *self.balance.read().unwrap();
        // Check if output amount is too large
        *output_amount > balance
    }

    pub async fn skip_deposit(
        &'static self,
        decoded_data: &IV3SpokePool::FundsDeposited,
        tx: &TransactionRequest,
        gross_profit: &U256,
        cached_gas_estimate: &u64,
        is_exclusive: &bool,
        next_timestamp: &u64,
        chain_base_fee: &U256,
        l1_gas_limit_buffer: &u64,
    ) -> Option<(U256, u64)> {
        if !(*is_exclusive) && decoded_data.exclusiveRelayer != B256::ZERO {
            let exclusive_deadline = decoded_data.exclusivityDeadline as u64;

            if *next_timestamp <= exclusive_deadline {
                self.log_skip_reason(
                    decoded_data,
                    format!(
                        "🧱 Waiting for exclusive_relayer {:?} (curr: {:?}, deadline: {:?})",
                        exclusive_deadline - next_timestamp,
                        next_timestamp,
                        exclusive_deadline
                    )
                    .as_str(),
                );
                return None;
            }
        }

        let gas_estimate;

        // Check if message is empty
        if *cached_gas_estimate == 0 {
            if decoded_data.message.len() == 0 {
                if self.supports_flashbots {
                    let balance = self
                        .http_provider
                        .get_balance(Address::from_word(decoded_data.recipient))
                        .await
                        .unwrap();

                    gas_estimate = match balance {
                        U256::ZERO => *self.gas_used,
                        _ => GAS_USED_STANDARD,
                    }
                } else {
                    gas_estimate = *self.gas_used;
                }
            } else {
                let estimate = self.estimate_gas(&tx).await;

                if estimate.is_none() {
                    self.log_skip_reason(decoded_data, format!("📐 Gas estimate failed").as_str());
                    return None;
                }
                gas_estimate = estimate.unwrap();
            }
        } else {
            gas_estimate = *cached_gas_estimate;
        }

        let l1_fee = self.get_l1_fee(tx);
        if *gross_profit < l1_fee {
            self.log_skip_reason(
                decoded_data,
                format!(
                    "⛽ profit less than l1_fee for {:?}: {:?} less than {:?}. Diff: {:?}",
                    decoded_data.outputAmount,
                    gross_profit,
                    l1_fee,
                    l1_fee - gross_profit
                )
                .as_str(),
            );
            return None;
        }
        let profit = gross_profit - l1_fee;

        let gas_estimate_u256 = U256::from(gas_estimate + *l1_gas_limit_buffer);
        let max_gas_price = profit / gas_estimate_u256;

        if *chain_base_fee > max_gas_price {
            self.log_skip_reason(
                decoded_data,
                format!(
                    "🦥 gas price too low for chain: {:?} less than {:?}. Diff: {:?}",
                    max_gas_price,
                    chain_base_fee,
                    chain_base_fee - max_gas_price
                )
                .as_str(),
            );
            return None;
        }

        return Some((max_gas_price, gas_estimate));
    }

    pub async fn add_fill_to_queue(&'static self, fill: Arc<Fill>) {
        self.fills_queued.lock().await.push(fill);
    }

    pub async fn tx_submit_updater(&'static self) {
        let close_to_next_block_timestamp =
            to_millis(&self.start_block.header.timestamp) - *self.tx_process_ms_interval;

        fixed_interval_loop(
            &close_to_next_block_timestamp,
            self.block_interval,
            self,
            process_new_fills,
            self.tx_process_ms_interval,
        )
        .await;
    }

    async fn sort_fills(
        &'static self,
        next_timestamp: &u64,
        chain_base_fee: &alloy::primitives::Uint<256, 4>,
        l1_gas_limit_buffer: &u64,
        fills_queued: &tokio::sync::MutexGuard<'_, Vec<Arc<Fill>>>,
        tracker: &TaskTracker,
        order_tasks: &mut FuturesUnordered<JoinHandle<(Arc<Fill>, bool)>>,
        ordered_fills: &mut BinaryHeap<Arc<Fill>>,
        fills_to_keep: &mut Vec<Arc<Fill>>,
    ) {
        // Timeout to continue and submit any ready tx
        let deadline = Instant::now() + *self.tx_estimate_limit;

        let candidate_fills = (*fills_queued).clone();
        for fill in candidate_fills {
            let order_task = tracker_tokio_spawn(
                tracker,
                fill.skip_deposit(*next_timestamp, *chain_base_fee, *l1_gas_limit_buffer),
            );
            order_tasks.push(order_task);
        }

        loop {
            let now = Instant::now();
            if now >= deadline {
                break;
            }
            // Time left before deadline
            let remaining = deadline - now;

            // Await next completed task or timeout
            match tokio::time::timeout(remaining, order_tasks.next()).await {
                Ok(Some(join_res)) => {
                    if let Ok(value) = join_res {
                        let (fill, skip_order) = value;

                        if fill.drop_deposit(next_timestamp) {
                            self.log_fill(&fill, format!("💔 ~ Dropping tx").as_str());
                        } else if skip_order {
                            fills_to_keep.push(Arc::clone(&fill));
                        } else {
                            ordered_fills.push(fill);
                        }
                    } else {
                        eprintln!("A tx order failed");
                    }
                }
                _ => break, // Either timeout or no more tasks
            }
        }
    }

    async fn process_fills(
        &'static self,
        next_timestamp: &u64,
        chain_base_fee: &U256,
        l1_gas_limit_buffer: &u64,
    ) {
        let mut fills_queued = self.fills_queued.lock().await;
        let queued_size = fills_queued.len();
        if queued_size == 0 {
            return;
        }

        let tracker = TaskTracker::new();

        let txs_failed = self
            .sort_and_submit_fills(
                next_timestamp,
                chain_base_fee,
                l1_gas_limit_buffer,
                &mut fills_queued,
                &queued_size,
                &tracker,
            )
            .await;
        drop(fills_queued);

        // Close the tracker after all txs are submitted.
        tracker.close();
        // Wait for everything to finish. (Gas estimates may have taken longer than allowed delay)
        tracker.wait().await;

        if txs_failed {
            // Wait for the next block and then update nonce with onchain value
            tokio::time::sleep(tokio::time::Duration::from_millis(
                *self.tx_process_ms_interval,
            ))
            .await;
            self.reset_nonce().await;
        }
    }

    async fn sort_and_submit_fills(
        &'static self,
        next_timestamp: &u64,
        chain_base_fee: &U256,
        l1_gas_limit_buffer: &u64,
        fills_queued: &mut tokio::sync::MutexGuard<'_, Vec<Arc<Fill>>>,
        queued_size: &usize,
        tracker: &TaskTracker,
    ) -> bool {
        let mut order_tasks: FuturesUnordered<JoinHandle<(Arc<Fill>, bool)>> =
            FuturesUnordered::new();
        // Fills are ordered by gas_price, making sure higher gas paid txs have a lower nonce
        let mut ordered_fills: BinaryHeap<Arc<Fill>> = BinaryHeap::new();
        let mut fills_to_keep: Vec<Arc<Fill>> = Vec::new();

        self.sort_fills(
            next_timestamp,
            chain_base_fee,
            l1_gas_limit_buffer,
            fills_queued,
            tracker,
            &mut order_tasks,
            &mut ordered_fills,
            &mut fills_to_keep,
        )
        .await;

        let mut nonce = self.nonce.lock().await;
        let txs_failed = self
            .submit_fills(tracker, ordered_fills, &mut fills_to_keep, &mut nonce)
            .await;

        // Wait until the first task ends or fails
        while let Some(result) = order_tasks.next().await {
            match result {
                Ok(value) => {
                    let (fill, _) = value;
                    fills_to_keep.push(Arc::clone(&fill));
                }
                _ => eprintln!("A late tx order failed"),
            }
        }
        drop(nonce);

        self.log(
            format!(
                "Submitted or dropped: {:?} out of {:?} {:?}",
                queued_size - fills_to_keep.len(),
                queued_size,
                txs_failed
            )
            .as_str(),
        );

        **fills_queued = fills_to_keep;

        txs_failed
    }

    async fn submit_fills(
        &'static self,
        tracker: &TaskTracker,
        mut ordered_fills: BinaryHeap<Arc<Fill>>,
        fills_to_keep: &mut Vec<Arc<Fill>>,
        nonce: &mut tokio::sync::MutexGuard<'_, u64>,
    ) -> bool {
        let mut submit_tasks = FuturesUnordered::new();

        let mut tx_nonce = **nonce;

        // Submit higher gas price txs first
        while let Some(fill) = ordered_fills.pop() {
            let submit_task = tracker_tokio_spawn(tracker, fill.submit(tx_nonce));
            submit_tasks.push(submit_task);
            tx_nonce = tx_nonce + 1;
        }

        let mut txs_failed = false;

        while let Some(result) = submit_tasks.next().await {
            match result {
                Ok(value) => {
                    let (fill, submit_succesfull) = value;
                    if submit_succesfull {
                        /*
                         * TODO: Improve tx and nonce management:
                         * Only drop tx after tx was submitted. (Consider removing this check and always keep txs)
                         * because we might want to re-submit the tx above with higher nonce.
                         *
                         * Ideally we try to submit valid fills every block until they are filled.
                         *
                         * NOTE: This needs to be changed to be used on eth mainnet if using flashbots.
                         */
                        **nonce += 1;
                    } else {
                        txs_failed = true;
                        fills_to_keep.push(Arc::clone(&fill));
                    }
                }
                _ => eprintln!("A tx submission failed"),
            }
        }

        txs_failed
    }
}

async fn process_kairos_auction_stream(chain: &'static ChainConfig, next_timestamp: u64) {
    chain.kairos_auction_stream(&next_timestamp).await;
}

fn process_update_block_timestamp(chain: &'static ChainConfig, next_timestamp: u64) {
    chain.update_block_timestamp(&next_timestamp);
}

async fn process_update_gas(chain: &'static ChainConfig, _: u64) {
    chain.update_gas().await;
}

fn process_new_fills(chain: &'static ChainConfig, next_timestamp: u64) {
    tokio::spawn(async move {
        let chain_base_fee = *chain.base_fee.read().unwrap();
        chain
            .process_fills(
                &next_timestamp,
                &chain_base_fee,
                &chain.get_l1_gas_limit_buffer(),
            )
            .await;
    });
}
