mod common;

use common::{
    collect_single, collect_single_prefix, golden_digits, one_million_path, two_million_path,
};

#[test]
fn individual_real_files_match_the_three_million_digit_golden_file() {
    let golden = golden_digits();
    let unit_sizes = [19, 257, 8191];

    for (block_id, unit_size) in unit_sizes.into_iter().enumerate() {
        let actual = collect_single(one_million_path(block_id), unit_size).unwrap();
        let start = block_id * 1_000_000;
        assert_eq!(actual, golden[start..start + 1_000_000]);
    }

    let first_two_million = collect_single(two_million_path(0), 20).unwrap();
    assert_eq!(first_two_million, golden[..2_000_000]);

    let third_million = collect_single_prefix(two_million_path(1), 65_537, 1_000_000).unwrap();
    assert_eq!(third_million, golden[2_000_000..]);
}

use std::io;
use ycd_reader::YcdSeqBlockStream;

#[test]
fn seq_stream_unit_size_larger_than_file() {
    let golden = golden_digits();
    let mut stream = YcdSeqBlockStream::new(one_million_path(0), 2_000_000).unwrap();

    let u = stream.next().unwrap().clone();
    assert_eq!(u.process_no, 1);
    assert_eq!(u.start_digit, 1);
    assert_eq!(u.value.len(), 1_000_000);
    assert_eq!(u.value, &golden[..1_000_000]);

    assert!(!stream.has_next());
    assert_eq!(
        stream.next().unwrap_err().kind(),
        io::ErrorKind::UnexpectedEof
    );
}

#[test]
fn seq_stream_exhaustion_repeated_eof() {
    let mut stream = YcdSeqBlockStream::new(one_million_path(0), 1_000_000).unwrap();
    let _ = stream.next().unwrap();

    for _ in 0..5 {
        assert_eq!(
            stream.next().unwrap_err().kind(),
            io::ErrorKind::UnexpectedEof
        );
    }
}

#[test]
fn seq_stream_debug_impl() {
    let stream = YcdSeqBlockStream::new(one_million_path(0), 19).unwrap();
    let debug = format!("{stream:?}");
    assert!(debug.contains("YcdSeqBlockStream"));
}
