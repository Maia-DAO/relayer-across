use alloy::{
    primitives::{B256, U256},
    sol,
    sol_types::SolCall,
};

// Codegen from ABI file to interact with the contract.
sol!(
    #[allow(missing_docs)]
    #[sol(rpc)]
    IV3SpokePool,
    "src/abis/V3SpokePoolInterface.json"
);

pub fn build_v3_spoke_pool_calldata(
    origin_chain_id: &'static U256,
    decoded_data: &IV3SpokePool::FundsDeposited,
    output_token: &'static B256,
    relayer_bytes32: &'static B256,
) -> Vec<u8> {
    return IV3SpokePool::fillRelayCall::abi_encode(&IV3SpokePool::fillRelayCall {
        relayData: V3SpokePoolInterface::V3RelayData {
            depositor: decoded_data.depositor,
            recipient: decoded_data.recipient,
            exclusiveRelayer: decoded_data.exclusiveRelayer,
            inputToken: decoded_data.inputToken,
            outputToken: *output_token,
            inputAmount: decoded_data.inputAmount,
            outputAmount: decoded_data.outputAmount,
            originChainId: *origin_chain_id,
            depositId: decoded_data.depositId,
            fillDeadline: decoded_data.fillDeadline,
            exclusivityDeadline: decoded_data.exclusivityDeadline,
            message: decoded_data.message.clone(),
        },
        repaymentChainId: *origin_chain_id,
        repaymentAddress: *relayer_bytes32,
    });
}
