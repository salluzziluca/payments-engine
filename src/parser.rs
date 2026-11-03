use rust_decimal::Decimal;

/// Everything left as pub just because this is a DTO
#[derive(Debug)]
pub struct Query {
    pub transaction_type: TransactionType,
    pub client_id: u16,
    pub tx_id: u32,
    pub amount: Option<Decimal>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TransactionType {
    Deposit,
    Withdrawal,
    Dispute,
    Resolve,
    Chargeback,
}
#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("missing field: {0}")]
    MissingField(&'static str),

    #[error("invalid transaction type: {0}")]
    InvalidTransactionType(String),

    #[error("invalid client ID: {0}")]
    InvalidClientId(#[source] std::num::ParseIntError),

    #[error("invalid transaction ID: {0}")]
    InvalidTxId(#[source] std::num::ParseIntError),

    #[error("invalid amount: {0}")]
    InvalidAmount(#[source] rust_decimal::Error),

    #[error("amount must have at most four decimal places")]
    InvalidPrecision,
}
pub fn parse(record: &csv::StringRecord) -> Result<Query, ParseError> {
    let transaction_type_str = record.get(0).ok_or(ParseError::MissingField("type"))?;

    let transaction_type = parse_transaction_type(transaction_type_str)?;

    let client_id = record
        .get(1)
        .ok_or(ParseError::MissingField("client"))?
        .parse::<u16>()
        .map_err(ParseError::InvalidClientId)?;

    let tx_id = record
        .get(2)
        .ok_or(ParseError::MissingField("tx"))?
        .parse::<u32>()
        .map_err(ParseError::InvalidTxId)?;

    let amount = match transaction_type {
        TransactionType::Deposit | TransactionType::Withdrawal => {
            let amount = record
                .get(3)
                .ok_or(ParseError::MissingField("amount"))?
                .parse::<Decimal>()
                .map_err(ParseError::InvalidAmount)?;

            if amount.normalize().scale() > 4 {
                return Err(ParseError::InvalidPrecision);
            }

            Some(amount)
        }
        _ => None,
    };

    Ok(Query {
        transaction_type,
        client_id,
        tx_id,
        amount,
    })
}

pub fn parse_transaction_type(input: &str) -> Result<TransactionType, ParseError> {
    match input {
        "deposit" => Ok(TransactionType::Deposit),
        "withdrawal" => Ok(TransactionType::Withdrawal),
        "dispute" => Ok(TransactionType::Dispute),
        "resolve" => Ok(TransactionType::Resolve),
        "chargeback" => Ok(TransactionType::Chargeback),
        _ => Err(ParseError::InvalidTransactionType(input.to_owned())),
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_transaction_type() {
        let cases = [
            ("deposit", TransactionType::Deposit),
            ("withdrawal", TransactionType::Withdrawal),
            ("dispute", TransactionType::Dispute),
            ("resolve", TransactionType::Resolve),
            ("chargeback", TransactionType::Chargeback),
        ];
        for (input, expected) in cases {
            assert_eq!(parse_transaction_type(input).unwrap(), expected);
        }
        assert!(parse_transaction_type("invalid").is_err());
    }

    #[test]
    fn test_parse() {
        let record = csv::StringRecord::from(vec!["deposit", "1", "1", "100.0"]);
        let query = parse(&record).unwrap();
        assert_eq!(query.transaction_type, TransactionType::Deposit);
        assert_eq!(query.client_id, 1);
        assert_eq!(query.tx_id, 1);
        assert_eq!(query.amount, Some(Decimal::from(100)));
    }
    #[test]
    fn test_parse_invalid_inputs() {
        let cases = [
            ("invalid type", vec!["invalid", "1", "1", "100.0"]),
            ("invalid client", vec!["deposit", "abc", "1", "100.0"]),
            ("invalid tx", vec!["deposit", "1", "abc", "100.0"]),
            ("invalid amount", vec!["deposit", "1", "1", "abc"]),
            ("missing amount", vec!["deposit", "1", "1"]),
        ];

        for (name, fields) in cases {
            let record = csv::StringRecord::from(fields);
            let result = parse(&record);

            assert!(result.is_err(), "Test case failed: {name}");
        }
    }
    #[test]
    fn test_parse_dispute_with_empty_amount() {
        // "dispute,1,1," -> 4 fields, the last one empty
        let record = csv::StringRecord::from(vec!["dispute", "1", "1", ""]);
        let query = parse(&record).unwrap();
        assert_eq!(query.transaction_type, TransactionType::Dispute);
        assert_eq!(query.amount, None);
    }

    #[test]
    fn test_parse_dispute_without_amount_field() {
        // "dispute,1,1" -> only 3 fields (needs .flexible(true) in the reader)
        let record = csv::StringRecord::from(vec!["dispute", "1", "1"]);
        let query = parse(&record).unwrap();
        assert_eq!(query.amount, None);
    }

    #[test]
    fn parse_validates_amount_precision() {
        let cases = [
            ("integer", "1", true),
            ("four decimals", "1.2345", true),
            ("trailing zero", "1.23450", true),
            ("smallest unit", "0.0001", true),
            ("extra precision", "1.23456", false),
            ("below smallest unit", "0.00001", false),
        ];

        for kind in ["deposit", "withdrawal"] {
            for (name, amount, valid) in cases {
                let record = csv::StringRecord::from(vec![kind, "1", "1", amount]);
                let result = parse(&record);

                assert_eq!(result.is_ok(), valid, "{kind}, {name}: {result:?}");

                if valid {
                    assert_eq!(
                        result.unwrap().amount,
                        Some(amount.parse::<Decimal>().unwrap()),
                        "{kind}, {name}"
                    );
                }
            }
        }
    }
    #[test]
    fn parse_returns_expected_errors() {
        use std::mem::discriminant;

        let cases = [
            ("missing type", vec![], ParseError::MissingField("type")),
            (
                "invalid type",
                vec!["invalid", "1", "1", "1"],
                ParseError::InvalidTransactionType("invalid".into()),
            ),
            (
                "missing client",
                vec!["deposit"],
                ParseError::MissingField("client"),
            ),
            (
                "missing tx",
                vec!["deposit", "1"],
                ParseError::MissingField("tx"),
            ),
            (
                "missing amount",
                vec!["deposit", "1", "1"],
                ParseError::MissingField("amount"),
            ),
            (
                "extra precision",
                vec!["deposit", "1", "1", "1.23456"],
                ParseError::InvalidPrecision,
            ),
        ];

        for (name, fields, expected) in cases {
            let record = csv::StringRecord::from(fields);
            let error = parse(&record).expect_err(name);

            assert_eq!(
                discriminant(&error),
                discriminant(&expected),
                "{name}: {error}"
            );

            if let ParseError::MissingField(expected_field) = expected {
                assert!(
                    matches!(error, ParseError::MissingField(field) if field == expected_field),
                    "{name}"
                );
            }
        }
    }
    #[test]
    fn parse_rejects_invalid_numeric_fields() {
        for client in ["abc", "-1", "65536"] {
            let record = csv::StringRecord::from(vec!["deposit", client, "1", "1"]);

            assert!(
                matches!(parse(&record), Err(ParseError::InvalidClientId(_))),
                "client: {client}"
            );
        }

        for tx in ["abc", "-1", "4294967296"] {
            let record = csv::StringRecord::from(vec!["deposit", "1", tx, "1"]);

            assert!(
                matches!(parse(&record), Err(ParseError::InvalidTxId(_))),
                "tx: {tx}"
            );
        }

        for amount in ["", "abc"] {
            let record = csv::StringRecord::from(vec!["deposit", "1", "1", amount]);

            assert!(
                matches!(parse(&record), Err(ParseError::InvalidAmount(_))),
                "amount: {amount:?}"
            );
        }
    }
}
