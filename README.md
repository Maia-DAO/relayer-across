# Across ETH L2 Relayer (Deprecated)

This repository contains a reference implementation for a relayer to fill WETH intents for Across V3/V4 contracts. The configured routes are the six directed routes between Optimism, Base, and Arbitrum One, with ETH as the native/gas token. Additional OP Stack or Orbit chains require code and configuration changes.

It is **no longer maintained**, has **not been audited**, and may contain bugs or vulnerabilities.  
**Do not expect updates or support.**  

You are welcome to fork and modify it for your own use, but please read the whole README and codebase carefully before running this relayer in any environment that handles real funds.

## Disclaimer

⚠️ **Security & Financial Risk Warning**

- This code has **not been audited**.  
- There is **no guarantee of profitability**; in fact, unprofitable fills are expected.  
- Running this relayer may result in **loss of funds**.  
- Small changes can break functionality.  

It is **strongly recommended** that you do not run this relayer "as is," but instead use it as a base implementation to build upon, test thoroughly, and adapt for your own needs.

## Ethereum Mainnet

Ethereum Mainnet routes are disabled in `CHAINS`. Using mainnet as a destination requires changes to Flashbots nonce management; using it as an origin requires reorganization checks.

If you do make any changes, please make sure to test them thoroughly before using it in production.

## Gas Costs Heuristics

To avoid estimating the gas costs of every transaction, there are assumptions made about the cost of each Across fill transaction without a message to execute.

If the user already has ETH balance on the account, then a regular fill will cost roughly `105k` gas units. If the user does not have ETH balance, then the fill would cost roughly `130k` gas units.

The relayer uses the `105k` estimate for L2 fills without a message. This can cause unprofitable fills when actual gas usage is higher. The original strategy accepted this risk because a failed fill was estimated to cost around `40k` gas units, while a successful fill with an underestimate of `25k` gas units could lose less on gas. Successful fills also tie up funds until settlement. These are historical estimates, not guarantees of current costs.

Assuming the fill revenue covers exactly `105k` gas units at the submitted gas price:

| Scenario                | Estimated Gas Used | Uncovered Gas Loss |
| ----------------------- | ------------------ | ------------------ |
| Account already has ETH | ~105k              | 0                  |
| Account has no ETH      | ~130k              | ~25k               |
| Failed fill             | ~40k               | ~40k               |

Small non-exclusive fills are skipped to limit this exposure. The historical strategy assumed that recovering the loss from one failed transaction could require 20 or more successful fills. For non-exclusive fills, `MAX_PROFIT` caps the deduction from the input amount in the scaling calculation, allowing more of the remaining spread to be spent on gas. It does not guarantee or cap actual net profit.

This was originally a private strategy. Keeping the method private made it safer to use than it is now that the code is public, because others can inspect and deliberately exploit its cost assumptions. Someone can grief the relayer by inducing loss-making fills, even if doing so also costs them money and provides no direct profit. Users can also take advantage of the heuristics to obtain cheaper bridging at the relayer's expense. Consider changing this approach and using another method to calculate transaction costs before using this implementation.

## RPC Usage Optimizations

### Gas Updater

This can be configured per chain. Values are set in `get_gas_update_interval`.

On OP Stack chains, L1 gas costs are fetched periodically to calculate calldata costs for each transaction. Only Fjord and Ecotone fee calculations are implemented, and `get_l1_fee_type` hardcodes Fjord for Optimism and Base. Review these assumptions against current network fee rules before use.

On Arbitrum One, an L1 gas limit buffer is fetched periodically and added to the transaction's gas limit to pay for L1 calldata costs. The estimate uses hardcoded example fill calldata (`DEFAULT_CALLDATA` in `src/utils/arbitrum/l1_fee/l1_gas_limit.rs`), which embeds the original relayer address and other historical transaction fields. Changing `RELAYER_ADDRESS` or `RELAYER_BYTES32` does not update these encoded bytes. Replace the example with representative calldata from your relayer for a more accurate L1 gas limit buffer; address byte patterns can affect calldata compression and therefore the estimate.

### Block Timestamp Updater

To save on RPC costs to fetch block timestamps, we can assume that there will be a new block per chain according to each chain's configuration:

- Every 0.25s on Arbitrum One
- Every 2s on OP Stack chains (Optimism and Base)
- Every 12s on Ethereum Mainnet (routes disabled)

Values are set in `get_block_interval`.

## Supported RPCs

### HTTP and WSS RPCs

The current implementation uses BlastAPI as primary RPC and Alchemy as a secondary RPC for events/logs. 

The previously announced BlastAPI deprecation date was 30 October 2025. This code still uses BlastAPI endpoints; replace `get_blast_url` with a supported RPC provider before use.

