pub mod account;
pub mod engine;
pub mod output;
pub mod parser;

use std::io::Read;

use crate::{engine::Engine, parser::parse};

/// A row that is not valid UTF-8 is a bad row, not a broken stream:
/// skip it and keep reading.
/// Anything else (I/O failure) means the input itself is unreliable.
pub fn run<R: Read>(input: R, engine: &mut Engine) -> Result<(), csv::Error> {
    let mut reader = csv::ReaderBuilder::new()
        .trim(csv::Trim::All)
        .flexible(true)
        .from_reader(input);

    for result in reader.records() {
        let record = match result {
            Ok(record) => record,
            Err(error) if matches!(error.kind(), csv::ErrorKind::Utf8 { .. }) => {
                eprintln!("Invalid record: {error}");
                continue;
            }
            Err(error) => return Err(error),
        };
        let line = record.position().map(|position| position.line());
        let query = match parse(&record) {
            Ok(query) => query,
            Err(error) => {
                eprintln!("Invalid record at line {line:?}: {error}");
                continue;
            }
        };
        let client_id = query.client_id;
        let tx_id = query.tx_id;

        if let Err(error) = engine.process_transaction(query) {
            eprintln!(
                "Transaction rejected at line {line:?}, \
         client {client_id}, tx {tx_id}: {error}"
            );
        }
    }

    Ok(())
}
