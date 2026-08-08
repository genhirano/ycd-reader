mod common;

use std::io;

use common::{collect_single, one_million_path, two_million_path, TempYcd};
use ycd_reader::{YcdFileUtil, YcdHeaderInfoElem, YcdSeqBlockStream};

#[test]
fn real_headers_are_parsed_at_the_exact_data_offset() {
    for block_id in [0, 1, 14] {
        let path = one_million_path(block_id);
        let expected_size = if block_id < 10 { 195 } else { 196 };
        assert_eq!(YcdFileUtil::get_header_size(&path).unwrap(), expected_size);

        let header = YcdFileUtil::get_ycd_header(&path).unwrap();
        assert_eq!(header[&YcdHeaderInfoElem::FileVersion], "1.1.0");
        assert_eq!(header[&YcdHeaderInfoElem::Base], "10");
        assert_eq!(header[&YcdHeaderInfoElem::Blocksize], "1000000");
        assert_eq!(header[&YcdHeaderInfoElem::BlockID], block_id.to_string());
    }

    for block_id in [0, 2] {
        let path = two_million_path(block_id);
        assert_eq!(YcdFileUtil::get_header_size(&path).unwrap(), 195);
        assert_eq!(
            YcdFileUtil::get_ycd_header(&path).unwrap()[&YcdHeaderInfoElem::Blocksize],
            "2000000"
        );
    }
}

#[test]
fn generated_file_preserves_leading_zeroes_and_a_partial_final_block() {
    let digits = concat!("0000000000000000001", "2345678901234567890", "00420");
    let file = TempYcd::valid(digits, digits.len(), 0, "\r\n", "").unwrap();
    let mut stream = YcdSeqBlockStream::new(&file.path, 20).unwrap();

    let first = stream.next().unwrap().clone();
    let second = stream.next().unwrap().clone();
    let third = stream.next().unwrap().clone();

    assert_eq!(first.value, &digits[..20]);
    assert_eq!(second.value, &digits[20..40]);
    assert_eq!(third.value, &digits[40..]);
    assert_eq!((first.process_no, first.start_digit), (1, 1));
    assert_eq!((second.process_no, second.start_digit), (2, 21));
    assert_eq!((third.process_no, third.start_digit), (3, 41));
    assert!(!stream.has_next());
}

#[test]
fn exact_multiple_of_nineteen_stops_without_reading_an_extra_block() {
    let digits = "12345678901234567890123456789012345678";
    let file = TempYcd::valid(digits, digits.len(), 0, "\n", "").unwrap();

    assert_eq!(collect_single(&file.path, 19).unwrap(), digits);
}

#[test]
fn total_digits_limits_a_short_final_file() {
    let digits = "123456789012345678901234";
    let file = TempYcd::final_block(digits, 38, 0, digits.len()).unwrap();

    assert_eq!(collect_single(&file.path, 19).unwrap(), digits);
}

#[test]
fn long_headers_and_lf_line_endings_are_supported() {
    let extra_header = format!("UnknownField:\t{}\n", "x".repeat(400));
    let digits = "1234567890123456789";
    let file = TempYcd::valid(digits, digits.len(), 0, "\n", &extra_header).unwrap();

    assert!(YcdFileUtil::get_header_size(&file.path).unwrap() > 300);
    assert_eq!(collect_single(&file.path, 19).unwrap(), digits);
}

#[test]
fn invalid_inputs_and_headers_return_specific_errors() {
    assert_eq!(
        YcdSeqBlockStream::new("does-not-exist.ycd", 18)
            .unwrap_err()
            .kind(),
        io::ErrorKind::InvalidInput
    );
    assert_eq!(
        YcdSeqBlockStream::new("does-not-exist.ycd", 19)
            .unwrap_err()
            .kind(),
        io::ErrorKind::NotFound
    );

    let invalid_headers = [
        "FileVersion: 1.1.0\nBase: 10\nFirstDigits: 3.14\nTotalDigits: 0\nBlockID: 0\nEndHeader\n\n\0",
        "FileVersion: 1.1.0\nBase: 10\nFirstDigits: 3.14\nTotalDigits: 0\nBlocksize: x\nBlockID: 0\nEndHeader\n\n\0",
        "FileVersion: 1.1.0\nBase: 16\nFirstDigits: 3.14\nTotalDigits: 0\nBlocksize: 19\nBlockID: 0\nEndHeader\n\n\0",
        "FileVersion: 1.1.0\nBase: 10\nFirstDigits: 3.14\nTotalDigits: 0\nBlocksize: 19\nBlockID: -1\nEndHeader\n\n\0",
        "FileVersion: 1.1.0\nBase: 10\nFirstDigits: 3.14\nTotalDigits: 0\nBlocksize: 19\nBlockID: 0\n",
        "FileVersion: 1.1.0\nBase: 10\nFirstDigits: 3.14\nTotalDigits: 0\nBlocksize: 19\nBlockID: 0\nEndHeader\n\nX",
    ];

    for header in invalid_headers {
        let file = TempYcd::from_bytes(header.as_bytes()).unwrap();
        assert_eq!(
            YcdFileUtil::get_ycd_header(&file.path).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }
}

#[test]
fn corrupt_payloads_are_rejected() {
    let header = concat!(
        "FileVersion: 1.1.0\n",
        "Base: 10\n",
        "FirstDigits: 3.14\n",
        "TotalDigits: 0\n",
        "Blocksize: 38\n",
        "BlockID: 0\n",
        "EndHeader\n\n\0"
    );
    let mut truncated = header.as_bytes().to_vec();
    truncated.extend(123_u64.to_le_bytes());
    let file = TempYcd::from_bytes(&truncated).unwrap();
    let mut stream = YcdSeqBlockStream::new(&file.path, 38).unwrap();
    assert_eq!(
        stream.next().unwrap_err().kind(),
        io::ErrorKind::UnexpectedEof
    );

    let mut oversized = header
        .replace("Blocksize: 38", "Blocksize: 19")
        .into_bytes();
    oversized.extend(u64::MAX.to_le_bytes());
    let file = TempYcd::from_bytes(&oversized).unwrap();
    let mut stream = YcdSeqBlockStream::new(&file.path, 19).unwrap();
    assert_eq!(
        stream.next().unwrap_err().kind(),
        io::ErrorKind::InvalidData
    );
}
