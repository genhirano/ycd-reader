use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use ycd_reader::{YcdFileUtil, YcdHeaderInfoElem, YcdMultiFileStream, YcdSeqBlockStream};

const GOLDEN_PATH: &str = "tests/ycd/Pi - Dec - Chudnovsky_3000000.txt";
const ONE_MILLION_BASE: &str = "tests/ycd/1000000/Pi - Dec - Chudnovsky - ";
const TWO_MILLION_BASE: &str = "tests/ycd/2000000/Pi - Dec - Chudnovsky - ";

static TEMP_FILE_ID: AtomicU64 = AtomicU64::new(0);

struct TempYcd {
    path: PathBuf,
}

impl TempYcd {
    fn from_bytes(bytes: &[u8]) -> io::Result<Self> {
        let id = TEMP_FILE_ID.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("ycd_reader_{}_{}.ycd", std::process::id(), id));
        fs::write(&path, bytes)?;
        Ok(Self { path })
    }

    fn valid(
        digits: &str,
        block_size: usize,
        block_id: usize,
        line_ending: &str,
        extra_header: &str,
    ) -> io::Result<Self> {
        assert_eq!(digits.len(), block_size);
        let header = format!(
            "#Compressed Digit File{e}{e}\
             FileVersion:\t1.1.0{e}{e}\
             Base:\t10{e}{e}\
             FirstDigits:\t3.14159265358979323846{e}{e}\
             TotalDigits:\t0{e}{e}\
             {extra_header}\
             Blocksize:\t{block_size}{e}\
             BlockID:\t{block_id}{e}{e}\
             EndHeader{e}{e}",
            e = line_ending
        );
        let mut bytes = header.into_bytes();
        bytes.push(0);
        bytes.extend(encode_digits(digits));
        Self::from_bytes(&bytes)
    }

    fn final_block(
        digits: &str,
        block_size: usize,
        block_id: usize,
        total_digits: usize,
    ) -> io::Result<Self> {
        let header = format!(
            "FileVersion: 1.1.0\n\
             Base: 10\n\
             FirstDigits: 3.14159265358979323846\n\
             TotalDigits: {total_digits}\n\
             Blocksize: {block_size}\n\
             BlockID: {block_id}\n\
             EndHeader\n\n\0"
        );
        let mut bytes = header.into_bytes();
        bytes.extend(encode_digits(digits));
        Self::from_bytes(&bytes)
    }
}

impl Drop for TempYcd {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn one_million_path(block_id: usize) -> String {
    format!("{ONE_MILLION_BASE}{block_id}.ycd")
}

fn two_million_path(block_id: usize) -> String {
    format!("{TWO_MILLION_BASE}{block_id}.ycd")
}

fn golden_digits() -> String {
    fs::read_to_string(GOLDEN_PATH).expect("golden digit file must be readable")
}

fn encode_digits(digits: &str) -> Vec<u8> {
    let mut encoded = Vec::new();
    for chunk in digits.as_bytes().chunks(19) {
        let mut block = String::from_utf8(chunk.to_vec()).expect("test digits are ASCII");
        while block.len() < 19 {
            block.push('0');
        }
        let number = block.parse::<u64>().expect("test block must fit in u64");
        encoded.extend(number.to_le_bytes());
    }
    encoded
}

fn collect_single(path: impl AsRef<Path>, unit_size: i32) -> io::Result<String> {
    let header = YcdFileUtil::get_ycd_header(path.as_ref())?;
    let block_size = header[&YcdHeaderInfoElem::Blocksize]
        .parse::<i64>()
        .unwrap();
    let block_id = header[&YcdHeaderInfoElem::BlockID].parse::<i64>().unwrap();
    let mut expected_start = 1 + block_size * block_id;
    let mut expected_process_no = 1;
    let mut result = String::new();
    let mut stream = YcdSeqBlockStream::new(path, unit_size)?;

    while stream.has_next() {
        let unit = stream.next()?.clone();
        assert_eq!(unit.process_no, expected_process_no);
        assert_eq!(unit.start_digit, expected_start);
        expected_process_no += 1;
        expected_start += unit.value.len() as i64;
        result.push_str(&unit.value);
    }

    assert_eq!(
        stream.next().unwrap_err().kind(),
        io::ErrorKind::UnexpectedEof
    );
    Ok(result)
}

fn collect_single_prefix(
    path: impl AsRef<Path>,
    unit_size: i32,
    length: usize,
) -> io::Result<String> {
    let mut result = String::new();
    let mut stream = YcdSeqBlockStream::new(path, unit_size)?;
    while result.len() < length {
        result.push_str(&stream.next()?.value);
    }
    result.truncate(length);
    Ok(result)
}

fn collect_multi<P: AsRef<Path>>(paths: &[P], unit_size: i32) -> io::Result<String> {
    let first_header = YcdFileUtil::get_ycd_header(paths[0].as_ref())?;
    let block_size = first_header[&YcdHeaderInfoElem::Blocksize]
        .parse::<i64>()
        .unwrap();
    let block_id = first_header[&YcdHeaderInfoElem::BlockID]
        .parse::<i64>()
        .unwrap();
    let mut expected_start = 1 + block_size * block_id;
    let mut expected_process_no = 1;
    let mut result = String::new();
    let mut stream = YcdMultiFileStream::new(paths, unit_size)?;

    while stream.has_next() {
        let unit = stream.next()?.clone();
        assert_eq!(unit.process_no, expected_process_no);
        assert_eq!(unit.start_digit, expected_start);
        expected_process_no += 1;
        expected_start += unit.value.len() as i64;
        result.push_str(&unit.value);
    }

    assert_eq!(
        stream.next().unwrap_err().kind(),
        io::ErrorKind::UnexpectedEof
    );
    Ok(result)
}

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
