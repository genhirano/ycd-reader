mod common;

use std::io;

use common::{collect_multi, golden_digits, one_million_path, two_million_path, TempYcd};
use ycd_reader::YcdMultiFileStream;

#[test]
fn every_read_shape_preserves_all_digits_and_search_ranges() {
    let golden = golden_digits();
    let paths = [
        one_million_path(0),
        one_million_path(1),
        one_million_path(2),
    ];
    let ranges = [
        (0, 100),
        (18, 100),
        (999_950, 200),
        (999_999, 100),
        (1_000_000, 100),
        (1_999_950, 200),
        (1_999_999, 100),
        (2_000_000, 100),
        (2_999_900, 100),
    ];

    for unit_size in [19, 20, 21, 999_983, 1_000_003] {
        let actual = collect_multi(&paths, unit_size).unwrap();
        assert_eq!(actual, golden, "unit size {unit_size}");

        for (start, length) in ranges {
            assert_eq!(
                actual[start..start + length],
                golden[start..start + length],
                "unit size {unit_size}, range {start}..{}",
                start + length
            );
        }
    }
}

#[test]
fn both_real_file_layouts_join_to_the_same_three_million_digits() {
    let golden = golden_digits();
    let one_million_layout = [
        one_million_path(0),
        one_million_path(1),
        one_million_path(2),
    ];
    let two_plus_one_million_layout = [two_million_path(0), one_million_path(2)];

    assert_eq!(
        collect_multi(&one_million_layout, 1_000_003).unwrap(),
        golden
    );
    assert_eq!(
        collect_multi(&two_plus_one_million_layout, 1_999_999).unwrap(),
        golden
    );
}

#[test]
fn multi_file_stream_rejects_empty_and_noncontiguous_lists() {
    let empty: [&str; 0] = [];
    assert_eq!(
        YcdMultiFileStream::new(&empty, 19).unwrap_err().kind(),
        io::ErrorKind::InvalidInput
    );

    let first = TempYcd::valid("1234567890123456789", 19, 0, "\n", "").unwrap();
    let third = TempYcd::valid("9876543210987654321", 19, 2, "\n", "").unwrap();
    assert_eq!(
        YcdMultiFileStream::new(&[&first.path, &third.path], 19)
            .unwrap_err()
            .kind(),
        io::ErrorKind::InvalidInput
    );
    assert_eq!(
        YcdMultiFileStream::new(&[&third.path, &first.path], 19)
            .unwrap_err()
            .kind(),
        io::ErrorKind::InvalidInput
    );
}

#[test]
fn multi_file_stream_joins_generated_files_inside_process_units() {
    let first_digits = "12345678901234567890123";
    let second_digits = "45678901234567890123456";
    let third_digits = "78901234567890123456789";
    let first = TempYcd::valid(first_digits, 23, 0, "\n", "").unwrap();
    let second = TempYcd::valid(second_digits, 23, 1, "\n", "").unwrap();
    let third = TempYcd::valid(third_digits, 23, 2, "\n", "").unwrap();
    let expected = format!("{first_digits}{second_digits}{third_digits}");

    assert_eq!(
        collect_multi(&[&first.path, &second.path, &third.path], 25).unwrap(),
        expected
    );
}
