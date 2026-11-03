use payments_engine::{engine::Engine, output::write_accounts, run};
use rust_decimal_macros::dec;

fn assert_csv_matches(actual: &str, expected: &str) {
    let mut actual_lines = actual.lines();
    let mut expected_lines = expected.lines();

    assert_eq!(
        actual_lines.next(),
        expected_lines.next(),
        "CSV headers differ"
    );

    let mut actual_rows: Vec<_> = actual_lines.collect();
    let mut expected_rows: Vec<_> = expected_lines.collect();

    actual_rows.sort_unstable();
    expected_rows.sort_unstable();

    assert_eq!(actual_rows, expected_rows, "CSV rows differ");
}

fn assert_input_output(input: &str, expected: &str) {
    let mut engine = Engine::default();
    run(input.as_bytes(), &mut engine).unwrap();

    let mut output = Vec::new();
    write_accounts(&mut output, engine.clients()).unwrap();

    let actual = String::from_utf8(output).unwrap();
    assert_csv_matches(&actual, expected);
}

// Fixtures

#[test]
fn basic_transactions_match_expected_output() {
    assert_input_output(
        include_str!("data/basic.csv"),
        include_str!("data/basic_expected.csv"),
    );
}

#[test]
fn dispute_lifecycle_matches_expected_output() {
    assert_input_output(
        include_str!("data/disputes.csv"),
        include_str!("data/disputes_expected.csv"),
    );
}

// Robustness

#[test]
fn malformed_row_is_skipped_and_processing_continues() {
    let input = "\
type,client,tx,amount
deposit,1,1,1.0
deposit,1,2,
garbage
deposit,1,3,2.0
";

    let mut engine = Engine::default();
    run(input.as_bytes(), &mut engine).unwrap();

    assert_eq!(engine.clients()[&1].available(), dec!(3));
}

#[test]
fn huge_amount_is_rejected_and_output_does_not_panic() {
    let input = "\
type,client,tx,amount
deposit,1,1,79228162514264337593543950335
deposit,2,2,1.0
";

    let mut engine = Engine::default();
    run(input.as_bytes(), &mut engine).unwrap();

    // The rejected deposit must not credit any funds.
    if let Some(account) = engine.clients().get(&1) {
        assert_eq!(account.available(), dec!(0));
        assert_eq!(account.held(), dec!(0));
    }

    assert_eq!(engine.clients()[&2].available(), dec!(1));

    let mut output = Vec::new();
    write_accounts(&mut output, engine.clients()).unwrap();

    let output = String::from_utf8(output).unwrap();

    assert!(
        output
            .lines()
            .any(|line| { line == "2,1.0000,0.0000,1.0000,false" })
    );
    assert!(!output.contains("79228162514264337593543950335"));
}

#[test]
fn invalid_utf8_row_is_skipped_and_processing_continues() {
    let mut input = b"type,client,tx,amount\ndeposit,1,1,1.0\n".to_vec();

    input.extend_from_slice(b"deposit,2,2,\xff\xfe\n");
    input.extend_from_slice(b"deposit,1,3,2.0\n");

    let mut engine = Engine::default();
    run(input.as_slice(), &mut engine).unwrap();

    assert_eq!(engine.clients()[&1].available(), dec!(3));
    assert!(!engine.clients().contains_key(&2));
}
#[test]
#[ignore = "large dataset; run explicitly"]
fn million_deposits_produce_expected_output() {
    use std::{
        fs::{self, File, OpenOptions},
        io::{BufWriter, Write},
        path::PathBuf,
        time::{Instant, SystemTime, UNIX_EPOCH},
    };

    const TRANSACTIONS: u32 = 1_000_000;
    const CLIENTS: u32 = 1000;

    struct TemporaryInput(PathBuf);

    impl Drop for TemporaryInput {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }

    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();

    let path = std::env::temp_dir().join(format!(
        "payments-benchmark-{}-{unique}.csv",
        std::process::id()
    ));

    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .unwrap();

    let temporary_input = TemporaryInput(path);

    // Generate the input before measuring processing.
    {
        let mut writer = BufWriter::new(file);
        writeln!(writer, "type,client,tx,amount").unwrap();

        for tx in 1..=TRANSACTIONS {
            writeln!(writer, "deposit,{},{tx},0.0001", tx % CLIENTS).unwrap();
        }

        writer.flush().unwrap();
    }

    let input = File::open(&temporary_input.0).unwrap();
    let mut engine = Engine::default();

    let processing_started = Instant::now();
    run(input, &mut engine).unwrap();
    let processing_time = processing_started.elapsed();

    assert_eq!(engine.clients().len(), CLIENTS as usize);

    let mut output = Vec::new();

    let output_started = Instant::now();
    write_accounts(&mut output, engine.clients()).unwrap();
    let output_time = output_started.elapsed();

    let actual = String::from_utf8(output).unwrap();

    let mut expected = String::from("client,available,held,total,locked\n");

    for client in 0..CLIENTS {
        expected.push_str(&format!("{client},0.1000,0.0000,0.1000,false\n"));
    }

    assert_csv_matches(&actual, &expected);

    eprintln!(
        "\nTransactions: {TRANSACTIONS}\n\
         Accounts: {}\n\
         Processing: {:.3} s\n\
         Output: {:.3} s\n\
         Throughput: {:.0} transactions/s",
        engine.clients().len(),
        processing_time.as_secs_f64(),
        output_time.as_secs_f64(),
        f64::from(TRANSACTIONS) / processing_time.as_secs_f64(),
    );
}
