pub mod account;
pub mod engine;
pub mod output;
pub mod parser;

use std::io::Read;

use crate::{engine::Engine, parser::parse};
#[derive(Debug, thiserror::Error)]
pub enum RunError {
    #[error("CSV input error: {0}")]
    Csv(#[from] csv::Error),

    #[error("invalid or missing CSV header; expected: type,client,tx,amount")]
    InvalidHeader,
}

/// Processes CSV records incrementally without loading the full input.
///
/// Requires the header: type,client,tx,amount.
/// Reuses a single record buffer to reduce per-row allocations.
///
/// Invalid UTF-8 data rows, parsing failures, and rejected transactions
/// are reported to stderr and skipped.
///
/// # Errors
/// Returns an error for an invalid or missing header, or a
/// non-recoverable CSV reading failure.
///
/// Transactions applied before a reading failure are not rolled back.
pub fn run<R: Read>(input: R, engine: &mut Engine) -> Result<(), RunError> {
    let mut reader = csv::ReaderBuilder::new()
        .trim(csv::Trim::All)
        .flexible(true)
        .from_reader(input);

    let expected = ["type", "client", "tx", "amount"];

    if !reader.headers()?.iter().eq(expected) {
        return Err(RunError::InvalidHeader);
    }

    // Reuse the record buffer instead of allocating a new one per row.
    let mut record = csv::StringRecord::new();

    loop {
        match reader.read_record(&mut record) {
            Ok(false) => break,
            Ok(true) => process_record(&record, engine),
            Err(error) if matches!(error.kind(), csv::ErrorKind::Utf8 { .. }) => {
                eprintln!("Invalid record: {error}");
            }
            Err(error) => return Err(error.into()),
        }
    }

    Ok(())
}

/// Parses and applies one record, reporting recoverable errors.
fn process_record(record: &csv::StringRecord, engine: &mut Engine) {
    let location = match record.position() {
        Some(position) => format!("line {}", position.line()),
        None => "unknown line".to_owned(),
    };

    let query = match parse(record) {
        Ok(query) => query,
        Err(error) => {
            eprintln!("Invalid record at {location}: {error}");
            return;
        }
    };

    let client_id = query.client_id;
    let tx_id = query.tx_id;

    if let Err(error) = engine.process_transaction(query) {
        eprintln!(
            "Transaction rejected at {location}, \
             client {client_id}, tx {tx_id}: {error}"
        );
    }
}
