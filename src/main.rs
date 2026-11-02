use std::{env, error::Error, fs::File, io};

use payments_engine::{engine::Engine, output, run};

fn main() -> Result<(), Box<dyn Error>> {
    let file_path = env::args()
        .nth(1)
        .ok_or("Usage: payments_engine <transactions.csv>")?;

    let file = File::open(file_path)?;
    let mut engine = Engine::default();

    run(file, &mut engine)?;

    let stdout = io::stdout();
    output::write_accounts(stdout.lock(), &engine.clients)?;

    Ok(())
}
