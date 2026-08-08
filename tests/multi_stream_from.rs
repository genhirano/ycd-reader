mod common;

use std::io;

use common::{collect_multi, collect_multi_from, golden_digits, one_million_path, TempYcd};
use ycd_reader::YcdMultiFileStream;

#[test]
fn multi_new_from_position_1_matches_new() {
    // new_from at position 1 should match new for a multi-file list.
    let paths: Vec<String> = (0..3).map(one_million_path).collect();
    let from_new = collect_multi(&paths, 1000).unwrap();
    let from_new_from = collect_multi_from(&paths, 1000, 1).unwrap();
    assert_eq!(from_new, from_new_from);
}

#[test]
fn multi_new_from_mid_first_file() {
    // Start mid-first-file and verify output matches golden from that position.
    let golden = golden_digits();
    let paths: Vec<String> = (0..3).map(one_million_path).collect();
    let start: i64 = 500_123;
    let result = collect_multi_from(&paths, 1000, start).unwrap();
    let expected = &golden[(start as usize - 1)..3_000_000];
    assert_eq!(result, expected);
}

#[test]
fn multi_new_from_at_file_boundary() {
    // Start exactly at the first digit of the second file.
    let golden = golden_digits();
    let paths: Vec<String> = (0..3).map(one_million_path).collect();
    let start: i64 = 1_000_001; // digit_start of file #1
    let result = collect_multi_from(&paths, 1000, start).unwrap();
    let expected = &golden[(start as usize - 1)..3_000_000];
    assert_eq!(result, expected);
}

#[test]
fn multi_new_from_mid_second_file() {
    let golden = golden_digits();
    let paths: Vec<String> = (0..3).map(one_million_path).collect();
    let start: i64 = 1_500_000;
    let result = collect_multi_from(&paths, 1000, start).unwrap();
    let expected = &golden[(start as usize - 1)..3_000_000];
    assert_eq!(result, expected);
}

#[test]
fn multi_new_from_last_file_start() {
    // Start at the beginning of the last file.
    let golden = golden_digits();
    let paths: Vec<String> = (0..3).map(one_million_path).collect();
    let start: i64 = 2_000_001;
    let result = collect_multi_from(&paths, 1000, start).unwrap();
    let expected = &golden[(start as usize - 1)..3_000_000];
    assert_eq!(result, expected);
}

#[test]
fn multi_new_from_last_digit() {
    let golden = golden_digits();
    let paths: Vec<String> = (0..3).map(one_million_path).collect();
    let start: i64 = 3_000_000;
    let result = collect_multi_from(&paths, 19, start).unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result, &golden[2_999_999..3_000_000]);
}

#[test]
fn multi_new_from_crossing_file_boundary() {
    // Unit size = 1000, start near the end of file 0 so the first unit crosses
    // into file 1.
    let golden = golden_digits();
    let paths: Vec<String> = (0..3).map(one_million_path).collect();
    let start: i64 = 999_700; // 300 digits left in file 0, then crosses into file 1
    let result = collect_multi_from(&paths, 1000, start).unwrap();
    let expected = &golden[(start as usize - 1)..3_000_000];
    assert_eq!(result, expected);
}

#[test]
fn multi_new_from_generated_files() {
    // Synthetic three-file list, start in the middle of the second file.
    let f0 = TempYcd::valid("1234567890123456789", 19, 0, "\n", "").unwrap();
    let f1 = TempYcd::valid("2345678901234567890", 19, 1, "\n", "").unwrap();
    let f2 = TempYcd::valid("3456789012345678901", 19, 2, "\n", "").unwrap();
    let all = "123456789012345678923456789012345678903456789012345678901";

    // Start at position 10 (mid f0)
    let result = collect_multi_from(&[&f0.path, &f1.path, &f2.path], 19, 10).unwrap();
    assert_eq!(result, &all[9..]);

    // Start at position 20 (start of f1)
    let result = collect_multi_from(&[&f0.path, &f1.path, &f2.path], 19, 20).unwrap();
    assert_eq!(result, &all[19..]);

    // Start at position 30 (mid f1)
    let result = collect_multi_from(&[&f0.path, &f1.path, &f2.path], 19, 30).unwrap();
    assert_eq!(result, &all[29..]);
}

#[test]
fn multi_new_from_errors() {
    let paths: Vec<String> = (0..3).map(one_million_path).collect();
    let empty: [&str; 0] = [];

    // Empty list
    assert_eq!(
        YcdMultiFileStream::new_from(&empty, 19, 1)
            .unwrap_err()
            .kind(),
        io::ErrorKind::InvalidInput
    );

    // start_position = 0
    assert_eq!(
        YcdMultiFileStream::new_from(&paths, 19, 0)
            .unwrap_err()
            .kind(),
        io::ErrorKind::InvalidInput
    );

    // start_position before list range
    assert_eq!(
        YcdMultiFileStream::new_from(&paths, 19, -1)
            .unwrap_err()
            .kind(),
        io::ErrorKind::InvalidInput
    );

    // start_position beyond list end
    assert_eq!(
        YcdMultiFileStream::new_from(&paths, 19, 3_000_001)
            .unwrap_err()
            .kind(),
        io::ErrorKind::InvalidInput
    );

    // invalid unit_size
    assert_eq!(
        YcdMultiFileStream::new_from(&paths, 1, 1)
            .unwrap_err()
            .kind(),
        io::ErrorKind::InvalidInput
    );

    // Non-contiguous files should still be rejected
    let f0 = TempYcd::valid("1234567890123456789", 19, 0, "\n", "").unwrap();
    let f2 = TempYcd::valid("1234567890123456789", 19, 2, "\n", "").unwrap();
    assert_eq!(
        YcdMultiFileStream::new_from(&[&f0.path, &f2.path], 19, 1)
            .unwrap_err()
            .kind(),
        io::ErrorKind::InvalidInput
    );
}
