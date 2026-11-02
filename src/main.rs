use std::{env, error::Error, fs::File};

use payments_engine::{engine::Engine, run};
fn main() -> Result<(), Box<dyn Error>> {
    let file_path = env::args().nth(1).ok_or("Missing file path argument")?;
    let file = File::open(&file_path)?;

    let mut engine = Engine::default();
    run(file, &mut engine);

    Ok(())
}
