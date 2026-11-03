use payments_engine::{engine::Engine, run};
use rust_decimal_macros::dec;
#[test]
fn malformed_row_is_skipped_and_processing_continues() {
    let data = "type,client,tx,amount\n\
                deposit,1,1,1.0\n\
                deposit,1,2,\n\
                garbage\n\
                deposit,1,3,2.0\n";
    let mut engine = Engine::default();
    run(data.as_bytes(), &mut engine).unwrap();
    assert_eq!(engine.clients()[&1].available(), dec!(3.0));
}

// this test recieves the output csv and checks if its is correct
#[test]
fn full_input_output_test() {
    let data = "type,client,tx,amount\n\
                deposit, 1, 1, 1.0\n\
                deposit, 2, 2, 2.0\n\
                deposit, 1, 3, 2.0\n\
                withdrawal, 1, 4, 1.5\n
                withdrawal, 2, 5, 3.0\n";
    let mut engine = Engine::default();
    run(data.as_bytes(), &mut engine).unwrap();
    let mut output = Vec::new();
    payments_engine::output::write_accounts(&mut output, engine.clients()).unwrap();
    let output_str = String::from_utf8(output).unwrap();
    let expected_output = "client,available,held,total,locked\n\
                           1,1.5000,0.0000,1.5000,false\n\
                           2,2.0000,0.0000,2.0000,false\n";
    let mut actual_lines = output_str.lines();
    let mut expected_lines = expected_output.lines();

    assert_eq!(actual_lines.next(), expected_lines.next());

    let mut actual_rows: Vec<_> = actual_lines.collect();
    let mut expected_rows: Vec<_> = expected_lines.collect();

    actual_rows.sort_unstable();
    expected_rows.sort_unstable();

    assert_eq!(actual_rows, expected_rows);
}

#[test]
fn huge_amount_is_rejected_and_output_does_not_panic() {
    let string = "type,client,tx,amount\n\
                deposit,1,1,79228162514264337593543950335\n\
                deposit,2,2,1.0\n";
    let mut engine = Engine::default();
    run(string.as_bytes(), &mut engine).unwrap();

    let mut out = Vec::new();
    payments_engine::output::write_accounts(&mut out, engine.clients()).unwrap();
    let out = String::from_utf8(out).unwrap();

    assert!(out.contains("2,1.0000,0.0000,1.0000,false"));
    assert!(!out.contains("79228162514264337593543950335"));
}

#[test]
fn invalid_utf8_row_is_skipped_and_processing_continues() {
    let mut data = b"type,client,tx,amount\ndeposit,1,1,1.0\n".to_vec();
    data.extend_from_slice(b"deposit,2,2,\xff\xfe\n"); // bytes que no son UTF-8
    data.extend_from_slice(b"deposit,1,3,2.0\n");

    let mut engine = Engine::default();
    run(data.as_slice(), &mut engine).unwrap();

    assert_eq!(engine.clients()[&1].available(), dec!(3.0));
}
