use crate::configs::chain_config::GAS_USED_STANDARD;

use super::{
    super::types::providers::{ChainProvider, SubmittedTxEnvelope},
    super::utils::{
        constants::DIVISOR,
        fill::Fill,
        http::{
            alchemy::alchemy_simulate, kairos_stats::kairos_order_info,
            kairos_submit::kairos_send_tx_in_bundle,
        },
        tokio_spawn_helper::tracker_tokio_spawn_with_cancel_token,
        v3_spoke_pool::IV3SpokePool,
    },
    chain_config::ChainConfig,
    fill_config::FillConfig,
};
use alloy::{
    consensus::{EthereumTxEnvelope, TxEip4844Variant},
    hex,
    primitives::{ruint::uint, Address, B256, U256},
    providers::Provider,
    pubsub::Subscription,
    rpc::types::{Log, TransactionRequest},
    sol_types::SolEvent,
};
use dashmap::DashSet;
use futures::{stream::FuturesUnordered, StreamExt};
use std::{
    future::Future,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::{sync::Mutex, task::JoinHandle};
use tokio_util::{sync::CancellationToken, task::TaskTracker};

async fn listen_for<F, Fut>(
    subscribe: F,
    chain_to_sub: &'static ChainConfig,
    chain_to_filter: &'static ChainConfig,
    wss_provider: ChainProvider,
    sub_id: Arc<Mutex<B256>>,
) -> Result<Subscription<Log>, Box<dyn std::error::Error + Send + Sync>>
where
    F: FnOnce(&'static ChainConfig, &'static ChainConfig, ChainProvider, Arc<Mutex<B256>>) -> Fut,
    Fut: Future<Output = Result<Subscription<Log>, Box<dyn std::error::Error + Send + Sync>>>,
{
    return Ok(subscribe(chain_to_sub, chain_to_filter, wss_provider, sub_id).await?);
}

pub fn is_exclusive_char(
    exclusive_relayer: &B256,
    relayer_bytes32: &'static B256,
) -> &'static char {
    if *exclusive_relayer == *relayer_bytes32 {
        &'🔥'
    } else {
        &' '
    }
}

// decrease gas price by 10% for exclusive fills
const GAS_PRICE_REDUCTION_FROM_EXCLUSIVE: U256 = uint!(9_000_000_U256);

pub struct ConnectionConfig {
    origin_chain: &'static ChainConfig,
    dest_chain: &'static ChainConfig,
    fill_config: &'static FillConfig,
    ids_filled: DashSet<String>,
    ids_found: DashSet<String>,

    deposit_subscription: Arc<Mutex<B256>>,
    fill_subscription: Arc<Mutex<B256>>,

    fallback_deposit_subscription: Arc<Mutex<B256>>,
    fallback_fill_subscription: Arc<Mutex<B256>>,
}

impl ConnectionConfig {
    pub fn new(
        origin_chain: &'static ChainConfig,
        dest_chain: &'static ChainConfig,
        fill_config: &'static FillConfig,
    ) -> Self {
        Self {
            origin_chain,
            dest_chain,
            fill_config,
            ids_filled: DashSet::new(),
            ids_found: DashSet::new(),

            deposit_subscription: Arc::new(Mutex::new(B256::ZERO)),
            fill_subscription: Arc::new(Mutex::new(B256::ZERO)),

            fallback_deposit_subscription: Arc::new(Mutex::new(B256::ZERO)),
            fallback_fill_subscription: Arc::new(Mutex::new(B256::ZERO)),
        }
    }

    pub fn simulate(&'static self) -> bool {
        return self.fill_config.simulate;
    }

