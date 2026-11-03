use std::collections::HashMap;

use rust_decimal::Decimal;

use crate::account::Account;
use crate::account::AccountError;

#[derive(Debug, thiserror::Error)]
pub enum OutputError {
    #[error("CSV output error: {0}")]
    Csv(#[from] csv::Error),

    #[error("failed to flush output: {0}")]
    Io(#[from] std::io::Error),

    #[error("cannot calculate account total: {0}")]
    Account(#[from] AccountError),
}

/// Formats a decimal with exactly four decimal places without
/// increasing its internal scale, which could overflow for large values.
fn format_amount(amount: Decimal) -> String {
    let mut text = amount.round_dp(4).to_string();

    let decimal_places = match text.split_once('.') {
        Some((_, fraction)) => fraction.len(),
        None => {
            text.push('.');
            0
        }
    };

    for _ in decimal_places..4 {
        text.push('0');
    }

    text
}
pub fn write_accounts<W: std::io::Write>(
    writer: W,
    accounts: &HashMap<u16, Account>,
) -> Result<(), OutputError> {
    let mut csv = csv::Writer::from_writer(writer);

    csv.write_record(["client", "available", "held", "total", "locked"])?;
    for (client_id, account) in accounts {
        csv.write_record([
            client_id.to_string(),
            format_amount(account.available()),
            format_amount(account.held()),
            format_amount(account.total()?),
            account.is_locked().to_string(),
        ])?;
    }

    csv.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{self, Write};

    #[test]
    fn empty_accounts_writes_header() {
        let mut output = Vec::new();

        write_accounts(&mut output, &HashMap::new()).unwrap();

        assert_eq!(
            String::from_utf8(output).unwrap(),
            "client,available,held,total,locked\n"
        );
    }

    struct FailingWriter {
        fail_on_write: bool,
    }

    impl Write for FailingWriter {
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            if self.fail_on_write {
                Err(io::Error::other("write failed"))
            } else {
                Ok(buffer.len())
            }
        }

        fn flush(&mut self) -> io::Result<()> {
            Err(io::Error::other("flush failed"))
        }
    }

    #[test]
    fn output_propagates_writer_errors() {
        for fail_on_write in [true, false] {
            let writer = FailingWriter { fail_on_write };

            let error = write_accounts(writer, &HashMap::new())
                .expect_err("writer failure must be propagated");

            assert!(
                matches!(error, OutputError::Io(_)),
                "fail_on_write={fail_on_write}: {error}"
            );
        }
    }

    #[test]
    fn format_amount_produces_four_decimal_places() {
        use rust_decimal_macros::dec;

        let cases = [
            ("zero", dec!(0), "0.0000"),
            ("integer", dec!(5), "5.0000"),
            ("fraction", dec!(1.5), "1.5000"),
            ("four decimals", dec!(1.2345), "1.2345"),
            ("negative", dec!(-3.5), "-3.5000"),
            (
                "maximum decimal",
                Decimal::MAX,
                "79228162514264337593543950335.0000",
            ),
            (
                "minimum decimal",
                Decimal::MIN,
                "-79228162514264337593543950335.0000",
            ),
        ];

        for (name, amount, expected) in cases {
            assert_eq!(format_amount(amount), expected, "{name}");
        }
    }
}
