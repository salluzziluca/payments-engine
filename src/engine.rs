use rust_decimal::Decimal;

use crate::{
    account::{Account, AccountError},
    parser::{Query, TransactionType},
};

use std::collections::HashMap;

#[derive(Debug, PartialEq)]
enum TxState {
    Normal,
    Disputed,
    ChargedBack,
}

struct TxRecord {
    client_id: u16,
    amount: Decimal,
    state: TxState,
}
#[derive(Default)]
pub struct Engine {
    clients: HashMap<u16, Account>,
    txs: HashMap<u32, TxRecord>,
}
#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("transaction is missing an amount")]
    MissingAmount,

    #[error("account error: {0}")]
    Account(#[from] AccountError),
    #[error("transaction id already used")]
    DuplicateTx,
}

impl Engine {
    pub fn clients(&self) -> &HashMap<u16, Account> {
        &self.clients
    }
    pub fn process_transaction(&mut self, query: Query) -> Result<(), EngineError> {
        match query.transaction_type {
            TransactionType::Deposit => {
                if self.txs.contains_key(&query.tx_id) {
                    return Err(EngineError::DuplicateTx);
                }
                let amount = query.amount.ok_or(EngineError::MissingAmount)?;
                self.clients
                    .entry(query.client_id)
                    .or_default()
                    .deposit(amount)?;
                let transaction_record = TxRecord {
                    client_id: query.client_id,
                    amount: amount,
                    state: TxState::Normal,
                };
                self.txs.insert(query.tx_id, transaction_record);
            }
            TransactionType::Withdrawal => {
                let amount = query.amount.ok_or(EngineError::MissingAmount)?;
                self.clients
                    .entry(query.client_id)
                    .or_default()
                    .withdraw(amount)?;
            }
            TransactionType::Dispute => {
                let Some(tx) = self.txs.get_mut(&query.tx_id) else {
                    return Ok(());
                };
                if tx.client_id != query.client_id || !matches!(tx.state, TxState::Normal) {
                    return Ok(());
                }
                self.clients
                    .entry(query.client_id)
                    .or_default()
                    .dispute(tx.amount)?;
                tx.state = TxState::Disputed;
            }
            TransactionType::Resolve => {
                let Some(tx) = self.txs.get_mut(&query.tx_id) else {
                    return Ok(());
                };
                if tx.client_id != query.client_id || !matches!(tx.state, TxState::Disputed) {
                    return Ok(());
                }
                self.clients
                    .entry(query.client_id)
                    .or_default()
                    .resolve(tx.amount)?;
                tx.state = TxState::Normal;
            }
            TransactionType::Chargeback => {
                let Some(tx) = self.txs.get_mut(&query.tx_id) else {
                    return Ok(());
                };
                if tx.client_id != query.client_id || !matches!(tx.state, TxState::Disputed) {
                    return Ok(());
                }
                self.clients
                    .entry(query.client_id)
                    .or_default()
                    .chargeback(tx.amount)?;
                tx.state = TxState::ChargedBack;
            }
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {

    use super::*;
    use crate::account::{Account, AccountError};
    use rust_decimal_macros::dec;

    // Use a fresh tx_id so existing transactions remain unchanged.
    fn lock_account(engine: &mut Engine, client_id: u16, tx_id: u32) {
        let transactions = [
            query(TransactionType::Deposit, client_id, tx_id, Some(dec!(1))),
            query(TransactionType::Dispute, client_id, tx_id, None),
            query(TransactionType::Chargeback, client_id, tx_id, None),
        ];

        for transaction in transactions {
            engine.process_transaction(transaction).unwrap();
        }

        assert!(engine.clients[&client_id].is_locked());
    }
    fn engine_with_disputes(disputed: &[u32]) -> Engine {
        let mut engine = Engine::default();
        for (tx_id, amount) in [(1, dec!(10)), (2, dec!(5))] {
            engine
                .process_transaction(query(TransactionType::Deposit, 1, tx_id, Some(amount)))
                .unwrap();
        }
        for &tx_id in disputed {
            engine
                .process_transaction(query(TransactionType::Dispute, 1, tx_id, None))
                .unwrap();
        }
        engine
    }

    fn query(kind: TransactionType, client_id: u16, tx_id: u32, amount: Option<Decimal>) -> Query {
        Query {
            transaction_type: kind,
            client_id,
            tx_id,
            amount,
        }
    }

    // Engine
    #[test]
    fn process_deposit_creates_missing_client() {
        let mut engine = Engine::default();

        engine
            .process_transaction(query(TransactionType::Deposit, 1, 1, Some(dec!(2))))
            .unwrap();

        assert_eq!(engine.clients.len(), 1);
        assert_eq!(engine.clients[&1].available(), dec!(2));
        assert_eq!(engine.clients[&1].held(), Decimal::ZERO);
        assert!(!engine.clients[&1].is_locked());
    }

    #[test]
    fn process_transactions_preserves_order_and_isolates_clients() {
        let mut engine = Engine::default();
        let transactions = [
            query(TransactionType::Deposit, 1, 1, Some(dec!(5))),
            query(TransactionType::Deposit, 2, 2, Some(dec!(10))),
            query(TransactionType::Withdrawal, 1, 3, Some(dec!(1.5))),
            query(TransactionType::Deposit, 1, 4, Some(dec!(2))),
            query(TransactionType::Withdrawal, 2, 5, Some(dec!(3))),
        ];

        for transaction in transactions {
            engine.process_transaction(transaction).unwrap();
        }

        assert_eq!(engine.clients.len(), 2);

        for (client_id, expected) in [(1, dec!(5.5)), (2, dec!(7))] {
            let account = &engine.clients[&client_id];

            assert_eq!(account.available(), expected, "client: {client_id}");
            assert_eq!(account.held(), Decimal::ZERO, "client: {client_id}");
            assert_eq!(account.total().unwrap(), expected, "client: {client_id}");
            assert!(!account.is_locked(), "client: {client_id}");
        }
    }

    #[test]
    fn process_without_amount_creates_no_account() {
        let cases = [
            ("deposit", TransactionType::Deposit),
            ("withdrawal", TransactionType::Withdrawal),
        ];

        for (name, kind) in cases {
            let mut engine = Engine::default();

            assert!(
                matches!(
                    engine.process_transaction(query(kind, 1, 1, None)),
                    Err(EngineError::MissingAmount)
                ),
                "{name}"
            );
            assert!(engine.clients.is_empty(), "{name}");
        }
    }

    #[test]
    fn process_propagates_invalid_amount_and_preserves_balance() {
        let cases = [
            ("zero deposit", TransactionType::Deposit, dec!(0)),
            ("negative deposit", TransactionType::Deposit, dec!(-1)),
            ("zero withdrawal", TransactionType::Withdrawal, dec!(0)),
            ("negative withdrawal", TransactionType::Withdrawal, dec!(-1)),
        ];

        for (name, kind, amount) in cases {
            let mut engine = Engine::default();
            let mut account = Account::default();
            account.deposit(dec!(7)).unwrap();
            account.dispute(dec!(2)).unwrap();
            engine.clients.insert(1, account);

            assert!(
                matches!(
                    engine.process_transaction(query(kind, 1, 1, Some(amount))),
                    Err(EngineError::Account(AccountError::InvalidAmount))
                ),
                "{name}"
            );
            assert_eq!(engine.clients[&1].available(), dec!(5), "{name}");
            assert_eq!(engine.clients[&1].held(), dec!(2), "{name}");
        }
    }

    #[test]
    fn process_withdrawal_for_missing_client_creates_empty_account() {
        let mut engine = Engine::default();

        assert!(matches!(
            engine.process_transaction(query(TransactionType::Withdrawal, 1, 1, Some(dec!(1)),)),
            Err(EngineError::Account(AccountError::NotEnoughBalance))
        ));

        assert_eq!(engine.clients.len(), 1);
        assert_eq!(engine.clients[&1].total().unwrap(), Decimal::ZERO);
        assert!(!engine.clients[&1].is_locked());
    }

    // disputes
    #[test]
    fn process_dispute_moves_funds_to_held() {
        let mut engine = Engine::default();
        engine
            .process_transaction(query(TransactionType::Deposit, 1, 1, Some(dec!(5))))
            .unwrap();

        engine
            .process_transaction(query(TransactionType::Dispute, 1, 1, None))
            .unwrap();

        let account = &engine.clients[&1];
        assert_eq!(account.available(), dec!(0));
        assert_eq!(account.held(), dec!(5));
        assert_eq!(account.total().unwrap(), dec!(5));
    }
    #[test]
    fn dispute_table_driven_tests() {
        let cases = [
            // name, client, tx, withdrawal, already_disputed, available, held
            ("valid", 1, 1, dec!(0), false, dec!(0), dec!(5)),
            ("missing tx", 1, 99, dec!(0), false, dec!(5), dec!(0)),
            ("wrong client", 2, 1, dec!(0), false, dec!(5), dec!(0)),
            ("repeated", 1, 1, dec!(0), true, dec!(0), dec!(5)),
            ("funds spent", 1, 1, dec!(3), false, dec!(-3), dec!(5)),
        ];

        for (
            name,
            client_id,
            tx_id,
            withdrawal,
            already_disputed,
            expected_available,
            expected_held,
        ) in cases
        {
            let mut engine = Engine::default();
            engine
                .process_transaction(query(TransactionType::Deposit, 1, 1, Some(dec!(5))))
                .unwrap();

            if withdrawal > Decimal::ZERO {
                engine
                    .process_transaction(query(TransactionType::Withdrawal, 1, 2, Some(withdrawal)))
                    .expect(name);
            }

            if already_disputed {
                engine
                    .process_transaction(query(TransactionType::Dispute, 1, 1, None))
                    .expect(name);
            }
            engine
                .process_transaction(query(TransactionType::Dispute, client_id, tx_id, None))
                .expect(name);
            let account = &engine.clients[&1];
            assert_eq!(account.available(), expected_available, "case: {name}");
            assert_eq!(account.held(), expected_held, "case: {name}");
            assert_eq!(
                account.total().unwrap(),
                expected_available + expected_held,
                "case: {name}"
            );
            assert!(!account.is_locked(), "case: {name}");
            assert_eq!(engine.clients.len(), 1, "{name}");

            let expected_state = if already_disputed || (client_id == 1 && tx_id == 1) {
                TxState::Disputed
            } else {
                TxState::Normal
            };
            assert_eq!(engine.txs[&1].state, expected_state, "{name}");
        }
    }
    #[test]
    fn dispute_only_affects_referenced_deposit() {
        let mut engine = Engine::default();

        for (tx_id, amount) in [(1, dec!(5)), (2, dec!(3))] {
            engine
                .process_transaction(query(TransactionType::Deposit, 1, tx_id, Some(amount)))
                .unwrap();
        }

        engine
            .process_transaction(query(TransactionType::Dispute, 1, 1, None))
            .unwrap();

        let account = &engine.clients[&1];

        assert_eq!(account.available(), dec!(3));
        assert_eq!(account.held(), dec!(5));
        assert_eq!(account.total().unwrap(), dec!(8));
        assert_eq!(engine.txs[&1].state, TxState::Disputed);
        assert_eq!(engine.txs[&2].state, TxState::Normal);
    }

    #[test]
    fn disputes_accumulate_held_funds() {
        let mut engine = Engine::default();

        for (tx_id, amount) in [(1, dec!(5)), (2, dec!(3))] {
            engine
                .process_transaction(query(TransactionType::Deposit, 1, tx_id, Some(amount)))
                .unwrap();
        }

        for tx_id in [1, 2] {
            engine
                .process_transaction(query(TransactionType::Dispute, 1, tx_id, None))
                .unwrap();
        }

        let account = &engine.clients[&1];

        assert_eq!(account.available(), dec!(0));
        assert_eq!(account.held(), dec!(8));
        assert_eq!(account.total().unwrap(), dec!(8));

        for tx_id in [1, 2] {
            assert_eq!(engine.txs[&tx_id].state, TxState::Disputed, "tx: {tx_id}");
        }
    }

    #[test]
    fn dispute_applies_on_locked_account() {
        let mut engine = Engine::default();
        engine
            .process_transaction(query(TransactionType::Deposit, 1, 1, Some(dec!(5))))
            .unwrap();
        lock_account(&mut engine, 1, 2);

        engine
            .process_transaction(query(TransactionType::Dispute, 1, 1, None))
            .unwrap();

        let account = &engine.clients[&1];
        assert_eq!(account.available(), dec!(0));
        assert_eq!(account.held(), dec!(5));
        assert!(account.is_locked());
        assert_eq!(engine.txs[&1].state, TxState::Disputed);
    }

    #[test]
    fn dispute_ignores_charged_back_transaction() {
        let mut engine = Engine::default();

        engine
            .process_transaction(query(TransactionType::Deposit, 1, 1, Some(dec!(5))))
            .unwrap();

        // Set the state directly to test this guard independently
        // of account locking and chargeback processing.
        engine.txs.get_mut(&1).unwrap().state = TxState::ChargedBack;

        engine
            .process_transaction(query(TransactionType::Dispute, 1, 1, None))
            .unwrap();

        let account = &engine.clients[&1];

        assert_eq!(account.available(), dec!(5));
        assert_eq!(account.held(), dec!(0));
        assert!(!account.is_locked());
        assert_eq!(engine.txs[&1].state, TxState::ChargedBack);
    }

    #[test]
    fn dispute_ignores_withdrawal() {
        let mut engine = Engine::default();

        engine
            .process_transaction(query(TransactionType::Deposit, 1, 1, Some(dec!(5))))
            .unwrap();

        engine
            .process_transaction(query(TransactionType::Withdrawal, 1, 2, Some(dec!(2))))
            .unwrap();

        engine
            .process_transaction(query(TransactionType::Dispute, 1, 2, None))
            .unwrap();

        let account = &engine.clients[&1];

        assert_eq!(account.available(), dec!(3));
        assert_eq!(account.held(), dec!(0));
        assert_eq!(account.total().unwrap(), dec!(3));
        assert!(!account.is_locked());
        assert_eq!(engine.txs[&1].state, TxState::Normal);

        if let Some(tx) = engine.txs.get(&2) {
            assert_eq!(tx.state, TxState::Normal);
        }
    }

    //resolve

    #[test]
    fn resolve_releases_held_funds() {
        let mut engine = engine_with_disputes(&[1]);

        engine
            .process_transaction(query(TransactionType::Resolve, 1, 1, None))
            .unwrap();

        let account = &engine.clients[&1];
        assert_eq!(account.available(), dec!(15));
        assert_eq!(account.held(), dec!(0));
        assert_eq!(account.total().unwrap(), dec!(15));
        assert!(!account.is_locked());
        assert_eq!(engine.txs[&1].state, TxState::Normal);
    }

    #[test]
    fn resolve_is_ignored_when_it_does_not_apply() {
        // name, client, tx, disputed txs before the resolve
        let cases = [
            ("missing tx", 1, 99, vec![1]),
            ("wrong client", 2, 1, vec![1]),
            ("tx not under dispute", 1, 2, vec![1]),
        ];

        for (name, client_id, tx_id, disputed) in cases {
            let mut engine = engine_with_disputes(&disputed);

            engine
                .process_transaction(query(TransactionType::Resolve, client_id, tx_id, None))
                .expect(name);

            let account = &engine.clients[&1];
            assert_eq!(account.available(), dec!(5), "{name}");
            assert_eq!(account.held(), dec!(10), "{name}");
            assert_eq!(engine.txs[&1].state, TxState::Disputed, "{name}");
        }
    }

    #[test]
    fn resolved_transaction_can_be_disputed_again() {
        let mut engine = engine_with_disputes(&[1]);

        for kind in [TransactionType::Resolve, TransactionType::Dispute] {
            engine.process_transaction(query(kind, 1, 1, None)).unwrap();
        }

        assert_eq!(engine.clients[&1].held(), dec!(10));
        assert_eq!(engine.txs[&1].state, TxState::Disputed);
    }

    // chargeback

    #[test]
    fn chargeback_removes_held_funds_and_locks_account() {
        let mut engine = engine_with_disputes(&[1]);

        engine
            .process_transaction(query(TransactionType::Chargeback, 1, 1, None))
            .unwrap();

        let account = &engine.clients[&1];
        assert_eq!(account.available(), dec!(5));
        assert_eq!(account.held(), dec!(0));
        assert_eq!(account.total().unwrap(), dec!(5));
        assert!(account.is_locked());
        assert_eq!(engine.txs[&1].state, TxState::ChargedBack);
    }

    #[test]
    fn chargeback_is_ignored_when_it_does_not_apply() {
        let cases = [
            ("missing tx", 1, 99, vec![1]),
            ("wrong client", 2, 1, vec![1]),
            ("tx not under dispute", 1, 2, vec![1]),
        ];

        for (name, client_id, tx_id, disputed) in cases {
            let mut engine = engine_with_disputes(&disputed);

            engine
                .process_transaction(query(TransactionType::Chargeback, client_id, tx_id, None))
                .expect(name);

            let account = &engine.clients[&1];
            assert_eq!(account.held(), dec!(10), "{name}");
            assert!(!account.is_locked(), "{name}");
            assert_eq!(engine.txs[&1].state, TxState::Disputed, "{name}");
        }
    }

    #[test]
    fn locked_account_rejects_deposits_and_withdrawals() {
        let mut engine = engine_with_disputes(&[1]);
        engine
            .process_transaction(query(TransactionType::Chargeback, 1, 1, None))
            .unwrap();

        for kind in [TransactionType::Deposit, TransactionType::Withdrawal] {
            assert!(matches!(
                engine.process_transaction(query(kind, 1, 3, Some(dec!(1)))),
                Err(EngineError::Account(AccountError::AccountLocked))
            ));
        }
        assert_eq!(engine.clients[&1].available(), dec!(5));
    }

    #[test]
    fn locked_account_can_still_settle_other_disputes() {
        // Chargeback on tx 1 locks the account; tx 2 is still disputed and
        // must be able to finish, or its funds would stay held forever.
        let mut engine = engine_with_disputes(&[1, 2]);

        engine
            .process_transaction(query(TransactionType::Chargeback, 1, 1, None))
            .unwrap();
        engine
            .process_transaction(query(TransactionType::Resolve, 1, 2, None))
            .unwrap();

        let account = &engine.clients[&1];
        assert_eq!(account.available(), dec!(5));
        assert_eq!(account.held(), dec!(0));
        assert!(account.is_locked());
    }

    #[test]
    fn chargeback_cannot_be_repeated_or_resolved() {
        let mut engine = engine_with_disputes(&[1]);
        engine
            .process_transaction(query(TransactionType::Chargeback, 1, 1, None))
            .unwrap();

        for kind in [TransactionType::Chargeback, TransactionType::Resolve] {
            engine.process_transaction(query(kind, 1, 1, None)).unwrap();
        }

        assert_eq!(engine.clients[&1].available(), dec!(5));
        assert_eq!(engine.clients[&1].held(), dec!(0));
        assert_eq!(engine.txs[&1].state, TxState::ChargedBack);
    }
}
