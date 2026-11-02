use rust_decimal::Decimal;

use crate::{
    account::{Account, AccountError},
    parser::{Query, TransactionType},
};

use std::collections::HashMap;

#[derive(Debug, PartialEq)]
pub enum TxState {
    Normal,
    Disputed,
    ChargedBack,
}

pub struct TxRecord {
    client_id: u16,
    pub(crate) amount: Decimal,
    pub(crate) state: TxState,
}
#[derive(Default)]
pub struct Engine {
    pub clients: HashMap<u16, Account>,
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
            assert_eq!(account.total(), expected, "client: {client_id}");
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
        assert_eq!(engine.clients[&1].total(), Decimal::ZERO);
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
        assert_eq!(account.total(), dec!(5));
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
                account.total(),
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
        assert_eq!(account.total(), dec!(8));
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
        assert_eq!(account.total(), dec!(8));

        for tx_id in [1, 2] {
            assert_eq!(engine.txs[&tx_id].state, TxState::Disputed, "tx: {tx_id}");
        }
    }

    #[test]

    fn dispute_rejects_locked_account_without_changing_state() {
        let mut engine = Engine::default();

        engine
            .process_transaction(query(TransactionType::Deposit, 1, 1, Some(dec!(5))))
            .unwrap();

        lock_account(&mut engine, 1, 2);

        assert!(matches!(
            engine.process_transaction(query(TransactionType::Dispute, 1, 1, None)),
            Err(EngineError::Account(AccountError::AccountLocked))
        ));

        let account = &engine.clients[&1];

        assert_eq!(account.available(), dec!(5));
        assert_eq!(account.held(), dec!(0));
        assert_eq!(account.total(), dec!(5));
        assert!(account.is_locked());
        assert_eq!(engine.txs[&1].state, TxState::Normal);
        assert_eq!(engine.txs[&2].state, TxState::ChargedBack);
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
        assert_eq!(account.total(), dec!(3));
        assert!(!account.is_locked());
        assert_eq!(engine.txs[&1].state, TxState::Normal);

        if let Some(tx) = engine.txs.get(&2) {
            assert_eq!(tx.state, TxState::Normal);
        }
    }
}
