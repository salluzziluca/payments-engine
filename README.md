# Concurrent processing experiment

This branch explores client-based sharding as an optional extension. It is separate from the main submission to preserve the simplicity requested for the exercise.

## Design

Each worker owns an independent `Engine`. Transactions are routed using:

```text
client_id % worker_count
```

A single producer submits transactions in input order. Each worker processes its queue sequentially, preserving the order of operations for each client while allowing different shards to run concurrently.

Bounded channels provide backpressure: submitting a transaction blocks when its destination queue is full.

Calling `finish` closes the channels, drains pending operations, joins all workers, and gathers the final accounts. Rejected transactions are returned in submission order.

## Running the example

The CSV reader and parser remain sequential; parsed transactions are dispatched to workers.

Run with four workers and a queue capacity of 64 transactions per worker:

```bash
cargo run --release --example sharded_csv -- tests/data/disputes.csv 4 > accounts.csv
```

Diagnostics go to stderr; stdout contains only the output CSV.

## Tests

```bash
cargo test --test sharded_tests
cargo test --lib sharded::tests
```

Tests compare final accounts and rejected operations against the sequential engine using 1, 2, 4, and 7 workers, with queue capacities of 0, 1, and 16.

They also cover interleaved clients, draining queued operations, empty input, invalid worker counts, disconnected channels, and worker panics.

## Limitations

- The interface assumes one ordered producer. Multiple input streams affecting the same client would require an explicit ordering policy.
- Transaction IDs are assumed to be globally unique. Additional duplicate-deposit detection is local to each shard.
- Bounded queues limit pending messages, but retained deposits and rejected transactions still consume memory proportional to their counts.
- A busy shard can block the producer and delay submissions to other shards.
- Worker failures are reported by `finish`; partial results are discarded on infrastructure failure.
- This extension does not implement TCP networking, asynchronous I/O, or parallel CSV parsing.
- Concurrency does not guarantee higher throughput. Channel overhead and workload distribution may outweigh the benefits for small operations.

## AI disclosure

This document was written with AI assistance. The code was not AI-generated. This statement concerns this document and does not replace the project's full AI usage disclosure and transcript.