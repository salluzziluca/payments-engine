use rust_decimal::Decimal;

use crate::parser::{Query, TransactionType};

use std::collections::HashMap;

#[derive(Debug, PartialEq)]
pub enum TxState {
    Normal,
    Disputed,
    ChargedBack,
}

pub struct TxRecord {
    client_id: u16,
    amount: Decimal,
    state: TxState,
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

#[derive(Default, Debug)]
pub struct Account {
    available: Decimal,
    held: Decimal,
    locked: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum AccountError {
    #[error("account is locked")]
    AccountLocked,
    #[error("amount must be positive")]
    InvalidAmount,
    #[error("arithmetic overflow")]
    Overflow,
    #[error("not enough balance to withdraw")]
    NotEnoughBalance,
    #[error("transaction already in dispute")]
    TxAlreadyDisputed,
}

impl Account {
    pub fn available(&self) -> Decimal {
        self.available
    }

    pub fn held(&self) -> Decimal {
        self.held
    }

    pub fn total(&self) -> Decimal {
        self.available + self.held
    }

    pub fn is_locked(&self) -> bool {
        self.locked
    }

    /// Credits `amount` to the available funds.
    ///
    /// # Errors
    /// Fails if the account is locked, the amount is not positive,
    /// or the addition overflows.
    pub fn deposit(&mut self, amount: Decimal) -> Result<(), AccountError> {
        if self.locked {
            return Err(AccountError::AccountLocked);
        }
        if amount <= Decimal::ZERO {
            return Err(AccountError::InvalidAmount);
        }
        self.available = self
            .available
            .checked_add(amount)
            .ok_or(AccountError::Overflow)?;
        Ok(())
    }

    pub fn withdraw(&mut self, amount: Decimal) -> Result<(), AccountError> {
        if self.locked {
            return Err(AccountError::AccountLocked);
        }
        if amount <= Decimal::ZERO {
            return Err(AccountError::InvalidAmount);
        }

        if self.available < amount {
            return Err(AccountError::NotEnoughBalance);
        }

        self.available = self
            .available
            .checked_sub(amount)
            .ok_or(AccountError::Overflow)?;
        Ok(())
    }
    /// Sets the give transaction to Disputed, decreases available funds and increases held funds, waiting for the resolve or chargeback
    pub fn dispute(&mut self, transaction: &mut TxRecord) -> Result<(), AccountError> {
        if self.locked {
            return Err(AccountError::AccountLocked);
        }

        let available = self
            .available
            .checked_sub(transaction.amount)
            .ok_or(AccountError::Overflow)?;
        let held = self
            .held
            .checked_add(transaction.amount)
            .ok_or(AccountError::Overflow)?;
        self.available = available;
        self.held = held;
        transaction.state = TxState::Disputed;
        Ok(())
    }

    pub fn resolve(&mut self, transaction: &mut TxRecord) -> Result<(), AccountError> {
        if self.locked {
            return Err(AccountError::AccountLocked);
        }
        let available = self
            .available
            .checked_add(transaction.amount)
            .ok_or(AccountError::Overflow)?;
        let held = self
            .held
            .checked_sub(transaction.amount)
            .ok_or(AccountError::Overflow)?;
        self.available = available;
        self.held = held;
        transaction.state = TxState::Normal;

        Ok(())
    }
    pub fn chargeback(&mut self, transaction: &mut TxRecord) -> Result<(), AccountError> {
        if self.locked {
            return Err(AccountError::AccountLocked);
        }

        let held = self
            .held
            .checked_sub(transaction.amount)
            .ok_or(AccountError::Overflow)?;
        self.held = held;
        self.locked = true;
        transaction.state = TxState::ChargedBack;

        Ok(())
    }
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
                    .dispute(tx)?;
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
                    .resolve(tx)?;
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
                    .chargeback(tx)?;
            }
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn query(kind: TransactionType, client_id: u16, tx_id: u32, amount: Option<Decimal>) -> Query {
        Query {
            transaction_type: kind,
            client_id,
            tx_id,
            amount,
        }
    }

    // Deposits

    #[test]
    fn deposit_updates_balances() {
        let cases = [
            ("empty account", dec!(0), dec!(0), dec!(1.5), dec!(1.5)),
            ("existing balance", dec!(5), dec!(2), dec!(3), dec!(8)),
            (
                "four decimals",
                dec!(1),
                dec!(0),
                dec!(0.0001),
                dec!(1.0001),
            ),
            ("negative balance", dec!(-5), dec!(10), dec!(2), dec!(-3)),
        ];

        for (name, available, held, amount, expected) in cases {
            let mut account = Account {
                available,
                held,
                locked: false,
            };

            account.deposit(amount).expect(name);

            assert_eq!(account.available(), expected, "{name}");
            assert_eq!(account.held(), held, "{name}");
            assert_eq!(account.total(), expected + held, "{name}");
            assert!(!account.is_locked(), "{name}");
        }
    }

    #[test]
    fn deposit_rejects_invalid_amount_without_changing_account() {
        for amount in [dec!(0), dec!(-1)] {
            let mut account = Account {
                available: dec!(5),
                held: dec!(2),
                locked: false,
            };

            assert!(
                matches!(account.deposit(amount), Err(AccountError::InvalidAmount)),
                "amount: {amount}"
            );
            assert_eq!(account.available(), dec!(5), "amount: {amount}");
            assert_eq!(account.held(), dec!(2), "amount: {amount}");
            assert!(!account.is_locked(), "amount: {amount}");
        }
    }

