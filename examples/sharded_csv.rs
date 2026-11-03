use std::{env, error::Error, fs::File, io};

use payments_engine::{RunError, output::write_accounts, parser::parse, sharded::ShardedEngine};

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = env::args_os().skip(1);

    let path = args
        .next()
        .ok_or("Usage: sharded_csv <input.csv> <workers>")?;

    let workers = args
        .next()
        .ok_or("Missing worker count")?
        .into_string()
        .map_err(|_| "Worker count must be valid UTF-8")?
        .parse::<usize>()?;

    if args.next().is_some() {
        return Err("Usage: sharded_csv <input.csv> <workers>".into());
    }

    let mut reader = csv::ReaderBuilder::new()
        .trim(csv::Trim::All)
        .flexible(true)
        .from_reader(File::open(path)?);

    if !reader
        .headers()?
        .iter()
        .eq(["type", "client", "tx", "amount"])
    {
        return Err(RunError::InvalidHeader.into());
    }

    let mut engine = ShardedEngine::new(workers, 64)?;
    let mut record = csv::StringRecord::new();

    loop {
        match reader.read_record(&mut record) {
            Ok(false) => break,
            Ok(true) => match parse(&record) {
                Ok(query) => engine.submit(query)?,
                Err(error) => {
                    eprintln!("Invalid record at {:?}: {error}", record.position());
                }
            },
            Err(error) if matches!(error.kind(), csv::ErrorKind::Utf8 { .. }) => {
                eprintln!("Invalid record: {error}");
            }
            Err(error) => return Err(error.into()),
        }
    }

    let result = engine.finish()?;

    for rejection in result.rejected {
        eprintln!(
            "Rejected submission {}, client {}, tx {}: {}",
            rejection.sequence, rejection.client_id, rejection.tx_id, rejection.error
        );
    }

    write_accounts(io::stdout().lock(), &result.clients)?;

    Ok(())
}
