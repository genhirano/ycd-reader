mod common;

use std::io;

use common::TempYcd;
use ycd_reader::{YcdFileUtil, YcdIndex, YcdMultiFileStream, YcdProcessUnit, YcdSeqBlockStream};

#[test]
fn digit_positions_and_lengths_are_interchangeable_across_apis() {
    let digits = "12345678901234567890123456789012345678";
    let file = TempYcd::valid(digits, digits.len(), 1, "\n", "").unwrap();
    let files = [&file.path];
    let index = YcdIndex::build(&files).unwrap();
    let entry = &index.entries()[0];
    let unit_size: usize = 19;

    let mut sequential = YcdSeqBlockStream::new(&file.path, unit_size).unwrap();
    let first = sequential.next().unwrap().clone();
    let process_no: usize = first.process_no;
    let start: usize = first.start_digit;
    assert_eq!(process_no, 1);
    assert_eq!(start, entry.file_start);
    assert_eq!(entry.file_length, digits.len());
    assert_eq!(
        YcdProcessUnit::new(process_no, start, first.value.clone()),
        first
    );
    assert_eq!(
        index.read_digits(start, first.value.len()).unwrap(),
        first.value
    );
    assert_eq!(
        YcdFileUtil::read_digits(&files, start, first.value.len()).unwrap(),
        first.value
    );

    let mut multi = YcdMultiFileStream::new(&files, unit_size).unwrap();
    assert_eq!(multi.next().unwrap(), &first);
    let resume = start + first.value.len();
    let mut resumed = YcdSeqBlockStream::new_from(&file.path, unit_size, resume).unwrap();
    let second = resumed.next().unwrap().clone();
    let mut resumed_multi = YcdMultiFileStream::new_from(&files, unit_size, resume).unwrap();
    assert_eq!(resumed_multi.next().unwrap(), &second);
    assert_eq!(
        index
            .read_digits(second.start_digit, second.value.len())
            .unwrap(),
        second.value
    );
}

#[test]
fn unit_sizes_above_i32_max_are_not_truncated() {
    let digits = "1234567890123456789";
    let file = TempYcd::valid(digits, digits.len(), 0, "\n", "").unwrap();
    let files = [&file.path];
    let unit_size = i32::MAX as usize + 1;

    for mut stream in [
        YcdSeqBlockStream::new(&file.path, unit_size).unwrap(),
        YcdSeqBlockStream::new_from(&file.path, unit_size, 1).unwrap(),
    ] {
        assert_eq!(stream.next().unwrap().value, digits);
        assert!(!stream.has_next());
    }
    for mut stream in [
        YcdMultiFileStream::new(&files, usize::MAX).unwrap(),
        YcdMultiFileStream::new_from(&files, usize::MAX, 1).unwrap(),
    ] {
        assert_eq!(stream.next().unwrap_err().kind(), io::ErrorKind::Other);
    }
    for unit_size in [0_usize, 18] {
        assert_eq!(
            YcdSeqBlockStream::new(&file.path, unit_size)
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput
        );
        assert_eq!(
            YcdMultiFileStream::new(&files, unit_size)
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput
        );
    }
}

#[test]
fn digit_positions_near_usize_max_are_preserved() {
    let digits = "1234567890123456789";
    let block_id = usize::MAX / digits.len() - 1;
    let start = block_id * digits.len() + 1;
    let file = TempYcd::valid(digits, digits.len(), block_id, "\n", "").unwrap();
    let files = [&file.path];
    let index = YcdIndex::build(&files).unwrap();
    assert_eq!(index.entries()[0].file_start, start);
    assert_eq!(index.read_digits(start, digits.len()).unwrap(), digits);
    assert_eq!(
        YcdFileUtil::read_digits(&files, start, digits.len()).unwrap(),
        digits
    );
    for mut stream in [
        YcdSeqBlockStream::new(&file.path, 19).unwrap(),
        YcdSeqBlockStream::new_from(&file.path, 19, start).unwrap(),
    ] {
        let unit = stream.next().unwrap();
        assert_eq!(unit.start_digit, start);
        assert_eq!(unit.value, digits);
        assert!(!stream.has_next());
    }
    for mut stream in [
        YcdMultiFileStream::new(&files, 19).unwrap(),
        YcdMultiFileStream::new_from(&files, 19, start).unwrap(),
    ] {
        let unit = stream.next().unwrap();
        assert_eq!(unit.start_digit, start);
        assert_eq!(unit.value, digits);
        assert!(!stream.has_next());
    }

    // Also test a 1-digit file ending at exact `usize::MAX` (`Blocksize = 1`, `BlockID = usize::MAX - 1`).
    let max_file = TempYcd::valid("7", 1, usize::MAX - 1, "\n", "").unwrap();
    let max_files = [&max_file.path];
    let max_index = YcdIndex::build(&max_files).unwrap();
    assert_eq!(max_index.entries()[0].file_start, usize::MAX);
    for mut stream in [
        YcdSeqBlockStream::new(&max_file.path, 19).unwrap(),
        YcdSeqBlockStream::new_from(&max_file.path, 19, usize::MAX).unwrap(),
    ] {
        let unit = stream.next().unwrap();
        assert_eq!(unit.start_digit, usize::MAX);
        assert_eq!(unit.value, "7");
        assert!(!stream.has_next());
    }
    for mut stream in [
        YcdMultiFileStream::new(&max_files, 19).unwrap(),
        YcdMultiFileStream::new_from(&max_files, 19, usize::MAX).unwrap(),
    ] {
        let unit = stream.next().unwrap();
        assert_eq!(unit.start_digit, usize::MAX);
        assert_eq!(unit.value, "7");
        assert!(!stream.has_next());
    }
}

#[test]
fn invalid_digit_metadata_is_rejected_without_wrapping() {
    let digits = "1234567890123456789";
    let valid = TempYcd::valid(digits, digits.len(), 0, "\n", "").unwrap();
    let bytes = std::fs::read(&valid.path).unwrap();
    let header_end = bytes.iter().position(|&b| b == 0).unwrap();
    let header = std::str::from_utf8(&bytes[..header_end]).unwrap();
    for invalid_header in [
        header.replace("Blocksize:\t19", "Blocksize:\t-19"),
        header.replace("Blocksize:\t19", &format!("Blocksize:\t{}0", usize::MAX)),
        header.replace("BlockID:\t0", &format!("BlockID:\t{}", usize::MAX)),
        header.replace("BlockID:\t0", &format!("BlockID:\t{}", usize::MAX / 19)),
        header
            .replace("Blocksize:\t19", &format!("Blocksize:\t{}", usize::MAX))
            .replace("BlockID:\t0", "BlockID:\t1"),
        header
            .replace("TotalDigits:\t0", "TotalDigits:\t1")
            .replace("BlockID:\t0", "BlockID:\t1"),
    ] {
        let mut bytes = invalid_header.into_bytes();
        bytes.push(0);
        let file = TempYcd::from_bytes(&bytes).unwrap();
        let files = [&file.path];
        assert_eq!(
            YcdSeqBlockStream::new(&file.path, 19).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        assert_eq!(
            YcdIndex::build(&files).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        assert_eq!(
            YcdFileUtil::read_digits(&files, 1, 1).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }
}
