use std::{env, error::Error, fs::File, io};

use payments_engine::{engine::Engine, output, run};

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = env::args_os().skip(1);

    let file_path = args
        .next()
        .ok_or("Usage: payments_engine <transactions.csv>")?;

    if args.next().is_some() {
        return Err("Usage: payments_engine <transactions.csv>".into());
    }

    let file = File::open(file_path)?;
    let mut engine = Engine::default();

    run(file, &mut engine)?;

    let stdout = io::stdout();
    output::write_accounts(stdout.lock(), engine.clients())?;

    Ok(())
}
