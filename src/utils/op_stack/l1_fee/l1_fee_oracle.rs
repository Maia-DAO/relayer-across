use super::{
    super::super::{
        super::types::providers::ChainProvider,
        constants::{CHAIN_ID_BASE, CHAIN_ID_OP},
    },
    l1_block_info::OpStackNetworkType,
    l1_block_oracle::L1BlockOracle,
};
use alloy::{
    consensus::{SignableTransaction, TxLegacy},
    primitives::U256,
    rpc::types::TransactionRequest,
};

// TODO: Make this dynamic by calling 420000000000000000000000000000000000000F to check for op stack network type
fn get_l1_fee_type(chain_id: &'static u32) -> OpStackNetworkType {
    match *chain_id {
        CHAIN_ID_OP => OpStackNetworkType::Fjord,
        CHAIN_ID_BASE => OpStackNetworkType::Fjord,
        _ => OpStackNetworkType::Invalid,
    }
}

pub struct L1FeeOracle {
    l1_fee_type: OpStackNetworkType,
    l1_block_oracle: &'static L1BlockOracle,
}

impl L1FeeOracle {
    pub async fn new(chain_id: &'static u32, http_provider: &ChainProvider) -> Option<Self> {
        let l1_fee_type = get_l1_fee_type(chain_id);

        if l1_fee_type == OpStackNetworkType::Invalid {
            return None;
        }

        Some(Self {
            l1_fee_type,
            l1_block_oracle: Box::leak(Box::new(L1BlockOracle::new(chain_id, http_provider).await)),
        })
    }

    pub async fn update_l1_block_info(&'static self) {
        self.l1_block_oracle.l1_block_info_updater().await;
    }

    pub fn get_l1_fee(&'static self, tx: &TransactionRequest) -> U256 {
        let tx_type = tx.clone().build_typed_tx().unwrap();
        let legacy_tx: &TxLegacy = tx_type.legacy().unwrap();
        let data: Vec<u8> = legacy_tx.encoded_for_signing();
        return self.l1_block_oracle.get_l1_fee(&data, &self.l1_fee_type);
    }
}
