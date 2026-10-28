use alloy::primitives::U256;
use op_alloy_flz::{tx_estimated_size_fjord, NON_ZERO_BYTE_COST, ZERO_BYTE_COST};

#[derive(PartialEq)]
pub enum OpStackNetworkType {
    Invalid,
    Fjord,
    Ecotone,
    // Bedrock,
}

const L1_FEE_DECIMALS: u32 = 6;

/// @notice L1 gas estimation calculation.
/// @param data Unsigned fully RLP-encoded transaction to get the L1 gas for.
/// @return Amount of L1 gas used to publish the transaction.
fn get_calldata_gas(data: &Vec<u8>) -> U256 {
    return U256::from(data.iter().fold(0, |acc, byte| {
        acc + if *byte == 0x00 {
            ZERO_BYTE_COST
        } else {
            NON_ZERO_BYTE_COST
        }
    }));
}

pub struct L1BlockInfo {
    l1_base_fee: U256,
    base_fee_scalar: U256,

    blob_base_fee: U256,
    blob_base_fee_scalar: U256,
}

impl L1BlockInfo {
    pub fn new(
        l1_base_fee: U256,
        base_fee_scalar: U256,
        blob_base_fee: U256,
        blob_base_fee_scalar: U256,
    ) -> Self {
        Self {
            l1_base_fee,
            base_fee_scalar,
            blob_base_fee,
            blob_base_fee_scalar,
        }
    }

    pub fn set_l1_base_fee(&mut self, l1_base_fee: U256) {
        self.l1_base_fee = l1_base_fee;
    }

    pub fn set_base_fee_scalar(&mut self, base_fee_scalar: U256) {
        self.base_fee_scalar = base_fee_scalar;
    }

    pub fn set_blob_base_fee(&mut self, blob_base_fee: U256) {
        self.blob_base_fee = blob_base_fee;
    }

    pub fn set_blob_base_fee_scalar(&mut self, blob_base_fee_scalar: U256) {
        self.blob_base_fee_scalar = blob_base_fee_scalar;
    }

    /// @notice Computes the L1 portion of the fee based on the size of the rlp encoded input
    ///         transaction, the current L1 base fee, and the various dynamic parameters.
    /// @param _data Unsigned fully RLP-encoded transaction to get the L1 fee for.
    /// @return L1 fee that should be paid for the tx
    pub fn get_l1_fee_for_data(&self, data: &Vec<u8>, l1_fee_type: &OpStackNetworkType) -> U256 {
        /*
         * TODO: Change this to use alloy's base implementation
         * https://github.com/bluealloy/revm/blob/6fada5efc68763054cef47917f5c6ecf29407b19/crates/optimism/src/l1block.rs#L513C1-L548
         */
        /*
         * if (isFjord) {
         *     return _getL1FeeFjord(_data);
         * } else if (isEcotone) {
         *     return _getL1FeeEcotone(_data);
         * }
         * return _getL1FeeBedrock(_data);
         */
        return match l1_fee_type {
            OpStackNetworkType::Fjord => self.get_l1_fee_fjord(data),
            OpStackNetworkType::Ecotone => self.get_l1_fee_ecotone(data),
            // OpStackNetworkType::Bedrock => self.get_l1_fee_bedrock(data),
            _ => panic!("Invalid network type"),
        };
    }

    /// @notice Fjord L1 cost based on the compressed and original tx size.
    /// @param _fastLzSize estimated compressed tx size.
    /// @return Fjord L1 fee that should be paid for the tx
    fn get_l1_fee_fjord(&self, data: &Vec<u8>) -> U256 {
        let estimated_size = tx_estimated_size_fjord(data);

        let fee_scaled = self.get_op_fee_scaled();
        return U256::from(estimated_size) * fee_scaled
            / U256::from(10_u128.pow(L1_FEE_DECIMALS * 2));
    }

    fn get_l1_fee_ecotone(&self, data: &Vec<u8>) -> U256 {
        let l1_gas_used = get_calldata_gas(data);

        let fee_scaled = self.get_op_fee_scaled();
        let fee = l1_gas_used * fee_scaled;
        return U256::from(fee / U256::from(NON_ZERO_BYTE_COST * 10_u64.pow(L1_FEE_DECIMALS)));
    }

