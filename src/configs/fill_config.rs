use super::{
    super::utils::constants::{CHAIN_ID_ARB, CHAIN_ID_BASE, CHAIN_ID_ETH, CHAIN_ID_OP, DIVISOR},
    chain_config::ChainConfig,
};
use alloy::primitives::{b256, uint, B256, U256};

fn is_same_token(token0: &B256, token1: &'static B256) -> bool {
    return *token0 != *token1;
}

fn not_zero_address(token0: &B256) -> bool {
    return *token0 != B256::ZERO;
}

// If profit from fill is more than this, set it to this.
const MAX_PROFIT: U256 = uint!(1_500_000_000_000_U256); // 0.0000015 ETH / 1500000000000 wei

const REBALANCE_FEE_SCALED_ETH: U256 = uint!(341_U256); // 0.341bps
const REBALANCE_FEE_SCALED_BASE: U256 = uint!(231_U256); // 0.211bps
const REBALANCE_FEE_SCALED_OP: U256 = uint!(231_U256); // 0.211bps
const REBALANCE_FEE_SCALED: U256 = uint!(200_U256); // 0.20bps

// Minimum profit to be added to every rebalance fee (in bips).
const MIN_PROFIT_SCALED: U256 = uint!(0_U256); // 0bps

pub fn get_rebalance_fee_scaled(chain_id: u32) -> &'static U256 {
    match chain_id {
        CHAIN_ID_ETH => &REBALANCE_FEE_SCALED_ETH,
        CHAIN_ID_BASE => &REBALANCE_FEE_SCALED_BASE,
        CHAIN_ID_OP => &REBALANCE_FEE_SCALED_OP,
        _ => &REBALANCE_FEE_SCALED,
    }
}

const TOKEN_1: B256 = b256!("000000000000000000000000c02aaa39b223fe8d0a0e5c4f27ead9083c756cc2");
const TOKEN_42161: B256 = b256!("00000000000000000000000082af49447d8a07e3bd95bd0d56f35241523fbab1");
const TOKEN_10: B256 = b256!("0000000000000000000000004200000000000000000000000000000000000006");
const TOKEN_8453: B256 = TOKEN_10;

fn get_token(chain_id: u32) -> &'static B256 {
    match chain_id {
        CHAIN_ID_ETH => &TOKEN_1,
        CHAIN_ID_ARB => &TOKEN_42161,
        CHAIN_ID_OP => &TOKEN_10,
        CHAIN_ID_BASE => &TOKEN_8453,
        _ => panic!("Invalid chain id: {}", chain_id),
    }
}

pub struct FillConfig {
    allowed_input_token: &'static B256,
    pub allowed_output_token: &'static B256,
    min_profit_scaled: U256,
    pub simulate: bool,
}

pub fn new_static_fill_config(
    origin_chain: &'static ChainConfig,
    dest_chain: &'static ChainConfig,
    simulate: bool,
) -> &'static FillConfig {
    Box::leak(Box::new(FillConfig::new(
        get_token(*origin_chain.chain_id_u64 as u32),
        get_token(*dest_chain.chain_id_u64 as u32),
        get_rebalance_fee_scaled(*origin_chain.chain_id_u64 as u32),
        &MIN_PROFIT_SCALED,
        simulate,
    )))
}

impl FillConfig {
    fn new(
        allowed_input_token: &'static B256,
        allowed_output_token: &'static B256,
        rebalance_fee_percent: &'static U256,
        min_profit_percent: &'static U256,
        simulate: bool,
    ) -> Self {
        let min_profit = rebalance_fee_percent + min_profit_percent;
        Self {
            allowed_input_token,
            allowed_output_token,
            min_profit_scaled: DIVISOR - min_profit,
            simulate,
        }
    }

    pub fn is_valid_input_token(&'static self, token: &B256) -> bool {
        return is_same_token(token, self.allowed_input_token);
    }

    pub fn is_valid_output_token(&'static self, token: &B256) -> bool {
        // TODO: THERE IS A FUNCTION ONCHAIN WE CAN CALL TO DYNAMICALLY GET THIS INFO (IT CAN CHANGE BUT NOT OFTEN)
        // TODO: IF WE ACCEPT BSC, ZERO ADDRESS IS NOT ACCEPTABLE OUTPUT TOKEN
        // ! REMOVE ZERO ADDRESS CHECK IF WE ADD ROUTES THAT NOT SUPPORT THEM
        return not_zero_address(token) && is_same_token(token, self.allowed_output_token);
    }

    // Scale input amount to take into account rebalancing and minimum profit
    pub fn get_scaled_amount(&'static self, input_amount: &U256, is_exclusive: &bool) -> U256 {
        let scaled_amount = input_amount * self.min_profit_scaled / DIVISOR;

        if *is_exclusive || MAX_PROFIT > *input_amount {
            // We can probably just return zero here since the size will never cover costs for tx submission.
            return scaled_amount;
        }

        // In case the amount is big, we can bid more since profit will still be more than other regular txs.
        let conservative_scaled_amount = input_amount - MAX_PROFIT;

        // If scaled_amount is larger than conservative_scaled_amount, use scaled_amount
        if scaled_amount > conservative_scaled_amount {
            return scaled_amount;
        } else {
            return conservative_scaled_amount;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy::primitives::{b256, ruint::uint, B256, U256};

    #[test]
    fn test_get_scaled_amount_zero() {
        let input_amount = uint!(0_U256);

        test_get_scaled_amount(&input_amount, &input_amount)
    }

    #[test]
    fn test_get_scaled_amount_skips_max_profit() {
        let input_amount = uint!(1_000_000_U256);
        let expected_subtracted_amount = uint!(100_U256);

        test_get_scaled_amount(&input_amount, &expected_subtracted_amount)
    }

    #[test]
    fn test_get_scaled_amount_not_max_profit() {
        let input_amount = uint!(100_000_000_000_U256);
        let expected_subtracted_amount = uint!(10_000_000_U256);

        test_get_scaled_amount(&input_amount, &expected_subtracted_amount)
    }

    #[test]
    fn test_get_scaled_amount_equals_max_profit() {
        let input_amount = MAX_PROFIT * uint!(10_000_U256);
        let expected_subtracted_amount = MAX_PROFIT;

        test_get_scaled_amount(&input_amount, &expected_subtracted_amount)
    }

    #[test]
    fn test_get_scaled_amount_enters_max_profit() {
        let input_amount = uint!(1_000_000_000_000_000_000_000_000_U256);
        let expected_subtracted_amount = MAX_PROFIT;

        test_get_scaled_amount(&input_amount, &expected_subtracted_amount)
    }

    fn test_get_scaled_amount(input_amount: &U256, expected_subtracted_amount: &U256) {
        const TEST_TOKEN: &'static B256 =
            &b256!("000000000000000000000000c02aaa38b223fe8d0a0e5c4f27ead8083c756cc2");
        const MIN_PROFIT_PERC: &'static U256 = &uint!(0_U256);

        const REBALANCE_FEE: &'static U256 = &uint!(1_000_U256); // 1bps
        let fill_config = Box::leak(Box::new(FillConfig::new(
            TEST_TOKEN,
            TEST_TOKEN,
            REBALANCE_FEE,
            MIN_PROFIT_PERC,
            true,
        )));

        let scaled_amount = fill_config.get_scaled_amount(&input_amount, &false);

        let expected_scaled_amount = input_amount - expected_subtracted_amount;
        println!(
            "🚀 ~ fntest_get_scaled_amount ~ expected_scaled_amount: {:?}",
            expected_scaled_amount
        );
        assert_eq!(scaled_amount, expected_scaled_amount)
    }
}