Simulation mode (`SIMULATE=true`) still calls Alchemy's `alchemy_simulateExecution`, which was deprecated on 30 September 2026. **Update the simulation implementation before relying on this mode.** Alchemy lists `debug_traceCall` with `callTracer` as the closest alternative for execution traces; `eth_simulateV1` does not provide full execution traces. Neither is a direct replacement without implementation changes. See [Alchemy's migration guidance](https://support.alchemy.com/articles/8324617353-what-to-use-instead-of-the-deprecated-alchemy-simulate-methods).

### Sequencer RPCs

The implementation submits transactions directly to sequencer RPCs on L2s. The disabled Ethereum Mainnet path targets Flashbots.

### Kairos Timeboost (Deprecated)

The Kairos integration is deprecated. Arbitrum One no longer uses Timeboost; it has transitioned to Priority Gas Auctions (PGA). See the [Arbitrum Foundation explanation of the new ordering mechanism](https://blog.arbitrum.foundation/how-arbitrums-new-ordering-mechanism-creates-more-opportunities-for-traders-defi-primitives/). The integration and its FIFO assumptions remain in this repository as legacy code and require changes before use with current Arbitrum ordering.

Set `KAIROS_ENABLED=false` before running; `.env.example` still sets it to `true`. Disabling Kairos does not update the Arbitrum processing path for PGA. The constructor also still loads `PK_KAIROS` unconditionally, even when Kairos is disabled.

Historically, this implementation submitted the same fill to Kairos and directly to Arbitrum's sequencer in parallel, using a separate wallet for bids. The code sends a **2 wei** payment per bundle. Payments could be spent even when a fill reverted; the legacy integration does not condition payment on fill success.

## Private Keys

Private keys are currently loaded from a local `.env` file; `.env.example` contains placeholders only. **Update private-key management before using this relayer.** Use secure secret storage and runtime injection, and exclude `.env` and other secrets from Docker build contexts and images. The current Dockerfile copies the repository into the image, and `.dockerignore` does not exclude `.env`, so building with a real `.env` would embed its keys in the image.

## Wallet Addresses

Before use, replace the placeholder addresses in `src/utils/constants.rs`. They are independent of the private keys loaded from the environment:

- Set `RELAYER_ADDRESS` to the address derived from `PK`.
- Set `RELAYER_BYTES32` to the same address, left-padded with zeros to 32 bytes. It is used for exclusivity checks and the repayment address.
- If retaining the legacy Kairos integration, set `KAIROS_BRIBER_ADDRESS` to the address derived from `PK_KAIROS`, using a separate wallet from the relayer.

Changing `.env` keys alone is insufficient. Balance and nonce queries, transaction sender fields, and repayment addresses use these constants, and the code does not validate that they match the signers. Deriving addresses from the signers or validating them at startup is a suggested improvement.

## Deposit Event Flow

The code selects the processing path by destination chain: Arbitrum uses immediate processing, while Optimism and Base use a queue. This dispatch is independent of transaction format; the implementation currently uses legacy gas pricing.

### Queued Processing (Optimism and Base)

- Deposits are handled by `process_deposit`.
- Each accepted deposit creates a `Fill` in the destination chain's queue.
- `process_fills` evaluates fills, orders ready fills by gas price, and submits them with increasing nonces before the expected next block. Fills can also be skipped or dropped.

### Immediate Processing (Arbitrum, Legacy FIFO Assumption)

- Deposits are handled by `process_deposit_old`.
- Deposits are evaluated and submitted as soon as possible, without queue-based reordering.
- This path was designed for the former FIFO/Timeboost ordering model. It has not been adapted to Arbitrum One's current PGA ordering.

## Known Test Failures

The test suite currently cannot pass in full. `calculate_tx_l1_cost_fjord_compare_calldatas` in `src/utils/op_stack/l1_fee/l1_block_info.rs` ends with an unconditional `assert!(false)`, so it fails even when the preceding fee assertions pass. Remove or replace this assertion when addressing the tests.

The Kairos tests in `src/utils/http/kairos_stats.rs` call the live deprecated service using fixed historical order IDs and unwrap the responses. They depend on network access and retained service data, so they are not reliable offline tests. The bundle-submission test also calls the live service and only prints its result. Replace these calls with mocked responses or separate, explicitly enabled integration tests.

## Suggested Improvements

The following changes would improve correctness, security, performance, and maintainability.

Known issues and suggested changes:

- Replace the deprecated `alchemy_simulateExecution` integration used by simulation mode
- Fix the unconditional failing assertion and isolate live-service tests described above
- Derive wallet addresses from signers or validate the configured constants against the keys
- Remove the deprecated Kairos integration and adapt Arbitrum processing to PGA
- Increase test coverage
- Use Rust utilities where appropriate and remove duplicated code
- Inspect submission errors to distinguish nonce failures from other failures, such as gas-limit errors
- Improve error handling for tx submission (specifically nonce)
- Check `fillDeadline` before submitting fills on both queued and FIFO paths, including after exclusivity waits and gas estimation. Current checks cover local deposit age and `exclusivityDeadline`, but do not reject expired fill deadlines; message-free fills bypass gas estimation and can be submitted after expiry, reverting and wasting gas.
- Make the rebalance fee dynamic
- Use EIP-1559 instead of legacy gas pricing
- Add a nonce updater to run every x hours
- Add a `balanceOf` updater to run every x hours
- Clear `DashSet` Ids every x hours
- Restart `run(&chains, &unique_chains, &unique_dest_chains, &connections).await` periodically
- Change `get_l1_fee_for_data` to use Alloy's base implementation