    fn get_op_fee_scaled(&self) -> U256 {
        let base_fee_scalar = self.base_fee_scalar;
        let l1_base_fee = self.l1_base_fee;
        let blob_base_fee_scalar = self.blob_base_fee_scalar;
        let blob_base_fee = self.blob_base_fee;

        return base_fee_scalar * U256::from(NON_ZERO_BYTE_COST) * l1_base_fee
            + blob_base_fee_scalar * blob_base_fee;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy::primitives::{bytes, hex, U256};

    #[test]
    fn test_calculate_tx_l1_cost_ecotone() {
        let l1_block_info = L1BlockInfo {
            l1_base_fee: U256::from(1_000),
            base_fee_scalar: U256::from(1_000),
            blob_base_fee: U256::from(1_000),
            blob_base_fee_scalar: U256::from(1_000),
        };

        // calldataGas * (l1BaseFee * 16 * l1BaseFeeScalar + l1BlobBaseFee * l1BlobBaseFeeScalar) / (16 * 1e6)
        // = (16 * 3) * (1000 * 16 * 1000 + 1000 * 1000) / (16 * 1e6)
        // = 51
        let input = bytes!("FACADE").to_vec();
        let gas_cost = l1_block_info.get_l1_fee_for_data(&input, &OpStackNetworkType::Ecotone);
        assert_eq!(gas_cost, U256::from(51));
    }

    #[test]
    fn calculate_tx_l1_cost_ecotone() {
        // rig

        // l1 block info for OP mainnet ecotone block 118024092
        // 1710374401 (ecotone timestamp)
        // 1711603765 (block 118024092 timestamp)
        // 1720627201 (fjord timestamp)
        // <https://optimistic.etherscan.io/block/118024092>
        // decoded from
        let l1_block_info = L1BlockInfo {
            l1_base_fee: U256::from_be_bytes(hex!(
                "0000000000000000000000000000000000000000000000000000000af39ac327"
            )), // 47036678951
            base_fee_scalar: U256::from(1368),
            blob_base_fee: U256::from_be_bytes(hex!(
                "0000000000000000000000000000000000000000000000000000000d5ea528d2"
            )), // 57422457042
            blob_base_fee_scalar: U256::from(810949),
        };

        // second tx in OP mainnet ecotone block 118024092
        // <https://optimistic.etherscan.io/tx/0xa75ef696bf67439b4d5b61da85de9f3ceaa2e145abe982212101b244b63749c2>
        let tx = bytes!("02f8b30a832253fc8402d11f39842c8a46398301388094dc6ff44d5d932cbd77b52e5612ba0529dc6226f180b844a9059cbb000000000000000000000000d43e02db81f4d46cdf8521f623d21ea0ec7562a50000000000000000000000000000000000000000000000008ac7230489e80000c001a02947e24750723b48f886931562c55d9e07f856d8e06468e719755e18bbc3a570a0784da9ce59fd7754ea5be6e17a86b348e441348cd48ace59d174772465eadbd1").to_vec();

        // l1 gas used for tx and l1 fee for tx, from OP mainnet block scanner
        // <https://optimistic.etherscan.io/tx/0xa75ef696bf67439b4d5b61da85de9f3ceaa2e145abe982212101b244b63749c2>
        let expected_l1_fee = U256::from_be_bytes(hex!(
            "000000000000000000000000000000000000000000000000000006a510bd7431" // 7306020222001 wei
        ));

        // test

        let l1_fee = l1_block_info.get_l1_fee_for_data(&tx, &OpStackNetworkType::Ecotone);

        assert_eq!(l1_fee, expected_l1_fee)
    }

    #[test]
    fn test_calculate_tx_l1_cost_fjord() {
        // l1FeeScaled = baseFeeScalar*l1BaseFee*16 + blobFeeScalar*l1BlobBaseFee
        //             = 1000 * 1000 * 16 + 1000 * 1000
        //             = 17e6
        let l1_block_info = L1BlockInfo {
            l1_base_fee: U256::from(1_000),
            base_fee_scalar: U256::from(1_000),
            blob_base_fee: U256::from(1_000),
            blob_base_fee_scalar: U256::from(1_000),
        };

        // fastLzSize = 4
        // estimatedSize = max(minTransactionSize, intercept + fastlzCoef*fastlzSize)
        //               = max(100e6, 836500*4 - 42585600)
        //               = 100e6
        let input = bytes!("FACADE").to_vec();
        // l1Cost = estimatedSize * l1FeeScaled / 1e12
        //        = 100e6 * 17 / 1e6
        //        = 1700
        let gas_cost = l1_block_info.get_l1_fee_for_data(&input, &OpStackNetworkType::Fjord);
        assert_eq!(gas_cost, U256::from(1700));

        // fastLzSize = 202
        // estimatedSize = max(minTransactionSize, intercept + fastlzCoef*fastlzSize)
        //               = max(100e6, 836500*202 - 42585600)
        //               = 126387400
        let input = bytes!("02f901550a758302df1483be21b88304743f94f80e51afb613d764fa61751affd3313c190a86bb870151bd62fd12adb8e41ef24f3f000000000000000000000000000000000000000000000000000000000000006e000000000000000000000000af88d065e77c8cc2239327c5edb3a432268e5831000000000000000000000000000000000000000000000000000000000003c1e5000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000a000000000000000000000000000000000000000000000000000000000000000148c89ed219d02f1a5be012c689b4f5b731827bebe000000000000000000000000c001a033fd89cb37c31b2cba46b6466e040c61fc9b2a3675a7f5f493ebd5ad77c497f8a07cdf65680e238392693019b4092f610222e71b7cec06449cb922b93b6a12744e").to_vec();
        // l1Cost = estimatedSize * l1FeeScaled / 1e12
        //        = 126387400 * 17 / 1e6
        //        = 2148
        let gas_cost = l1_block_info.get_l1_fee_for_data(&input, &OpStackNetworkType::Fjord);
        assert_eq!(gas_cost, U256::from(2148));
    }

    #[test]
    fn calculate_tx_l1_cost_fjord() {
        // rig

        // L1 block info for OP mainnet fjord block 124665056
        // <https://optimistic.etherscan.io/block/124665056>
        let l1_block_info = L1BlockInfo {
            l1_base_fee: U256::from(1055991687),
            base_fee_scalar: U256::from(5227),
            blob_base_fee: U256::from(1014213),
            blob_base_fee_scalar: U256::from(1),
        };

        // Second tx in OP mainnet Fjord block 124665056
        // <https://optimistic.etherscan.io/tx/0x1059e8004daff32caa1f1b1ef97fe3a07a8cf40508f5b835b66d9420d87c4a4a>
        let tx = bytes!("02f904940a8303fba78401d6d2798401db2b6d830493e0943e6f4f7866654c18f536170780344aa8772950b680b904246a761202000000000000000000000000087000a300de7200382b55d40045000000e5d60e0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000014000000000000000000000000000000000000000000000000000000000000000010000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000003a0000000000000000000000000000000000000000000000000000000000000022482ad56cb0000000000000000000000000000000000000000000000000000000000000020000000000000000000000000000000000000000000000000000000000000000200000000000000000000000000000000000000000000000000000000000000400000000000000000000000000000000000000000000000000000000000000120000000000000000000000000dc6ff44d5d932cbd77b52e5612ba0529dc6226f1000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000600000000000000000000000000000000000000000000000000000000000000044095ea7b300000000000000000000000021c4928109acb0659a88ae5329b5374a3024694c0000000000000000000000000000000000000000000000049b9ca9a6943400000000000000000000000000000000000000000000000000000000000000000000000000000000000021c4928109acb0659a88ae5329b5374a3024694c000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000600000000000000000000000000000000000000000000000000000000000000024b6b55f250000000000000000000000000000000000000000000000049b9ca9a694340000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000415ec214a3950bea839a7e6fbb0ba1540ac2076acd50820e2d5ef83d0902cdffb24a47aff7de5190290769c4f0a9c6fabf63012986a0d590b1b571547a8c7050ea1b00000000000000000000000000000000000000000000000000000000000000c080a06db770e6e25a617fe9652f0958bd9bd6e49281a53036906386ed39ec48eadf63a07f47cf51a4a40b4494cf26efc686709a9b03939e20ee27e59682f5faa536667e").to_vec();

        // L1 gas used for tx and L1 fee for tx, from OP mainnet block scanner
        // https://optimistic.etherscan.io/tx/0x1059e8004daff32caa1f1b1ef97fe3a07a8cf40508f5b835b66d9420d87c4a4a
        let expected_l1_fee = U256::from_be_bytes(hex!(
            "00000000000000000000000000000000000000000000000000000005bf1ab43d"
        ));

        // test

        let l1_fee = l1_block_info.get_l1_fee_for_data(&tx, &OpStackNetworkType::Fjord);

        assert_eq!(l1_fee, expected_l1_fee)
    }

    #[test]
    fn calculate_tx_l1_cost_fjord_compare_calldatas() {
        // rig

        // L1 block info for OP mainnet fjord block 134358125
        // <https://optimistic.etherscan.io/block/134358125>
        let l1_block_info = L1BlockInfo {
            l1_base_fee: U256::from(759374120),
            base_fee_scalar: U256::from(5227),
            blob_base_fee: U256::from(392305812),
            blob_base_fee_scalar: U256::from(1014213),
        };

        // Second tx in OP mainnet Fjord block 124665056
        // <https://optimistic.etherscan.io/tx/0x52e9f079f38d648feee23f41c613d2a4de9e53531afedde13f7acfed60a27cb9>
        let tx_old = bytes!("f9026b82524d8409e5803f830249f0946f26bf09b1c792e3228e5467807a900a503c028180b90204deff4b2400000000000000000000000000000000000000000000000000000000000000600000000000000000000000000000000000000000000000000000000000002105000000000000000000000000FFfFfFffFFfffFFfFFfFFFFFffFFFffffFfFFFfF00000000000000000000000063152b532d0fa9a7e42040b462a310ce494ee23b00000000000000000000000063152b532d0fa9a7e42040b462a310ce494ee23b000000000000000000000000000000000000000000000000000000000000000000000000000000000000000042000000000000000000000000000000000000060000000000000000000000004200000000000000000000000000000000000006000000000000000000000000000000000000000000000000024d55b5a946b000000000000000000000000000000000000000000000000000024d3fc637a992290000000000000000000000000000000000000000000000000000000000002105000000000000000000000000000000000000000000000000000000000037aa700000000000000000000000000000000000000000000000000000000067f845b600000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000180000000000000000000000000000000000000000000000000000000000000000038a01986aad0ff1de7ba8a115696ce263aa732e708b4ea308500f08fd9096ec912e09fa97783940c93248750bbc34f0aaed6b149abc1b9e6d0444fe3df20829457f5 ").to_vec();

        // Second tx in OP mainnet Fjord block 124665056
        // <https://optimistic.etherscan.io/tx/0x52e9f079f38d648feee23f41c613d2a4de9e53531afedde13f7acfed60a27cb9>
        let tx_new = bytes!("f9026b82524d8409e5803f830249f0946f26bf09b1c792e3228e5467807a900a503c028180b9020463152b532d0fa9a7e42040b462a310ce494ee23bb4327f154b3bd8fea64f352c649022bd0e2ef9240000000000000000000000000000000000000000000000000000000000000000000000000000000000000000024d55b5a946b000000000000000000000000000000000000000000000000000024d3fc637a99229000000000037aa7067f845b6000000004206902438a01986aad0ff1de7ba8a115696ce263aa732e708b4ea308500f08fd9096ec912e09fa97783940c93248750bbc34f0aaed6b149abc1b9e6d0444fe3df20829457f5").to_vec();

        // L1 gas used for tx and L1 fee for tx, from OP mainnet block scanner
        // https://optimistic.etherscan.io/tx/0x52e9f079f38d648feee23f41c613d2a4de9e53531afedde13f7acfed60a27cb9
        let expected_l1_fee = U256::from_be_bytes(hex!(
            "0000000000000000000000000000000000000000000000000000001240032fd2"
        ));

        // test

        let l1_fee = l1_block_info.get_l1_fee_for_data(&tx_old, &OpStackNetworkType::Fjord);
        let l1_fee_new = l1_block_info.get_l1_fee_for_data(&tx_new, &OpStackNetworkType::Fjord);
        println!(
            "🚀 ~ fncalculate_tx_l1_cost_fjord_compare_calldatas ~ l1_fee_new: {}, {}",
            l1_fee, l1_fee_new
        );

        assert_eq!(l1_fee, expected_l1_fee);

        assert!(l1_fee > l1_fee_new);
        assert!(false)
    }
}
