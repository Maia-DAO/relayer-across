use super::{
    super::configs::connection_config::{is_exclusive_char, ConnectionConfig},
    super::utils::v3_spoke_pool::IV3SpokePool::FundsDeposited,
};
use alloy::{
    consensus::{EthereumTxEnvelope, TxEip4844Variant},
    network::Ethereum,
    primitives::U256,
    providers::PendingTransactionBuilder,
    rpc::types::TransactionRequest,
    transports::{RpcError, TransportErrorKind},
    uint,
};
use std::{
    cmp::Ordering,
    sync::{Arc, RwLock},
};

#[derive(Clone)]
pub struct Fill {
    connection_config: &'static ConnectionConfig,
    deposit: FundsDeposited,
    pub deposit_id: String,
    estimate_tx: TransactionRequest,
    max_gas_price: Arc<RwLock<U256>>,
    gas_estimate: Arc<RwLock<u64>>,
    gross_profit: U256,
    is_exclusive: bool,
    timestamp: u64,
}

impl Fill {
    pub fn new(
        connection_config: &'static ConnectionConfig,
        deposit: &FundsDeposited,
        gross_profit: &U256,
        is_exclusive: &bool,
        timestamp: &u64,
    ) -> Self {
        // NOTE: Consider estimating message if exists. Or if there is enough time until new block, check user balance.
        Self {
            connection_config,
            deposit: deposit.clone(),
            deposit_id: deposit.depositId.to_string(),
            estimate_tx: connection_config.build_tx(deposit),
            max_gas_price: Arc::new(RwLock::new(uint!(0_U256))),
            gas_estimate: Arc::new(RwLock::new(0)),
            gross_profit: *gross_profit,
            is_exclusive: *is_exclusive,
            timestamp: *timestamp,
        }
    }

    pub fn log(&self, message: &str) {
        self.connection_config.log(message);
    }

    fn is_exclusive_char(&self) -> &'static char {
        is_exclusive_char(
            &self.deposit.exclusiveRelayer,
            self.connection_config.relayer_bytes32(),
        )
    }

    fn log_submitted(&self, pending_tx: &PendingTransactionBuilder<Ethereum>) {
        self.log(
            format!(
                "{:?} {:?}⛺ Filled for revenue {:?}: {:?} {:?}",
                self.deposit_id,
                self.is_exclusive_char(),
                self.deposit.inputAmount - self.deposit.outputAmount,
                self.max_gas_price.read().unwrap(),
                pending_tx.tx_hash()
            )
            .as_str(),
        );
    }

    fn log_failed(&self, error: &RpcError<TransportErrorKind>) {
        self.log(
            format!(
                "{:?} 🐛 Error sending first tx: {:?}",
                self.deposit_id, error
            )
            .as_str(),
        );

        if self.deposit.exclusiveRelayer == *self.connection_config.relayer_bytes32() {
            self.log(format!("{:?} 🔥🐛 Was exclusive fill", self.deposit_id).as_str());
        }
    }

    pub fn drop_deposit(&self, next_timestamp: &u64) -> bool {
        return self.connection_config.drop_deposit(
            &self.deposit,
            &self.deposit_id,
            &self.timestamp,
            next_timestamp,
        );
    }

    pub async fn skip_deposit(
        self: Arc<Self>,
        next_timestamp: u64,
        chain_base_fee: U256,
        l1_gas_limit_buffer: u64,
    ) -> (Arc<Self>, bool) {
        let cached_gas_estimate = *self.gas_estimate.read().unwrap();
        let gas_info = self
            .connection_config
            .skip_deposit(
                &self.deposit,
                &self.estimate_tx,
                &self.gross_profit,
                &cached_gas_estimate,
                &self.is_exclusive,
                &next_timestamp,
                &chain_base_fee,
                &l1_gas_limit_buffer,
            )
            .await;

        if gas_info.is_none() {
            return (Arc::clone(&self), true);
        }

        let (max_gas_price, gas_estimate) = gas_info.unwrap();
        *self.max_gas_price.write().unwrap() = max_gas_price;
        *self.gas_estimate.write().unwrap() = gas_estimate;

        return (Arc::clone(&self), false);
    }

    async fn build_tx_envelope(&self, nonce: &u64) -> EthereumTxEnvelope<TxEip4844Variant> {
        let cached_max_gas_price = *self.max_gas_price.read().unwrap();
        let cached_gas_estimate = *self.gas_estimate.read().unwrap();
        return self
            .connection_config
            .build_tx_envelope(
                &self.estimate_tx,
                &cached_max_gas_price,
                &cached_gas_estimate,
                nonce,
            )
            .await;
    }

    async fn simulate_fill(&self) {
        self.connection_config
            .simulate_fill(&self.deposit, &self.deposit_id)
            .await;
    }

    pub async fn submit(self: Arc<Self>, nonce: u64) -> (Arc<Self>, bool) {
        if self.connection_config.simulate() {
            self.simulate_fill().await;
            return (Arc::clone(&self), true);
        }

        self.submit_fill(&nonce).await
    }

    async fn submit_fill(self: Arc<Self>, nonce: &u64) -> (Arc<Self>, bool) {
        let tx_envelope = self.build_tx_envelope(nonce).await;
        let call = self.connection_config.send_tx_envelope(tx_envelope);
        match call.await {
            Ok(pending_tx) => {
                self.log_submitted(&pending_tx);
            }
            Err(e) => {
                self.log_failed(&e);

                return (Arc::clone(&self), false);
            }
        };

        (Arc::clone(&self), true)
    }
}

// The priority queue depends on `Ord`.
// Explicitly implement the trait so the queue becomes a min-heap
// instead of a max-heap.
impl Ord for Fill {
    fn cmp(&self, other: &Self) -> Ordering {
        // Notice that we flip the ordering on costs.
        // In case of a tie we compare positions - this step is necessary
        // to make implementations of `PartialEq` and `Ord` consistent.
        self.max_gas_price
            .read()
            .unwrap()
            .cmp(&other.max_gas_price.read().unwrap())
            .then_with(|| self.deposit_id.cmp(&other.deposit_id))
    }
}

// `PartialOrd` needs to be implemented as well.
impl PartialOrd for Fill {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

// `Eq` needs to be implemented as well.
impl Eq for Fill {}

// `PartialEq` needs to be implemented as well.
impl PartialEq for Fill {
    fn eq(&self, other: &Self) -> bool {
        self.deposit_id == other.deposit_id
    }

    fn ne(&self, other: &Self) -> bool {
        !self.eq(other)
    }
}
