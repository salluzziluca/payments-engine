// lib.rs
pub mod engine;
pub mod output;
pub mod parser;

// src/lib.rs (o un módulo aparte, por ejemplo run.rs)
use std::io::Read;

use crate::{engine::Engine, parser::parse};

pub fn run<R: Read>(input: R, engine: &mut Engine) {
    let mut reader = csv::ReaderBuilder::new()
        .trim(csv::Trim::All)
        .flexible(true)
        .from_reader(input);

    for result in reader.records() {
        let record = match result {
            Ok(record) => record,
            Err(e) => {
                eprintln!("unreadable row: {e}");
                continue;
            }
        };
        let query = match parse(&record) {
            Ok(query) => query,
            Err(e) => {
                eprintln!("error parsing record: {e}");
                continue;
            }
        };
        if let Err(e) = engine.process_transaction(query) {
            eprintln!("transaction rejected: {e}");
        }
    }
    let stdout = std::io::stdout();
    if let Err(error) = output::write_accounts(stdout.lock(), &engine.clients) {
        eprintln!("Failed to write output: {error}");
        std::process::exit(1);
    }
}
