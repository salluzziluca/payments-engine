pub mod engine;
pub mod output;
pub mod parser;

use std::io::Read;

use crate::{engine::Engine, parser::parse};

pub fn run<R: Read>(input: R, engine: &mut Engine) -> Result<(), csv::Error> {
    let mut reader = csv::ReaderBuilder::new()
        .trim(csv::Trim::All)
        .flexible(true)
        .from_reader(input);

    for result in reader.records() {
        let record = result?;

        let query = match parse(&record) {
            Ok(query) => query,
            Err(error) => {
                eprintln!("Invalid record: {error}");
                continue;
            }
        };

        if let Err(error) = engine.process_transaction(query) {
            eprintln!("Transaction rejected: {error}");
        }
    }

    Ok(())
}
