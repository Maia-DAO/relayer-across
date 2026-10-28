use super::{
    super::super::{
        super::types::providers::ChainProvider,
        constants::{CHAIN_ID_BASE, CHAIN_ID_OP},
    },
    l1_block_info::{L1BlockInfo, OpStackNetworkType},
};
use alloy::{
    primitives::{address, Address, U256},
    sol,
};
use futures::future::try_join_all;
use std::sync::{Arc, RwLock};
use IL1Block::IL1BlockInstance;

// Codegen from ABI file to interact with the contract.
sol!(
    #[allow(missing_docs)]
    #[sol(rpc)]
    IL1Block,
    "src/abis/L1Block.json"
);

// Precompile to fecth info to use logic from: 420000000000000000000000000000000000000F
const L1_BLOCK_ORACLE_OP_STACK: Address = address!("4200000000000000000000000000000000000015");
fn get_l1_block_oracle(chain_id: &'static u32) -> Address {
    match *chain_id {
        CHAIN_ID_OP => L1_BLOCK_ORACLE_OP_STACK,
        CHAIN_ID_BASE => L1_BLOCK_ORACLE_OP_STACK,
        _ => panic!("Invalid op stack chain id"),
    }
}

async fn get_l1_base_fee(
    l1_block_oracle: &IL1BlockInstance<ChainProvider>,
) -> Result<U256, Box<dyn std::error::Error + Send + Sync>> {
    return Ok(l1_block_oracle.basefee().call().await?);
}

async fn get_base_fee_scalar(
    l1_block_oracle: &IL1BlockInstance<ChainProvider>,
) -> Result<U256, Box<dyn std::error::Error + Send + Sync>> {
    return Ok(U256::from(l1_block_oracle.baseFeeScalar().call().await?));
}

async fn get_blob_base_fee(
    l1_block_oracle: &IL1BlockInstance<ChainProvider>,
) -> Result<U256, Box<dyn std::error::Error + Send + Sync>> {
    return Ok(l1_block_oracle.blobBaseFee().call().await?);
}

async fn get_blob_base_fee_scalar(
    l1_block_oracle: &IL1BlockInstance<ChainProvider>,
) -> Result<U256, Box<dyn std::error::Error + Send + Sync>> {
    return Ok(U256::from(
        l1_block_oracle.blobBaseFeeScalar().call().await?,
    ));
}

pub struct L1BlockOracle {
    l1_block_oracle: IL1BlockInstance<ChainProvider>,
    l1_block_info: Arc<RwLock<L1BlockInfo>>,
}

impl L1BlockOracle {
    pub async fn new(chain_id: &'static u32, http_provider: &ChainProvider) -> Self {
        let l1_block_oracle_address = get_l1_block_oracle(chain_id);

        let l1_block_oracle = IL1BlockInstance::new(l1_block_oracle_address, http_provider.clone());

        let l1_base_fee = get_l1_base_fee(&l1_block_oracle).await.unwrap();
        let base_fee_scalar = get_base_fee_scalar(&l1_block_oracle).await.unwrap();
        let blob_base_fee = get_blob_base_fee(&l1_block_oracle).await.unwrap();
        let blob_base_fee_scalar = get_blob_base_fee_scalar(&l1_block_oracle).await.unwrap();

        Self {
            l1_block_info: Arc::new(RwLock::new(L1BlockInfo::new(
                l1_base_fee,
                base_fee_scalar,
                blob_base_fee,
                blob_base_fee_scalar,
            ))),
            l1_block_oracle,
        }
    }

    pub fn get_l1_fee(&'static self, data: &Vec<u8>, l1_fee_type: &OpStackNetworkType) -> U256 {
        return self
            .l1_block_info
            .read()
            .unwrap()
            .get_l1_fee_for_data(&data, l1_fee_type);
    }

    pub async fn l1_block_info_updater(&'static self) {
        match self.update_l1_block_info().await {
            Ok(_) => {}
            Err(e) => {
                println!("Error updating L1 block info: {:?}", e);
            }
        };
    }

    async fn update_l1_block_info(
        &'static self,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let l1_base_fee = tokio::spawn(get_l1_base_fee(&self.l1_block_oracle));
        let base_fee_scalar = tokio::spawn(get_base_fee_scalar(&self.l1_block_oracle));
        let blob_base_fee = tokio::spawn(get_blob_base_fee(&self.l1_block_oracle));
        let blob_base_fee_scalar = tokio::spawn(get_blob_base_fee_scalar(&self.l1_block_oracle));

        let requests: [tokio::task::JoinHandle<
            Result<U256, Box<dyn std::error::Error + Send + Sync>>,
        >; 4] = [
            l1_base_fee,
            base_fee_scalar,
            blob_base_fee,
            blob_base_fee_scalar,
        ];

        let results = try_join_all(requests).await?;

        // Destructure the results directly
        let [l1_base_fee, base_fee_scalar, blob_base_fee, blob_base_fee_scalar] =
            results.into_iter().collect::<Vec<_>>().try_into().unwrap();

        let l1_base_fee = l1_base_fee?;
        let base_fee_scalar = base_fee_scalar?;
        let blob_base_fee = blob_base_fee?;
        let blob_base_fee_scalar = blob_base_fee_scalar?;

        let mut l1_block_info = self.l1_block_info.write().unwrap();
        l1_block_info.set_l1_base_fee(l1_base_fee);
        l1_block_info.set_base_fee_scalar(base_fee_scalar);
        l1_block_info.set_blob_base_fee(blob_base_fee);
        l1_block_info.set_blob_base_fee_scalar(blob_base_fee_scalar);

        Ok(())
    }
}
