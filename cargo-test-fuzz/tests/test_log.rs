use std::{
    fs::{read_dir, File},
    io::{BufRead, BufReader},
    path::Path,
};
use test_log::test;

#[test]
fn all_tests_use_test_log() {
    let tests = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");

    for entry in read_dir(tests).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        let file = File::open(path).unwrap();
        assert!(BufReader::new(file)
            .lines()
            .any(|line| { line.unwrap() == "use test_log::test;" }));
    }
}
