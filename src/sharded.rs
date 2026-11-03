use std::{
    collections::HashMap,
    sync::mpsc::{self, Receiver, SyncSender},
    thread::{self, JoinHandle},
};

use crate::{
    account::Account,
    engine::{Engine, EngineError},
    parser::Query,
};

#[derive(Debug, thiserror::Error)]
pub enum ShardedError {
    #[error("worker count must be greater than zero")]
    InvalidWorkerCount,

    #[error("could not start worker: {0}")]
    Spawn(#[from] std::io::Error),

    #[error("worker {0} stopped receiving transactions")]
    Disconnected(usize),

    #[error("worker {0} panicked")]
    WorkerPanicked(usize),
}

/// Identifies an operation rejected by an engine.
#[derive(Debug)]
pub struct RejectedTransaction {
    pub sequence: usize,
    pub client_id: u16,
    pub tx_id: u32,
    pub error: EngineError,
}

/// Final accounts and rejected operations in submission order.
#[derive(Debug)]
pub struct ShardedOutput {
    pub clients: HashMap<u16, Account>,
    pub rejected: Vec<RejectedTransaction>,
}

// Message transferred from the producer to a worker.
struct IndexedQuery {
    sequence: usize,
    query: Query,
}

// Each worker returns ownership of its engine and its rejections.
struct WorkerOutput {
    engine: Engine,
    rejected: Vec<RejectedTransaction>,
}

/// Concurrent engines partitioned by client, fed by one ordered producer.
pub struct ShardedEngine {
    senders: Vec<SyncSender<IndexedQuery>>,
    workers: Vec<JoinHandle<WorkerOutput>>,
    next_sequence: usize,
}

/// Processes queued operations sequentially until all senders are dropped.
fn process_worker(receiver: Receiver<IndexedQuery>) -> WorkerOutput {
    let mut output = WorkerOutput {
        engine: Engine::default(),
        rejected: Vec::new(),
    };

    for IndexedQuery { sequence, query } in receiver {
        let client_id = query.client_id;
        let tx_id = query.tx_id;

        if let Err(error) = output.engine.process_transaction(query) {
            output.rejected.push(RejectedTransaction {
                sequence,
                client_id,
                tx_id,
                error,
            });
        }
    }

    output
}

impl ShardedEngine {
    /// Starts one engine per worker with a bounded input queue.
    ///
    /// A zero queue capacity creates a rendezvous channel.
    ///
    /// # Errors
    /// Rejects zero workers and propagates thread creation failures.
    pub fn new(worker_count: usize, queue_capacity: usize) -> Result<Self, ShardedError> {
        if worker_count == 0 {
            return Err(ShardedError::InvalidWorkerCount);
        }

        let mut pool = Self {
            senders: Vec::new(),
            workers: Vec::new(),
            next_sequence: 0,
        };

        for shard in 0..worker_count {
            let (sender, receiver) = mpsc::sync_channel(queue_capacity);

            let worker = thread::Builder::new()
                .name(format!("payments-shard-{shard}"))
                .spawn(move || process_worker(receiver))?;

            pool.senders.push(sender);
            pool.workers.push(worker);
        }

        Ok(pool)
    }
    /// Enqueues an operation, blocking when the destination queue is full.
    ///
    /// Operations for the same client go to the same worker in submission order.
    /// Successful enqueueing does not mean the transaction was accepted:
    /// account rejections are collected and returned by `finish`.
    ///
    /// # Errors
    /// Returns an error if the destination worker stopped receiving.
    pub fn submit(&mut self, query: Query) -> Result<(), ShardedError> {
        let shard = usize::from(query.client_id) % self.senders.len();

        self.senders[shard]
            .send(IndexedQuery {
                sequence: self.next_sequence,
                query,
            })
            .map_err(|_| ShardedError::Disconnected(shard))?;

        self.next_sequence += 1;

        Ok(())
    }

    /// Drains queued operations, joins all workers, and returns final results.
    ///
    /// Rejections are sorted by their original submission position.
    ///
    /// # Errors
    /// Reports a worker panic after joining all workers.
    /// Partial results are discarded on worker failure.
    pub fn finish(mut self) -> Result<ShardedOutput, ShardedError> {
        // Workers exit once their queues are closed and fully drained.
        self.senders.clear();

        let mut output = ShardedOutput {
            clients: HashMap::new(),
            rejected: Vec::new(),
        };

        let mut failure = None;

        for (shard, worker) in self.workers.drain(..).enumerate() {
            match worker.join() {
                Ok(result) => {
                    output.clients.extend(result.engine.into_clients());
                    output.rejected.extend(result.rejected);
                }
                Err(_) => {
                    if failure.is_none() {
                        failure = Some(ShardedError::WorkerPanicked(shard));
                    }
                }
            }
        }

        if let Some(error) = failure {
            return Err(error);
        }

        output
            .rejected
            .sort_unstable_by_key(|rejection| rejection.sequence);

        Ok(output)
    }
}

impl Drop for ShardedEngine {
    fn drop(&mut self) {
        // Close every queue before waiting for workers to finish.
        self.senders.clear();

        for worker in self.workers.drain(..) {
            // Cleanup must not panic while another panic is unwinding.
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::TransactionType;
    use rust_decimal::Decimal;

    #[test]
    fn finish_reports_worker_panic() {
        let mut engine = ShardedEngine::new(1, 1).unwrap();

        // Shut down the original worker before injecting a failing one.
        engine.senders.clear();
        assert!(engine.workers.pop().unwrap().join().is_ok());

        engine
            .workers
            .push(thread::spawn(|| panic!("injected worker failure")));

        assert!(matches!(
            engine.finish(),
            Err(ShardedError::WorkerPanicked(0))
        ));
    }

    #[test]
    fn submit_reports_disconnected_worker() {
        let mut engine = ShardedEngine::new(1, 1).unwrap();

        let (sender, receiver) = mpsc::sync_channel(1);
        drop(receiver);

        // Replace the sender with a channel whose receiver is gone.
        engine.senders[0] = sender;

        let result = engine.submit(Query {
            transaction_type: TransactionType::Deposit,
            client_id: 1,
            tx_id: 1,
            amount: Some(Decimal::ONE),
        });

        assert!(matches!(result, Err(ShardedError::Disconnected(0))));

        engine.finish().unwrap();
    }
}