    pub fn relayer_bytes32(&'static self) -> &'static B256 {
        return self.dest_chain.relayer_bytes32;
    }

    pub fn log(&'static self, message: &str) {
        self.origin_chain
            .log(format!("to {}: {}", self.dest_chain.chain_id_u64, message,).as_str());
    }

    fn is_exclusive_char(&'static self, exclusive_relayer: &B256) -> &'static char {
        is_exclusive_char(exclusive_relayer, self.relayer_bytes32())
    }

    fn log_skip_reason(&'static self, decoded_data: &IV3SpokePool::FundsDeposited, skip: &str) {
        self.log(format!("{} Deposit skipped: {}", decoded_data.depositId, skip).as_str());
    }

    pub async fn unsub_self_subscriptions(&'static self) {
        self.origin_chain
            .unsub_subscription(&self.deposit_subscription)
            .await;
        self.dest_chain
            .unsub_subscription(&self.fill_subscription)
            .await;
    }

    fn insert_filled_id(&'static self, deposit_id: &String) -> bool {
        self.ids_filled.insert(deposit_id.clone())
    }

    fn insert_found_id(&'static self, deposit_id: &String) -> bool {
        self.ids_found.insert(deposit_id.clone())
    }

    pub fn already_filled(&'static self, deposit_id: &String) -> bool {
        return self.ids_filled.contains(deposit_id);
    }

    pub async fn listen(
        &'static self,
        tracker: TaskTracker,
        token: CancellationToken,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.log("Listening for fills");

        let mut tasks = Vec::new();

        let fills_task_0 = tracker_tokio_spawn_with_cancel_token(
            &tracker,
            self.listen_for_fills(
                ChainConfig::listen_spoke_pool_fills,
                (*self.dest_chain.wss_provider.read().unwrap()).clone(),
                self.fill_subscription.clone(),
            ),
            &token,
        );
        let fills_task_1 = tracker_tokio_spawn_with_cancel_token(
            &tracker,
            self.listen_for_fills(
                ChainConfig::listen_spoke_pool_fills,
                (*self.dest_chain.wss_fallback.read().unwrap()).clone(),
                self.fallback_fill_subscription.clone(),
            ),
            &token,
        );

        let deposits_task_0 = tracker_tokio_spawn_with_cancel_token(
            &tracker,
            self.listen_for_deposits(
                ChainConfig::listen_spoke_pool_deposits,
                (*self.origin_chain.wss_provider.read().unwrap()).clone(),
                self.deposit_subscription.clone(),
            ),
            &token,
        );
        let deposits_task_1 = tracker_tokio_spawn_with_cancel_token(
            &tracker,
            self.listen_for_deposits(
                ChainConfig::listen_spoke_pool_deposits,
                (*self.origin_chain.wss_fallback.read().unwrap()).clone(),
                self.fallback_deposit_subscription.clone(),
            ),
            &token,
        );

        tasks.push(fills_task_0);
        tasks.push(fills_task_1);
        tasks.push(deposits_task_0);
        tasks.push(deposits_task_1);

        let mut futs = tasks
            .into_iter()
            // The _ is inferred to be the return type of `compute_value`
            .collect::<FuturesUnordered<JoinHandle<_>>>();

        // Wait until the first task ends or fails
        while let Some(result) = futs.next().await {
            eprintln!("A listen task terminated earlier: {:?}", result);
            break;
        }

        Ok(())
    }

    async fn listen_for_fills<F, Fut>(
        &'static self,
        subscribe: F,
        wss_provider: ChainProvider,
        sub_id: Arc<Mutex<B256>>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
    where
        F: FnOnce(
            &'static ChainConfig,
            &'static ChainConfig,
            ChainProvider,
            Arc<Mutex<B256>>,
        ) -> Fut,
        Fut: Future<Output = Result<Subscription<Log>, Box<dyn std::error::Error + Send + Sync>>>,
    {
        let sub = listen_for(
            subscribe,
            self.dest_chain,
            self.origin_chain,
            wss_provider,
            sub_id,
        )
        .await?;
        let mut stream = sub.into_stream();
        while let Some(log) = stream.next().await {
            tokio::spawn(async move {
                let _ = self.process_fill(&log).await;
            });
        }
        Ok(())
    }

    async fn process_fill(
        &'static self,
        log: &Log,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let topics = log.topics();
        let deposit_id: U256 = topics.get(2).unwrap().to_string().parse().unwrap();
        let deposit_id: &String = &deposit_id.to_string();
        if !self.insert_filled_id(deposit_id) {
            return Ok(());
        }

        let relayer = *topics.get(3).unwrap();
        let filled_by_this_relayer = relayer == *self.relayer_bytes32();

        self.print_filled_id(deposit_id, &relayer, &filled_by_this_relayer, log.data());

        if filled_by_this_relayer && self.dest_chain.supports_flashbots {
            self.increase_nonce().await;
        }

        Ok(())
    }

    fn print_filled_id(
        &'static self,
        deposit_id: &String,
        relayer: &B256,
        filled_by_this_relayer: &bool,
        data: &alloy::primitives::LogData,
    ) {
        let decoded_data = IV3SpokePool::FilledRelay::decode_log_data(data).unwrap();

        if self
            .fill_config
            .is_valid_input_token(&decoded_data.inputToken)
        {
            return;
        }

        let filled_emoji = match filled_by_this_relayer {
            true => " 💲",
            false => " 💀",
        };
        let relayer = relayer.to_string();
        let other_relayer = match filled_by_this_relayer {
            true => "",
            false => relayer.as_ref(),
        };

        let exclusive_emoji = match decoded_data.exclusiveRelayer == *self.relayer_bytes32() {
            true => "🔥",
            false => "",
        };

        let msg = " Deposit filled ";
        self.log(
            (deposit_id.to_owned() + filled_emoji + exclusive_emoji + msg + other_relayer).as_str(),
        );
    }

    async fn increase_nonce(&'static self) {
        let mut nonce = self.dest_chain.nonce.lock().await;
        *nonce += 1; // Increment the nonce
        drop(nonce);
    }

    async fn listen_for_deposits<F, Fut>(
        &'static self,
        subscribe: F,
        wss_provider: ChainProvider,
        sub_id: Arc<Mutex<B256>>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
    where
        F: FnOnce(
            &'static ChainConfig,
            &'static ChainConfig,
            ChainProvider,
            Arc<Mutex<B256>>,
        ) -> Fut,
        Fut: Future<Output = Result<Subscription<Log>, Box<dyn std::error::Error + Send + Sync>>>,
    {
        let sub = listen_for(
            subscribe,
            self.origin_chain,
            self.dest_chain,
            wss_provider,
            sub_id,
        )
        .await?;

        let mut stream = sub.into_stream();
        while let Some(log) = stream.next().await {
            tokio::spawn(async move {
                if self.dest_chain.is_arbitrum {
                    let _ = self.process_deposit_old(&log).await;
                } else {
                    let _ = self.process_deposit(&log).await;
                }
            });
        }
        Ok(())
    }

    pub async fn process_deposit(
        &'static self,
        log: &Log,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let data = log.data();
        let decoded_data = IV3SpokePool::FundsDeposited::decode_log_data(data).unwrap();

        // Skip log if it is already being processed
        let deposit_id = &decoded_data.depositId.to_string();
        /*
         * TODO: IMPORTANT - This is not suitable for eth mainnet.
         * If there is a re-rorg we are skipping the correct deposit!
         */
        if !self.insert_found_id(deposit_id) {
            return Ok(());
        }

        let fill: Option<Fill> = self.parse_deposit(&decoded_data);

        self.add_fill_to_queue(deposit_id, fill).await;

        Ok(())
    }

    async fn add_fill_to_queue(&'static self, deposit_id: &String, fill: Option<Fill>) {
        if !fill.is_none() {
            let fill = fill.unwrap();
            self.dest_chain.add_fill_to_queue(Arc::new(fill)).await;

            self.log(format!("⚒️  Received fill {:?}", deposit_id).as_str());
        }
    }

    async fn process_deposit_old(
        &'static self,
        log: &Log,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let data = log.data();
        let decoded_data = IV3SpokePool::FundsDeposited::decode_log_data(data).unwrap();

        // Skip log if it is already being processed
        let deposit_id = &decoded_data.depositId.to_string();
        /*
         * TODO: IMPORTANT - This is not suitable for eth mainnet.
         * If there is a re-rorg we are skipping the correct deposit!
         */
        if !self.insert_found_id(deposit_id) {
            return Ok(());
        }

        let tx: TransactionRequest = self.build_tx(&decoded_data);

        let gas_info = self.check_conditions(&decoded_data, &tx).await;
        // Early returns if conditions are not met
        if gas_info.is_none() {
            if decoded_data.exclusiveRelayer == *self.relayer_bytes32() {
                self.log(format!("{:?} 🔥💔💔 Was exclusive fill", deposit_id).as_str());
            }
            return Ok(());
        }
        let (gas_price, gas_limit) = gas_info.unwrap();

        let mut nonce = self.dest_chain.nonce.lock().await;

        let tx_envelope = self
            .build_tx_envelope(&tx, &gas_price, &gas_limit, &nonce)
            .await;

        if self.already_filled(deposit_id) {
            self.log_skip_reason(&decoded_data, format!("💔 already filled").as_str());
            if decoded_data.exclusiveRelayer == *self.relayer_bytes32() {
                self.log(format!("{:?} 🔥💔 Was exclusive fill", deposit_id).as_str());
            }
            return Ok(());
        }

        if !self.dest_chain.supports_flashbots {
            *nonce += 1; // Increment the nonce
        }
        drop(nonce);

        if self.fill_config.simulate {
            self.simulate_fill(&decoded_data, deposit_id).await;
        } else {
            let use_kairos = self.dest_chain.use_kairos();
            if use_kairos {
                let tx_envelope = tx_envelope.clone();
                let deposit_id = deposit_id.clone();
                tokio::spawn(async move {
                    let kairos_bribe_tx_envelope = self
                        .dest_chain
                        .build_bribe_kairos_tx_envelope(&gas_price)
                        .await;

                    let call = kairos_send_tx_in_bundle(tx_envelope, kairos_bribe_tx_envelope);

                    let mut rollback_nonce = false;
                    let kairos_uuid;

                    match call.await {
                        Ok(pending_tx) => {
                            kairos_uuid = Some(pending_tx);
                        }
                        Err(e) => {
                            kairos_uuid = None;
                            rollback_nonce = true;

                            self.log(
                                format!(
                                    "{:?} {:?}🐛 Error sending kairos tx: {:?}",
                                    deposit_id,
                                    self.is_exclusive_char(&decoded_data.exclusiveRelayer),
                                    e
                                )
                                .as_str(),
                            );
                        }
                    };

                    if !rollback_nonce && kairos_uuid.is_some() {
                        // Wait for next 2 blocks to see if tx was submitted
                        tokio::time::sleep(
                            *self.dest_chain.block_interval + *self.dest_chain.block_interval,
                        )
                        .await;

                        let kairos_uuid = kairos_uuid.clone().unwrap();
                        let success = kairos_order_info(&kairos_uuid).await;
                        if success.is_err() {
                            rollback_nonce = true;
                        } else {
                            let success = success.unwrap();

                            rollback_nonce = !success;
                        }
                    }

                    if rollback_nonce {
                        self.dest_chain.decrease_kairos_briber_nonce().await;

                        self.log(
                            format!(
                                "{:?} {:?}🐛🐨 Kairos did not submit onchain. {:?}",
                                deposit_id,
                                self.is_exclusive_char(&decoded_data.exclusiveRelayer),
                                kairos_uuid
                            )
                            .as_str(),
                        );
                    } else {
                        self.log(
                            format!(
                                "{:?} {:?}⛺ Filled with Kairos for revenue {:?}: {:?} {:?}",
                                deposit_id,
                                self.is_exclusive_char(&decoded_data.exclusiveRelayer),
                                decoded_data.inputAmount - decoded_data.outputAmount,
                                gas_price,
                                kairos_uuid.unwrap()
                            )
                            .as_str(),
                        );
                    }
                });
            }

            // Send the raw transaction and retrieve the transaction receipt.
            // [Provider::send_tx_envelope] is a convenience method that encodes the transaction using
            // EIP-2718 encoding and broadcasts it to the network using [Provider::send_raw_transaction].
            let call = self.send_tx_envelope(tx_envelope);

            match call.await {
                Ok(pending_tx) => {
                    self.log(
                        format!(
                            "{:?} {:?}⛺ Filled for revenue {:?}: {:?} {:?}",
                            deposit_id,
                            self.is_exclusive_char(&decoded_data.exclusiveRelayer),
                            decoded_data.inputAmount - decoded_data.outputAmount,
                            gas_price,
                            pending_tx.tx_hash()
                        )
                        .as_str(),
                    );
                }
                Err(e) => {
                    self.log(
                        format!(
                            "{:?} {:?}🐛 Error sending first tx: {:?}",
                            deposit_id,
                            self.is_exclusive_char(&decoded_data.exclusiveRelayer),
                            e
                        )
                        .as_str(),
                    );

                    if use_kairos {
                        // If we submitted through kairos, it should have failed with `nonce too low``
                        return Ok(());
                    }

                    let updated_nonce = self
                        .dest_chain
                        .http_provider
                        .get_transaction_count(*self.dest_chain.relayer_address)
                        .await
                        .unwrap();
                    let resubmit_tx_envelope = self
                        .build_tx_envelope(&tx, &gas_price, &gas_limit, &updated_nonce)
                        .await;

                    let call = self.send_tx_envelope(resubmit_tx_envelope);

                    match call.await {
                        Ok(pending_tx) => {
                            self.log(
                                format!(
                                    "{:?} {:?}⛺ Filled for revenue {:?}: {:?}",
                                    deposit_id,
                                    self.is_exclusive_char(&decoded_data.exclusiveRelayer),
                                    decoded_data.inputAmount - decoded_data.outputAmount,
                                    pending_tx.tx_hash()
                                )
                                .as_str(),
                            );
                        }
                        Err(e) => {
                            self.log(
                                format!(
                                    "{:?} {:?}🐛 Error sending tx: {:?}",
                                    deposit_id,
                                    self.is_exclusive_char(&decoded_data.exclusiveRelayer),
                                    e
                                )
                                .as_str(),
                            );
                        }
                    };

                    // Wait 12 seconds and then update nonce
                    tokio::time::sleep(tokio::time::Duration::from_secs(12)).await;
                    let updated_nonce = self
                        .dest_chain
                        .http_provider
                        .get_transaction_count(*self.dest_chain.relayer_address)
                        .await
                        .unwrap();
                    let mut nonce = self.dest_chain.nonce.lock().await;
                    *nonce = updated_nonce;
                    drop(nonce);
                }
            };
        }

        Ok(())
    }

    fn build_tx_calldata(&'static self, decoded_data: &IV3SpokePool::FundsDeposited) -> Vec<u8> {
        self.origin_chain
            .build_tx_calldata(decoded_data, self.fill_config.allowed_output_token)
    }

    pub fn build_tx(
        &'static self,
        decoded_data: &IV3SpokePool::FundsDeposited,
    ) -> TransactionRequest {
        self.dest_chain.build_tx(
            self.origin_chain,
            decoded_data,
            self.fill_config.allowed_output_token,
        )
    }

    pub async fn build_tx_envelope(
        &'static self,
        tx: &TransactionRequest,
        gas_price: &U256,
        gas_limit: &u64,
        nonce: &u64,
    ) -> EthereumTxEnvelope<TxEip4844Variant> {
        self.dest_chain
            .build_tx_envelope(tx, gas_price, gas_limit, nonce)
            .await
    }

    fn amount_too_small(&'static self, decoded_data: &IV3SpokePool::FundsDeposited) -> bool {
        //  This exists so we don't lose funds on fills that the user did not have balance (costs ~127k gas).
        //  Skip fills that have input amount less than 0.005 ETH on L2s.
        if decoded_data.inputAmount < uint!(5_000_000_000_000_000_U256) {
            self.log_skip_reason(
                decoded_data,
                format!(
                    "💤🤔 L2 small non exclusive fill {:?}",
                    decoded_data.inputAmount
                )
                .as_str(),
            );

            return true;
        }

        false
    }

    fn parse_deposit(&'static self, decoded_data: &IV3SpokePool::FundsDeposited) -> Option<Fill> {
        if self
            .fill_config
            .is_valid_input_token(&decoded_data.inputToken)
        {
            return None;
        }

        if self
            .fill_config
            .is_valid_output_token(&decoded_data.outputToken)
        {
            self.log_skip_reason(
                decoded_data,
                format!(
                    "❌ output_token not allowed: {:?}",
                    decoded_data.outputToken
                )
                .as_str(),
            );
            return None;
        }

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_secs();

        let is_exclusive: bool;

        if decoded_data.exclusiveRelayer == B256::ZERO {
            if self.skip_non_exclusive_deposit(decoded_data) {
                return None;
            }

            is_exclusive = false;

        // Check if exclusive relayer is set to the current relayer
        } else if decoded_data.exclusiveRelayer != *self.relayer_bytes32() {
            let exclusive_deadline = decoded_data.exclusivityDeadline as u64;

            if exclusive_deadline > timestamp {
                let remaining_time = exclusive_deadline - timestamp;
                // NOTE: Consider setting a value per origin chain or connection
                if remaining_time > 60 {
                    self.log_skip_reason(decoded_data, "🤔 exclusivity deadline too long");
                    return None;
                }

                self.log_skip_reason(
                    decoded_data,
                    format!(
                        "🧱 waiting for exclusive_relayer {:?} until: {:?}",
                        decoded_data.exclusiveRelayer, exclusive_deadline
                    )
                    .as_str(),
                );
            }

            is_exclusive = false;
        } else {
            is_exclusive = true;
        }

        let scaled_amount = self
            .fill_config
            .get_scaled_amount(&decoded_data.inputAmount, &is_exclusive);

        // Check if profit is above minimum gas cost
        let output_amount = decoded_data.outputAmount;
        if scaled_amount < output_amount {
            self.log_skip_reason(
                decoded_data,
                format!(
                    "✋ scaled_amount less than output_amount: {:?} less than {:?}",
                    scaled_amount, output_amount
                )
                .as_str(),
            );
            return None;
        }
        let profit = scaled_amount - output_amount;

        Some(Fill::new(
            self,
            decoded_data,
            &profit,
            &is_exclusive,
            &timestamp,
        ))
    }

    fn skip_non_exclusive_deposit(
        &'static self,
        decoded_data: &IV3SpokePool::FundsDeposited,
    ) -> bool {
        if *self.dest_chain.skip_non_exclusive.read().unwrap() {
            self.log_skip_reason(decoded_data, "💤 Non exclusive fill");
            return true;
        }

        if !self.dest_chain.supports_flashbots {
            if self.amount_too_small(decoded_data) {
                return true;
            }
        }

        return false;
    }

    fn tx_too_old(&'static self, deposit_timestamp: &u64, next_timestamp: &u64) -> bool {
        self.dest_chain
            .tx_too_old(deposit_timestamp, next_timestamp)
    }

    fn not_enough_balance(&'static self, output_amount: &U256) -> bool {
        self.dest_chain.not_enough_balance(output_amount)
    }

    pub fn drop_deposit(
        &'static self,
        decoded_data: &IV3SpokePool::FundsDeposited,
        deposit_id: &String,
        deposit_timestamp: &u64,
        next_timestamp: &u64,
    ) -> bool {
        return self.tx_too_old(deposit_timestamp, next_timestamp)
            || self.not_enough_balance(&decoded_data.outputAmount)
            || self.already_filled(deposit_id);
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
        return self
            .dest_chain
            .skip_deposit(
                decoded_data,
                tx,
                gross_profit,
                cached_gas_estimate,
                is_exclusive,
                next_timestamp,
                chain_base_fee,
                l1_gas_limit_buffer,
            )
            .await;
    }

    async fn check_conditions(
        &'static self,
        decoded_data: &IV3SpokePool::FundsDeposited,
        tx: &TransactionRequest,
    ) -> Option<(U256, u64)> {
        if self
            .fill_config
            .is_valid_input_token(&decoded_data.inputToken)
        {
            return None;
        }

        if self
            .fill_config
            .is_valid_output_token(&decoded_data.outputToken)
        {
            self.log_skip_reason(
                decoded_data,
                format!(
                    "❌ output_token not allowed: {:?}",
                    decoded_data.outputToken
                )
                .as_str(),
            );
            return None;
        }

        let is_exclusive: bool;

        if decoded_data.exclusiveRelayer == B256::ZERO {
            if *self.dest_chain.skip_non_exclusive.read().unwrap() {
                self.log_skip_reason(decoded_data, "💤 Non exclusive fill");
                return None;
            }

            if !self.dest_chain.supports_flashbots {
                if self.amount_too_small(decoded_data) {
                    return None;
                }
            }

            is_exclusive = false;
            // Check if exclusive relayer is set to the current relayer
        } else if decoded_data.exclusiveRelayer != *self.relayer_bytes32() {
            let exclusive_deadline = decoded_data.exclusivityDeadline as u64;

            self.log_skip_reason(
                decoded_data,
                format!(
                    "🧱 waiting for exclusive_relayer {:?} until: {:?}",
                    decoded_data.exclusiveRelayer, exclusive_deadline
                )
                .as_str(),
            );

            if self
                .dest_chain
                .wait_for_exclusive_deadline(&exclusive_deadline)
                .await
            {
                self.log_skip_reason(decoded_data, "🤔 exclusivity deadline too long");
                return None;
            }

            is_exclusive = false;
        } else {
            is_exclusive = true;
        }

        // Scale input amount to take into account rebalancing and minimum profit
        let scaled_amount = self
            .fill_config
            .get_scaled_amount(&decoded_data.inputAmount, &is_exclusive);

        // Check if profit is above minimum
        if scaled_amount < decoded_data.outputAmount {
            self.log_skip_reason(
                decoded_data,
                format!(
                    "✋ scaled_amount less than output_amount: {:?} less than {:?}",
                    scaled_amount, decoded_data.outputAmount
                )
                .as_str(),
            );
            return None;
        }
        let profit = scaled_amount - decoded_data.outputAmount;

        let gas_estimate;

        // Check if message is empty
        if decoded_data.message.len() == 0 {
            if self.dest_chain.supports_flashbots {
                let balance = self
                    .dest_chain
                    .http_provider
                    .get_balance(Address::from_word(decoded_data.recipient))
                    .await
                    .unwrap();

                gas_estimate = match balance {
                    U256::ZERO => *self.dest_chain.gas_used,
                    _ => GAS_USED_STANDARD,
                }
            } else {
                gas_estimate =
                    *self.dest_chain.gas_used + self.dest_chain.get_l1_gas_limit_buffer();
            }
        } else {
            let estimate = self.dest_chain.estimate_gas(&tx).await;

            if estimate.is_none() {
                self.log_skip_reason(decoded_data, "😭 Estimate for non-exclusive fill failed");
                return None;
            }
            gas_estimate = estimate.unwrap();
        }

        let l1_fee = self.dest_chain.get_l1_fee(tx);
        if profit < l1_fee {
            self.log_skip_reason(
                decoded_data,
                format!(
                    "⛽ profit less than l1_fee for {:?}: {:?} less than {:?}. Diff: {:?}",
                    decoded_data.outputAmount,
                    profit,
                    l1_fee,
                    l1_fee - profit
                )
                .as_str(),
            );
            return None;
        }
        let profit = profit - l1_fee;

        let gas_estimate_u256 = U256::from(gas_estimate);
        let gas_price = profit / gas_estimate_u256;

        let chain_base_fee = *self.dest_chain.base_fee.read().unwrap();
        let min_gas_price: U256;

        // If exclusive fill, lower chain_base_fee since tx can take longer to be included
        if is_exclusive {
            min_gas_price = chain_base_fee * GAS_PRICE_REDUCTION_FROM_EXCLUSIVE / DIVISOR;
        } else {
            min_gas_price = chain_base_fee;
        }

        if min_gas_price > gas_price {
            self.log_skip_reason(
                decoded_data,
                format!(
                    "🦥 gas price too low for chain: {:?} less than {:?}. Diff: {:?}",
                    gas_price,
                    chain_base_fee,
                    chain_base_fee - gas_price
                )
                .as_str(),
            );
            return None;
        }

        let balance = *self.dest_chain.balance.read().unwrap();
        // Check if output amount is too large
        if decoded_data.outputAmount > balance {
            self.log_skip_reason(
                decoded_data,
                format!(
                    "🤏 not enough balance: {:?}",
                    decoded_data.outputAmount - balance
                )
                .as_str(),
            );
            return None;
        }

        return Some((gas_price, gas_estimate));
    }

    pub async fn send_tx_envelope(
        &'static self,
        tx_envelope: EthereumTxEnvelope<TxEip4844Variant>,
    ) -> SubmittedTxEnvelope {
        // Send the raw transaction and retrieve the transaction receipt.
        // [Provider::send_tx_envelope] is a convenience method that encodes the transaction using
        // EIP-2718 encoding and broadcasts it to the network using [Provider::send_raw_transaction].
        return self
            .dest_chain
            .sequencer_provider
            .send_tx_envelope(tx_envelope)
            .await;
    }

    pub async fn simulate_fill(
        &'static self,
        deposit: &IV3SpokePool::FundsDeposited,
        deposit_id: &String,
    ) {
        // Rebuild the calldata
        let calldata = self.build_tx_calldata(&deposit);

        // Print log details if all conditions pass
        self.log(
            format!(
                "{:?} {:?}⛺ Simulating deposit amount: {:?}",
                deposit_id,
                self.is_exclusive_char(&deposit.exclusiveRelayer),
                deposit.outputAmount,
            )
            .as_str(),
        );

        match alchemy_simulate(
            &self.dest_chain.simulation_http,
            self.dest_chain.contract_address.to_string().as_str(),
            self.origin_chain.relayer_address.to_string().as_str(),
            "0x0",
            ("0x".to_owned() + hex::encode(calldata).as_str()).as_str(),
        )
        .await
        {
            Ok(response) => {
                self.log(format!("{:?} ⛺ Simulated: {:?}", deposit_id, response).as_str())
            }
            Err(e) => self.log(format!("{:?} 🐛 Error: {}", deposit_id, e).as_str()),
        }
    }
}
