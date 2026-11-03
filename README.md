# Payments-Engine

This repository contains a Rust implementation of a payments engine that processes transactions and maintains account balances. The engine supports deposits, withdrawals, disputes, resolutions, and chargebacks. It also detects duplicate IDs against retained deposits.

The focus of this implementation is simplicity, correctness, and performance. It uses the `rust_decimal` crate for precise decimal arithmetic, ensuring accurate financial calculations. 

I paid special attention to the design of the engine to avoid unnecessary cloning of data structures, which can lead to performance issues. The engine uses the `Entry` API for efficient lookups and updates in the transaction map, and it enforces a maximum balance limit to prevent overflows.

Also, I designed this system to be easily understandable and maintainable, with clear separation of concerns between the engine logic, account management, and output formatting. This also leads to a more testable codebase, as demonstrated by the included integration and unit tests.


## Usage

run the engine with a CSV file containing transactions and output the resulting account balances to another CSV file:

```bash
cargo run -- transactions.csv > accounts.csv
``` 

the input CSV file should have the following columns: `type`, `client`, `tx`, and `amount`. The output CSV file will contain the final state of each account, including available balance, held funds, total balance, and whether the account is locked.

## Business rules:

The engine enforces the following business rules:
- Deposits and withdrawals must have a valid amount. Withdrawals cannot exceed the available balance.
- Disputes can only be raised on deposits, and they move the disputed amount from available to held funds. Resolutions and chargebacks can only be applied to disputed transactions.
- Transaction IDs are assumed to be globally unique, as specified. An additional duplicate check is performed against retained deposit IDs. Withdrawal IDs are not retained or checked for duplicates.
- The engine enforces an application-defined maximum total balance of 10^18 currency units after deposits. Checked arithmetic detects overflow before account balances are modified.
- Invalid records and rejected transactions are skipped and reported to stderr. Non-recoverable CSV reading errors abort processing, and the CLI does not write partial account output.
- Locked accounts reject deposits and withdrawals, but allow disputes, resolutions, and chargebacks because these operations manage previously accepted transactions.
- Chargebacks remove the disputed amount from held funds, reduce the account’s total balance, and permanently lock the account.
- Amounts only accept positive decimal values with a precision of up to four places past the decimal. Trailing zeros are accepted.
- The amount field may be empty or omitted for disputes, resolutions, and chargebacks.
- CSV headers are mandatory. And they should follow the "`type`, `client`, `tx`, and `amount`" convention.
- Output account order is not guaranteed, as it does not affect the correctness of the system.
- Disputes, resolutions, and chargebacks are ignored if the referenced deposit does not exist, belongs to another client, or is not in the required state.
- Available funds may become negative when a deposit is disputed after some or all of its funds have already been withdrawn.


## System design:

The engine is designed to be efficient and maintainable. It uses a `HashMap` to store accounts and transactions, allowing for fast lookups and updates. The `Entry` API avoids double lookups when processing deposits and rejects IDs that already exist in the retained deposit history.

Additionally, the engine is designed to be easily testable, with clear separation of concerns between the engine logic, account management, and output formatting. This allows for comprehensive unit and integration tests to ensure correctness and reliability.

The separation of the engine logic from the output formatting also allows for flexibility in how the results are presented, making it easy to adapt the engine for different use cases or output formats in the future.

Even though the engine is designed for single-threaded operation, it can be extended to support sharding or multi-threading in the future if needed, allowing for improved performance and scalability.

## Performance 

On an AMD Ryzen 7 2700X with 31 GiB of RAM running Ubuntu Linux (x86_64), processing one million deposits across 1,000 clients took 0.72 seconds and used approximately 77.3 MiB of peak resident memory. The binary was compiled with Rust 1.89.0 using the release profile. Wall-clock time and peak memory were measured with `/usr/bin/time -v`, excluding compilation and input generation. The input was cached in memory, so this does not measure cold-disk performance.
A performance test can be run using `cargo test --release million_deposits_produce_expected_output -- --ignored --nocapture`

Processing the CSV one record at a time avoids loading the entire file into memory. Memory usage is not constant: the account and transaction maps grow with the number of clients and retained deposits.
Memory usage is O(clients + retained deposits), with at most 65,536 clients because client IDs are represented as u16 values. The maps grow dynamically because the number of records is not known in advance.

I decided not to introduce complex concurrency into the main implementation. Transactions for each client must be processed in input order, since withdrawals and disputes depend on previous operations. Parallel processing would require additional coordination to preserve that order. Given the CLI scope and the measured sequential performance, I prioritized simplicity.

## Scalability (with thousands of TCP Streams)
The run function is generic over std::io::Read, so it accepts blocking input sources such as files, in-memory buffers, and TCP streams. Integrating it into an asynchronous server would require adapting the input layer or moving blocking processing off the async runtime threads.

Transactions could be routed by client ID to workers, each owning an engine and receiving operations through a bounded channel. A single ordered producer would preserve submission order for each client; multiple streams affecting the same client would require an explicit ordering policy.

Routing by client ID does not enforce global transaction ID uniqueness. The system can rely on the input guarantee of unique IDs, or introduce a shared index if global duplicate detection is required.

If the retained history exceeds available memory, a disk-backed store could be considered. Recovery after failures would also require atomic updates to account balances and transaction history, together with a replay or checkpoint strategy.

I explored client-based sharding in the optional `experiment/sharded-engine` branch. I kept it outside the main implementation to preserve the simplicity requested for this exercise.


## Tests 
Run the unit and integration tests with:
```
cargo test
```
The tests cover account balance updates, transaction state transitions, locked accounts, duplicate deposits, arithmetic overflow, input validation, invalid UTF-8 records, and output errors. Integration tests also verify complete CSV input/output scenarios. The large-dataset test is ignored by default and can be run using the command in the Performance section.


## AI Usage 
The only AI used for this implementation was github copilot, free version. And only used for writing test with autocomplete on. I would write the name of the test, se if the recomendation was ok, and then hit enter. It was deactivated for the rest of the implementation as I enjoy writing code by hand sometimes

---

Built and tested with Rust 1.89.0 (edition 2024).