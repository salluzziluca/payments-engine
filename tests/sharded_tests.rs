use std::collections::HashMap;

use payments_engine::{
    account::Account,
    engine::Engine,
    output::write_accounts,
    parser::{Query, parse},
    sharded::ShardedEngine,
};

fn queries(input: &str) -> impl Iterator<Item = Query> + '_ {
    csv::ReaderBuilder::new()
        .trim(csv::Trim::All)
        .flexible(true)
        .from_reader(input.as_bytes())
        .into_records()
        .map(|record| parse(&record.unwrap()).unwrap())
}

fn account_rows(accounts: &HashMap<u16, Account>) -> Vec<String> {
    let mut output = Vec::new();
    write_accounts(&mut output, accounts).unwrap();

    let output = String::from_utf8(output).unwrap();
    let mut rows: Vec<_> = output.lines().skip(1).map(str::to_owned).collect();

    rows.sort_unstable();
    rows
}

fn assert_equivalent(input: &str) {
    let mut sequential = Engine::default();
    let mut expected_rejections = Vec::new();

    for (sequence, query) in queries(input).enumerate() {
        let client_id = query.client_id;
        let tx_id = query.tx_id;

        if let Err(error) = sequential.process_transaction(query) {
            expected_rejections.push((sequence, client_id, tx_id, error.to_string()));
        }
    }

    let expected_accounts = account_rows(sequential.clients());

    for workers in [1, 2, 4, 7] {
        for capacity in [0, 1, 16] {
            let mut engine = ShardedEngine::new(workers, capacity).unwrap();

            for query in queries(input) {
                engine.submit(query).unwrap();
            }

            let result = engine.finish().unwrap();

            assert_eq!(
                account_rows(&result.clients),
                expected_accounts,
                "workers={workers}, capacity={capacity}"
            );

            let actual_rejections: Vec<_> = result
                .rejected
                .into_iter()
                .map(|rejection| {
                    (
                        rejection.sequence,
                        rejection.client_id,
                        rejection.tx_id,
                        rejection.error.to_string(),
                    )
                })
                .collect();

            assert_eq!(
                actual_rejections, expected_rejections,
                "workers={workers}, capacity={capacity}"
            );
        }
    }
}

#[test]
fn basic_transactions_match_sequential_engine() {
    assert_equivalent(include_str!("data/basic.csv"));
}

#[test]
fn dispute_lifecycle_matches_sequential_engine() {
    assert_equivalent(include_str!("data/disputes.csv"));
}
#[test]
fn interleaved_clients_preserve_order_and_rejections() {
    assert_equivalent(
        "\
type,client,tx,amount
withdrawal,0,1,1
deposit,0,2,10
deposit,1,3,5
deposit,2,4,8
deposit,65535,4294967295,2
withdrawal,1,5,6
withdrawal,0,6,3
dispute,2,4,
dispute,0,2,
resolve,2,4,
resolve,0,2,
dispute,0,2,
deposit,1,7,4
dispute,1,3,
dispute,1,7,
chargeback,1,3,
resolve,1,7,
deposit,1,8,1
withdrawal,1,9,1
chargeback,0,2,
chargeback,0,2,
resolve,0,2,
dispute,2,4294967295,
dispute,2,999,
withdrawal,65535,10,2
deposit,2,11,-1
",
    );
}

#[test]
fn zero_workers_are_rejected() {
    use payments_engine::sharded::ShardedError;

    assert!(matches!(
        ShardedEngine::new(0, 1),
        Err(ShardedError::InvalidWorkerCount)
    ));
}

#[test]
fn empty_engine_finishes_without_accounts_or_rejections() {
    let result = ShardedEngine::new(4, 0).unwrap().finish().unwrap();

    assert!(result.clients.is_empty());
    assert!(result.rejected.is_empty());
}
