//! Fixed checksum-valid malformed save bodies from native packet0.
use super::*;

fn check_case(name: &str) {
    let case = include_str!("../../../../reverse-engineering/malformed-save-body-fixtures.txt")
        .lines()
        .map(|line| line.split_ascii_whitespace().collect::<Vec<_>>())
        .find(|case| case[0] == name)
        .unwrap();
    let packet = hex::decode(case[2]).unwrap();
    let raw = hex::decode(case[1]).unwrap();
    assert_eq!(SaveData::deobfuscate(&packet).unwrap(), raw);
    let error = SaveData::decode(&packet).expect_err("malformed body must return an error");
    assert!(!error.is::<UnsupportedSaveVersion>());
}

#[test]
fn crc_valid_bad_persistent_count_returns_error() {
    check_case("persistent-count");
}

#[test]
fn crc_valid_bad_index_count_returns_error() {
    check_case("index-count");
}

#[test]
fn crc_valid_bad_timestamp_returns_error() {
    for name in [
        "month-zero",
        "month-thirteen",
        "day-zero",
        "hour-twenty-four",
        "minute-sixty",
        "second-sixty",
    ] {
        check_case(name);
    }
}

#[test]
fn crc_valid_truncated_body_returns_error() {
    check_case("truncated-body");
}
