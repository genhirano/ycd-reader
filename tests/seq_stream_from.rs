mod common;

use std::io;

use common::{collect_single, collect_single_from, golden_digits, one_million_path, TempYcd};
use ycd_reader::YcdSeqBlockStream;

#[test]
fn seq_new_from_position_1_matches_new() {
    // new_from at the file's first digit should produce the same output as new.
    let path = one_million_path(0);
    let from_new = collect_single(&path, 1000).unwrap();
    let from_new_from = collect_single_from(&path, 1000, 1).unwrap();
    assert_eq!(from_new, from_new_from);
}

#[test]
fn seq_new_from_mid_file_matches_golden_suffix() {
    // Start at an arbitrary position mid-file and verify the digits match the golden file.
    let golden = golden_digits();
    let start: i64 = 500_000;
    let path = one_million_path(0);
    let result = collect_single_from(&path, 1000, start).unwrap();
    let expected = &golden[(start as usize - 1)..1_000_000];
    assert_eq!(
        result, expected,
        "digits from position {start} should match golden"
    );
}

#[test]
fn seq_new_from_block_boundary_position() {
    // Position exactly at a block boundary (multiple of 19, converted to 1-based).
    let golden = golden_digits();
    // 19 * 10 = 190, so position 191 is the start of block 10 (0-based: local_start = 190).
    let start: i64 = 191;
    let path = one_million_path(0);
    let result = collect_single_from(&path, 19, start).unwrap();
    let expected = &golden[(start as usize - 1)..1_000_000];
    assert_eq!(result, expected);
}

#[test]
fn seq_new_from_mid_block_position() {
    // Position inside a block (offset_in_block > 0).
    let golden = golden_digits();
    // local_start = 99 → block_index=5, offset_in_block=4
    let start: i64 = 100; // 1-based, local_start = 99
    let path = one_million_path(0);
    let result = collect_single_from(&path, 100, start).unwrap();
    let expected = &golden[(start as usize - 1)..1_000_000];
    assert_eq!(result, expected);
}

#[test]
fn seq_new_from_last_digit() {
    // Start at the very last digit of a file — should return exactly 1 digit.
    let golden = golden_digits();
    let start: i64 = 1_000_000;
    let path = one_million_path(0);
    let result = collect_single_from(&path, 19, start).unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result, &golden[999_999..1_000_000]);
}

#[test]
fn seq_new_from_non_zero_blockid_file() {
    // File with BlockID=1 (digit_start=1_000_001). new_from with that start position.
    let golden = golden_digits();
    let path = one_million_path(1);
    // digit_start = 1_000_001, digit_end = 2_000_000
    let start: i64 = 1_500_000;
    let result = collect_single_from(&path, 1000, start).unwrap();
    let expected = &golden[(start as usize - 1)..2_000_000];
    assert_eq!(result, expected);
}

#[test]
fn seq_new_from_generated_file_mid_block() {
    // Verify the skipped leading digits are correct using a small synthetic file.
    let digits = "12345678901234567890123456789012345678"; // 38 digits, 2 blocks
    let f = TempYcd::valid(digits, 38, 0, "\n", "").unwrap();

    // Start at position 5 (local_start=4, block_index=0, offset_in_block=4)
    let result = collect_single_from(&f.path, 19, 5).unwrap();
    assert_eq!(result, &digits[4..]);

    // Start at position 20 (local_start=19, block_index=1, offset_in_block=0)
    let result = collect_single_from(&f.path, 19, 20).unwrap();
    assert_eq!(result, &digits[19..]);

    // Start at position 25 (local_start=24, block_index=1, offset_in_block=5)
    let result = collect_single_from(&f.path, 19, 25).unwrap();
    assert_eq!(result, &digits[24..]);
}

#[test]
fn seq_new_from_errors() {
    let path = one_million_path(0); // digit_start=1, digit_length=1_000_000

    // start_position = 0 (before file range)
    assert_eq!(
        YcdSeqBlockStream::new_from(&path, 19, 0)
            .unwrap_err()
            .kind(),
        io::ErrorKind::InvalidInput
    );

    // start_position beyond last digit
    assert_eq!(
        YcdSeqBlockStream::new_from(&path, 19, 1_000_001)
            .unwrap_err()
            .kind(),
        io::ErrorKind::InvalidInput
    );

    // invalid unit_size
    assert_eq!(
        YcdSeqBlockStream::new_from(&path, 1, 1).unwrap_err().kind(),
        io::ErrorKind::InvalidInput
    );

    // second file with BlockID=1 — start_position in the first file's range
    let path2 = one_million_path(1); // digit_start=1_000_001
    assert_eq!(
        YcdSeqBlockStream::new_from(&path2, 19, 1)
            .unwrap_err()
            .kind(),
        io::ErrorKind::InvalidInput
    );
}
