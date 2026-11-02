use std::error::Error;

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
pub fn parse(record: &csv::StringRecord) -> Result<Query, Box<dyn Error>> {
    let transaction_type_str = record.get(0).ok_or("Missing transaction type")?;
    let transaction_type = parse_transaction_type(transaction_type_str)?;

    let client_id = record.get(1).ok_or("Missing client ID")?.parse()?;
    let tx_id = record.get(2).ok_or("Missing transaction ID")?.parse()?;
    let amount = Some(record.get(3).ok_or("Missing amount")?.parse()?);

    Ok(Query {
        transaction_type,
        client_id,
        tx_id,
        amount,
    })
}

pub fn parse_transaction_type(input: &str) -> Result<TransactionType, Box<dyn Error>> {
    match input {
        "deposit" => Ok(TransactionType::Deposit),
        "withdrawal" => Ok(TransactionType::Withdrawal),
        "dispute" => Ok(TransactionType::Dispute),
        "resolve" => Ok(TransactionType::Resolve),
        "chargeback" => Ok(TransactionType::Chargeback),
        _ => Err(format!("Invalid transaction type: {}", input).into()),
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
}
