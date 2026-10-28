use std::time::Duration;

use alloy::primitives::{address, b256, ruint::uint, Address, B256, U256};

pub const DIVISOR: U256 = uint!(10_000_000_U256); // 100%

pub const RELAYER_BYTES32: B256 =
    b256!("000000000000000000000000FFfFfFffFFfffFFfFFfFFFFFffFFFffffFfFFFfF");
pub const RELAYER_ADDRESS: Address = address!("FFfFfFffFFfffFFfFFfFFFFFffFFFffffFfFFFfF");
pub const KAIROS_BRIBER_ADDRESS: Address = address!("FFfFfFffFFfffFFfFFfFFFFFffFFFffffFfFFFfF");

// Consider changing data types for easier usability, maintenance and expansions
pub const CHAIN_ID_ETH: u32 = 1;
pub const CHAIN_ID_ARB: u32 = 42161;
pub const CHAIN_ID_OP: u32 = 10;
pub const CHAIN_ID_BASE: u32 = 8453;

pub const CHAIN_ID_ETH_U64: u64 = 1;
pub const CHAIN_ID_ARB_U64: u64 = 42161;
pub const CHAIN_ID_OP_U64: u64 = 10;
pub const CHAIN_ID_BASE_U64: u64 = 8453;

pub const CHAIN_ID_ETH_U256: U256 = uint!(1_U256);
pub const CHAIN_ID_ARB_U256: U256 = uint!(42161_U256);
pub const CHAIN_ID_OP_U256: U256 = uint!(10_U256);
pub const CHAIN_ID_BASE_U256: U256 = uint!(8453_U256);

// NOTE: Eth mainnet is not supported atm, if support is added, then add it to CHAINS.
pub const CHAINS: [(&'static u32, &'static u32); 6] = [
    // To Arbitrum
    (&CHAIN_ID_OP, &CHAIN_ID_ARB),
    (&CHAIN_ID_BASE, &CHAIN_ID_ARB),
    // To Optimism
    (&CHAIN_ID_ARB, &CHAIN_ID_OP),
    (&CHAIN_ID_BASE, &CHAIN_ID_OP),
    // To Base
    (&CHAIN_ID_OP, &CHAIN_ID_BASE),
    (&CHAIN_ID_ARB, &CHAIN_ID_BASE),
];

pub fn get_chain_id_u64(chain_id: &'static u32) -> &'static u64 {
    match *chain_id {
        CHAIN_ID_ETH => &CHAIN_ID_ETH_U64,
        CHAIN_ID_ARB => &CHAIN_ID_ARB_U64,
        CHAIN_ID_OP => &CHAIN_ID_OP_U64,
        CHAIN_ID_BASE => &CHAIN_ID_BASE_U64,
        _ => panic!("Invalid chain id: {}", chain_id),
    }
}

pub fn get_chain_id_u256(chain_id: &'static u32) -> &'static U256 {
    match *chain_id {
        CHAIN_ID_ETH => &CHAIN_ID_ETH_U256,
        CHAIN_ID_ARB => &CHAIN_ID_ARB_U256,
        CHAIN_ID_OP => &CHAIN_ID_OP_U256,
        CHAIN_ID_BASE => &CHAIN_ID_BASE_U256,
        _ => panic!("Invalid chain id: {}", chain_id),
    }
}

const BLOCK_TIMESTAMP_INTERVAL_ETH: u64 = 12_000;
const BLOCK_TIMESTAMP_INTERVAL_ARB: u64 = 250;
const BLOCK_TIMESTAMP_INTERVAL_OP_STACK: u64 = 2_000;

const BLOCK_INTERVAL_ETH: Duration = Duration::from_millis(BLOCK_TIMESTAMP_INTERVAL_ETH);
const BLOCK_INTERVAL_ARB: Duration = Duration::from_millis(BLOCK_TIMESTAMP_INTERVAL_ARB);
const BLOCK_INTERVAL_OP_STACK: Duration = Duration::from_millis(BLOCK_TIMESTAMP_INTERVAL_OP_STACK);

pub fn get_block_interval(chain_id: &'static u32) -> &'static Duration {
    match *chain_id {
        CHAIN_ID_ETH => &BLOCK_INTERVAL_ETH,
        CHAIN_ID_ARB => &BLOCK_INTERVAL_ARB,
        CHAIN_ID_OP => &BLOCK_INTERVAL_OP_STACK,
        CHAIN_ID_BASE => &BLOCK_INTERVAL_OP_STACK,
        _ => panic!("Invalid chain id: {}", chain_id),
    }
}

const GAS_UPDATE_INTERVAL_ETH: Duration = Duration::from_secs(12);
const GAS_UPDATE_INTERVAL_ARB: Duration = Duration::from_secs(4);
const GAS_UPDATE_INTERVAL_OP_STACK: Duration = Duration::from_secs(4);

pub fn get_gas_update_interval(chain_id: &'static u32) -> &'static Duration {
    match *chain_id {
        CHAIN_ID_ETH => &GAS_UPDATE_INTERVAL_ETH,
        CHAIN_ID_ARB => &GAS_UPDATE_INTERVAL_ARB,
        CHAIN_ID_OP => &GAS_UPDATE_INTERVAL_OP_STACK,
        CHAIN_ID_BASE => &GAS_UPDATE_INTERVAL_OP_STACK,
        _ => panic!("Invalid chain id: {}", chain_id),
    }
}

// NOTE: These values are estimations, can be improved.
const TX_PROCESS_MS_INTERVAL_ETH: u64 = 2_500;
const TX_PROCESS_MS_INTERVAL_ARB: u64 = 40; // ! This is not used for now.
const TX_PROCESS_MS_INTERVAL_OP_STACK: u64 = 750; // If lower, then it is too low (only included in next block)

// Time before a new Block starts to start to process fills
pub fn get_tx_process_ms_interval(chain_id: &'static u32) -> &'static u64 {
    match *chain_id {
        CHAIN_ID_ETH => &TX_PROCESS_MS_INTERVAL_ETH,
        CHAIN_ID_ARB => &TX_PROCESS_MS_INTERVAL_ARB,
        CHAIN_ID_OP => &TX_PROCESS_MS_INTERVAL_OP_STACK,
        CHAIN_ID_BASE => &TX_PROCESS_MS_INTERVAL_OP_STACK,
        _ => panic!("Invalid chain id: {}", chain_id),
    }
}

const TX_ESTIMATE_LIMIT_ETH: Duration = Duration::from_millis(1_500);
const TX_ESTIMATE_LIMIT_ARB: Duration = Duration::from_millis(100); // ! This is not used for now.
const TX_ESTIMATE_LIMIT_OP_STACK: Duration = Duration::from_millis(500);

// Max time for gas estimation before starting to submit fills to a new Block
pub fn get_tx_estimate_limit(chain_id: &'static u32) -> &'static Duration {
    match *chain_id {
        CHAIN_ID_ETH => &TX_ESTIMATE_LIMIT_ETH,
        CHAIN_ID_ARB => &TX_ESTIMATE_LIMIT_ARB,
        CHAIN_ID_OP => &TX_ESTIMATE_LIMIT_OP_STACK,
        CHAIN_ID_BASE => &TX_ESTIMATE_LIMIT_OP_STACK,
        _ => panic!("Invalid chain id: {}", chain_id),
    }
}
