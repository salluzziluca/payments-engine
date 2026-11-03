use rust_decimal::Decimal;

/// max client total balance to prevent overflows
///
/// Keeps every balance inside the range `Decimal` can print with four
/// decimals (formatting panics at 28+ digits)
/// Rejects absurd amounts early.
pub const MAX_BALANCE: Decimal = Decimal::from_parts(0xA764_0000, 0x0DE0_B6B3, 0, false, 0);
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
}

impl Account {
    pub fn available(&self) -> Decimal {
        self.available
    }

    pub fn held(&self) -> Decimal {
        self.held
    }

    pub fn total(&self) -> Result<Decimal, AccountError> {
        self.available
            .checked_add(self.held)
            .ok_or(AccountError::Overflow)
    }

    pub fn is_locked(&self) -> bool {
        self.locked
    }

    /// Credits `amount` to the available funds.
    ///
    /// # Errors
    /// Fails if the account is locked, the amount is not positive,
    /// or the addition overflows.
    pub(crate) fn deposit(&mut self, amount: Decimal) -> Result<(), AccountError> {
        if self.is_locked() {
            return Err(AccountError::AccountLocked);
        }
        if amount <= Decimal::ZERO {
            return Err(AccountError::InvalidAmount);
        }
        let new_total = self
            .total()?
            .checked_add(amount)
            .ok_or(AccountError::Overflow)?;
        if new_total > MAX_BALANCE {
            return Err(AccountError::Overflow);
        }
        self.available = self
            .available
            .checked_add(amount)
            .ok_or(AccountError::Overflow)?;
        Ok(())
    }

    pub(crate) fn withdraw(&mut self, amount: Decimal) -> Result<(), AccountError> {
        if self.is_locked() {
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
    /// Updates the account to reflect a dispute on a transaction of the given `amount`.
    ///
    /// Moves the `amount` from available to held funds.
    ///
    /// Doesnt check if the account is locked, because disputes can be raised on locked accounts.
    ///
    /// # Errors
    /// Fails if the operation would overflow.
    pub(crate) fn dispute(&mut self, amount: Decimal) -> Result<(), AccountError> {
        let available = self
            .available
            .checked_sub(amount)
            .ok_or(AccountError::Overflow)?;

        let held = self
            .held
            .checked_add(amount)
            .ok_or(AccountError::Overflow)?;

        self.available = available;
        self.held = held;
        Ok(())
    }
    /// Updates the account to reflect a resolution of a dispute on a transaction of the given `amount`.
    ///
    /// Moves the `amount` from held to available funds.
    ///
    /// Doesnt check if the account is locked, because disputes can be resolved on locked accounts.
    ///
    /// # Errors
    /// Fails if the operation would overflow.
    pub(crate) fn resolve(&mut self, amount: Decimal) -> Result<(), AccountError> {
        let available = self
            .available
            .checked_add(amount)
            .ok_or(AccountError::Overflow)?;
        let held = self
            .held
            .checked_sub(amount)
            .ok_or(AccountError::Overflow)?;
        self.available = available;
        self.held = held;

        Ok(())
    }

    /// Updates the account to reflect a chargeback of a disputed transaction of the given `amount`.
    ///
    /// Moves the `amount` from held funds and locks the account.
    ///
    /// Doesnt check if the account is locked, because chargebacks can be raised on locked accounts.
    ///
    /// # Errors
    /// Fails if operation would overflow.
    pub(crate) fn chargeback(&mut self, amount: Decimal) -> Result<(), AccountError> {
        let held = self
            .held
            .checked_sub(amount)
            .ok_or(AccountError::Overflow)?;
        self.held = held;
        self.locked = true;

        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::account::{Account, AccountError};
    use rust_decimal_macros::dec;

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
            assert_eq!(account.total().unwrap(), expected + held, "{name}");
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
            assert_eq!(account.total().unwrap(), expected + held, "{name}");
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

    #[test]
    fn dispute_overflow_preserves_balances() {
        let cases = [
            ("available subtraction", Decimal::MIN, Decimal::ZERO),
            ("held addition", dec!(5), Decimal::MAX),
        ];

        for (name, available, held) in cases {
            let mut account = Account {
                available,
                held,
                locked: false,
            };

            assert!(
                matches!(account.dispute(dec!(1)), Err(AccountError::Overflow)),
                "{name}"
            );

            assert_eq!(account.available(), available, "{name}");
            assert_eq!(account.held(), held, "{name}");
            assert!(!account.is_locked(), "{name}");
        }
    }
    #[test]
    fn max_balance_is_ten_to_the_eighteen() {
        assert_eq!(MAX_BALANCE, Decimal::from(1_000_000_000_000_000_000i64));
    }

    #[test]
    fn resolve_overflow_preserves_balances() {
        let cases = [
            ("available addition", Decimal::MAX, dec!(1)),
            ("held subtraction", dec!(0), Decimal::MIN),
        ];

        for (name, available, held) in cases {
            let mut account = Account {
                available,
                held,
                locked: true,
            };

            assert!(
                matches!(account.resolve(dec!(1)), Err(AccountError::Overflow)),
                "{name}"
            );

            assert_eq!(account.available(), available, "{name}");
            assert_eq!(account.held(), held, "{name}");
            assert!(account.is_locked(), "{name}");
        }
    }

    #[test]
    fn chargeback_overflow_preserves_balances_and_lock_state() {
        for locked in [false, true] {
            let mut account = Account {
                available: dec!(5),
                held: Decimal::MIN,
                locked,
            };

            assert!(matches!(
                account.chargeback(dec!(1)),
                Err(AccountError::Overflow)
            ));

            assert_eq!(account.available(), dec!(5));
            assert_eq!(account.held(), Decimal::MIN);
            assert_eq!(account.is_locked(), locked);
        }
    }
}
