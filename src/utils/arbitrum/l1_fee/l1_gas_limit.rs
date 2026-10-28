use super::super::super::super::{
    configs::chain_config::KAIROS_PAYMENT_ADDRESS, types::providers::ChainProvider,
    utils::constants::CHAIN_ID_ARB,
};
use alloy::{
    primitives::{address, bytes, uint, Address, Bytes, U256},
    sol,
};
use std::sync::{Arc, RwLock};
use NodeInterface::{gasEstimateL1ComponentReturn, NodeInterfaceInstance};

// Codegen from ABI file to interact with the contract.
sol!(
    #[allow(missing_docs)]
    #[sol(rpc)]
    NodeInterface,
    "src/abis/NodeInterface.json"
);

const NODE_INTERFACE: Address = address!("00000000000000000000000000000000000000C8");

const NON_ZERO_BYTE_COST: U256 = uint!(16_U256);

impl gasEstimateL1ComponentReturn {
    pub fn calc_l1_gas_limit(&self) -> u64 {
        if self.gasEstimateForL1 == 0 || self.l1BaseFeeEstimate == U256::ZERO {
            return 0;
        }

        // Getting useful values for calculating the formula
        let l1_gas_estimated = self.gasEstimateForL1;
        let l2_estimated_price = self.baseFee;
        let l1_estimated_price = self.l1BaseFeeEstimate * NON_ZERO_BYTE_COST;

        // Calculating some extra values to be able to apply all variables of the formula
        // -------------------------------------------------------------------------------
        // NOTE: This one might be a bit confusing, but l1GasEstimated (B in the formula) is calculated based on l2 gas fees
        let l1_cost = U256::from(l1_gas_estimated) * l2_estimated_price;
        // NOTE: This is similar to 140 + utils.hexDataLength(txData);
        let l1_size = l1_cost / l1_estimated_price;

        // Getting the result of the formula
        // ---------------------------------
        // L1C (L1 Cost) = L1P * L1S
        let l1c = l1_estimated_price * l1_size;

        // B (Extra Buffer) = L1C / P
        let b = l1c / l2_estimated_price;

        // Add a 5% buffer
        return b.to::<u64>() * 110 / 100;
    }
}

const DEFAULT_CALLDATA: Bytes = bytes!("deff4b2400000000000000000000000000000000000000000000000000000000000000600000000000000000000000000000000000000000000000000000000000002105000000000000000000000000FFfFfFffFFfffFFfFFfFFFFFffFFFffffFfFFFfF000000000000000000000000c6e29c217c2b6db72ff0ef4ab56141ba5378b101000000000000000000000000c6e29c217c2b6db72ff0ef4ab56141ba5378b1010000000000000000000000000000000000000000000000000000000000000000000000000000000000000000420000000000000000000000000000000000000600000000000000000000000082af49447d8a07e3bd95bd0d56f35241523fbab1000000000000000000000000000000000000000000000000001550f7dca7000000000000000000000000000000000000000000000000000000154db6502e9d7600000000000000000000000000000000000000000000000000000000000021050000000000000000000000000000000000000000000000000000000000347cd20000000000000000000000000000000000000000000000000000000067ce4a51000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000001800000000000000000000000000000000000000000000000000000000000000000");

async fn gas_estimate_l1_component(
    node_interface: &NodeInterfaceInstance<ChainProvider>,
    to: &'static Address,
    // data: &Bytes,
) -> Result<gasEstimateL1ComponentReturn, Box<dyn std::error::Error + Send + Sync>> {
    return Ok(node_interface
        .gasEstimateL1Component(*to, false, DEFAULT_CALLDATA)
        .call()
        .await?);
}

async fn gas_estimate_l1_component_eth_transfer(
    node_interface: &NodeInterfaceInstance<ChainProvider>,
) -> Result<gasEstimateL1ComponentReturn, Box<dyn std::error::Error + Send + Sync>> {
    return Ok(node_interface
        .gasEstimateL1Component(KAIROS_PAYMENT_ADDRESS, false, bytes!(""))
        .value(uint!(2_U256))
        .call()
        .await?);
}

pub struct L1GasLimit {
    pub l1_gas_limit_buffer: Arc<RwLock<u64>>,
    pub l1_gas_limit_buffer_eth_transfer: Arc<RwLock<u64>>,

    to: &'static Address,
    node_interface: NodeInterfaceInstance<ChainProvider>,
}

impl L1GasLimit {
    pub async fn new(
        chain_id: &'static u32,
        http_provider: &ChainProvider,
        to: &'static Address,
    ) -> Option<Self> {
        if *chain_id != CHAIN_ID_ARB {
            return None;
        }

        let node_interface = NodeInterfaceInstance::new(NODE_INTERFACE, http_provider.clone());
        let l1_gas_limit_buffer = gas_estimate_l1_component(&node_interface, to)
            .await
            .unwrap()
            .calc_l1_gas_limit();
        let l1_gas_limit_buffer_eth_transfer =
            gas_estimate_l1_component_eth_transfer(&node_interface)
                .await
                .unwrap()
                .calc_l1_gas_limit();

        Some(Self {
            l1_gas_limit_buffer: Arc::new(RwLock::new(l1_gas_limit_buffer)),
            l1_gas_limit_buffer_eth_transfer: Arc::new(RwLock::new(
                l1_gas_limit_buffer_eth_transfer,
            )),

            to,
            node_interface,
        })
    }

    pub async fn update_l1_gas_limit_buffer(&'static self) {
        match self.get_default_gas_estimate_l1_component().await {
            Ok(l1_component) => {
                let new_l1_gas_limit_buffer = l1_component.calc_l1_gas_limit();
                let mut l1_gas_limit_buffer = self.l1_gas_limit_buffer.write().unwrap();
                *l1_gas_limit_buffer = new_l1_gas_limit_buffer;
            }
            Err(e) => {
                println!("Error updating L1 gas limit info: {:?}", e);
            }
        }
    }

    pub async fn update_l1_gas_limit_buffer_eth_transfer(&'static self) {
        match self
            .get_default_gas_estimate_l1_component_eth_transfer()
            .await
        {
            Ok(l1_component) => {
                let new_l1_gas_limit_buffer_eth_transfer = l1_component.calc_l1_gas_limit();
                let mut l1_gas_limit_buffer_eth_transfer =
                    self.l1_gas_limit_buffer_eth_transfer.write().unwrap();
                *l1_gas_limit_buffer_eth_transfer = new_l1_gas_limit_buffer_eth_transfer;
            }
            Err(e) => {
                println!("Error updating L1 gas limit eth transfer info: {:?}", e);
            }
        }
    }

    async fn get_default_gas_estimate_l1_component(
        &'static self,
    ) -> Result<gasEstimateL1ComponentReturn, Box<dyn std::error::Error + Send + Sync>> {
        return Ok(gas_estimate_l1_component(&self.node_interface, self.to).await?);
    }

    async fn get_default_gas_estimate_l1_component_eth_transfer(
        &'static self,
    ) -> Result<gasEstimateL1ComponentReturn, Box<dyn std::error::Error + Send + Sync>> {
        return Ok(gas_estimate_l1_component_eth_transfer(&self.node_interface).await?);
    }
}