    #[test]
    fn deposit_rejects_locked_account() {
        let mut account = Account {
            available: dec!(5),
            held: dec!(2),
            locked: true,
        };

        assert!(matches!(
            account.deposit(dec!(1)),
            Err(AccountError::AccountLocked)
        ));
        assert_eq!(account.available(), dec!(5));
        assert_eq!(account.held(), dec!(2));
        assert!(account.is_locked());
    }

    #[test]
    fn deposit_overflow_preserves_account() {
        let mut account = Account {
            available: Decimal::MAX,
            held: Decimal::ZERO,
            locked: false,
        };

        assert!(matches!(
            account.deposit(dec!(1)),
            Err(AccountError::Overflow)
        ));
        assert_eq!(account.available(), Decimal::MAX);
        assert_eq!(account.held(), Decimal::ZERO);
        assert!(!account.is_locked());
    }

    // Withdrawals

    #[test]
    fn withdraw_updates_balances() {
        let cases = [
            ("partial", dec!(5), dec!(2), dec!(1.5), dec!(3.5)),
            ("exact balance", dec!(3), dec!(2), dec!(3), dec!(0)),
            (
                "four decimals",
                dec!(1),
                dec!(0),
                dec!(0.0001),
                dec!(0.9999),
            ),
        ];

        for (name, available, held, amount, expected) in cases {
            let mut account = Account {
                available,
                held,
                locked: false,
            };

            account.withdraw(amount).expect(name);

            assert_eq!(account.available(), expected, "{name}");
            assert_eq!(account.held(), held, "{name}");
            assert_eq!(account.total(), expected + held, "{name}");
            assert!(!account.is_locked(), "{name}");
        }
    }

    #[test]
    fn withdraw_rejects_invalid_amount_without_changing_account() {
        for amount in [dec!(0), dec!(-1)] {
            let mut account = Account {
                available: dec!(5),
                held: dec!(2),
                locked: false,
            };

            assert!(
                matches!(account.withdraw(amount), Err(AccountError::InvalidAmount)),
                "amount: {amount}"
            );
            assert_eq!(account.available(), dec!(5), "amount: {amount}");
            assert_eq!(account.held(), dec!(2), "amount: {amount}");
            assert!(!account.is_locked(), "amount: {amount}");
        }
    }

    #[test]
    fn withdraw_rejects_insufficient_available_funds() {
        let cases = [
            ("empty balance", dec!(0)),
            ("insufficient balance", dec!(2)),
            ("negative balance", dec!(-1)),
        ];

        for (name, available) in cases {
            let mut account = Account {
                available,
                held: dec!(10),
                locked: false,
            };

            assert!(
                matches!(
                    account.withdraw(dec!(3)),
                    Err(AccountError::NotEnoughBalance)
                ),
                "{name}"
            );
            assert_eq!(account.available(), available, "{name}");
            assert_eq!(account.held(), dec!(10), "{name}");
            assert!(!account.is_locked(), "{name}");
        }
    }

    #[test]
    fn withdraw_rejects_locked_account() {
        let mut account = Account {
            available: dec!(5),
            held: dec!(2),
            locked: true,
        };

        assert!(matches!(
            account.withdraw(dec!(1)),
            Err(AccountError::AccountLocked)
        ));
        assert_eq!(account.available(), dec!(5));
        assert_eq!(account.held(), dec!(2));
        assert!(account.is_locked());
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
            engine.clients.insert(
                1,
                Account {
                    available: dec!(5),
                    held: dec!(2),
                    locked: false,
                },
            );

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

        engine.clients.get_mut(&1).unwrap().locked = true;

        assert!(matches!(
            engine.process_transaction(query(TransactionType::Dispute, 1, 1, None)),
            Err(EngineError::Account(AccountError::AccountLocked))
        ));

        let account = &engine.clients[&1];

        assert_eq!(account.available(), dec!(5));
        assert_eq!(account.held(), dec!(0));
        assert!(account.is_locked());
        assert_eq!(engine.txs[&1].state, TxState::Normal);
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
    fn dispute_overflow_preserves_balances_and_transaction_state() {
        let cases = [
            ("available subtraction", Decimal::MIN, Decimal::ZERO),
            ("held addition", dec!(5), Decimal::MAX),
        ];

        for (name, available, held) in cases {
            let mut engine = Engine::default();

            engine
                .process_transaction(query(TransactionType::Deposit, 1, 1, Some(dec!(1))))
                .expect(name);

            // Artificial boundary values to exercise arithmetic failures.
            let account = engine.clients.get_mut(&1).unwrap();
            account.available = available;
            account.held = held;

            assert!(
                matches!(
                    engine.process_transaction(query(TransactionType::Dispute, 1, 1, None)),
                    Err(EngineError::Account(AccountError::Overflow))
                ),
                "{name}"
            );

            let account = &engine.clients[&1];

            assert_eq!(account.available(), available, "{name}");
            assert_eq!(account.held(), held, "{name}");
            assert!(!account.is_locked(), "{name}");
            assert_eq!(engine.txs[&1].state, TxState::Normal, "{name}");
        }
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
