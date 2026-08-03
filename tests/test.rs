use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use ycd_reader::{
    compute_seek_params, YcdFileUtil, YcdHeaderInfoElem, YcdMultiFileStream, YcdSeqBlockStream,
};

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

// ============================================================================
// Tests for YcdFileUtil::read_digits (random-access API)
// ============================================================================

// --- Helper: construct raw YCD bytes with a corrupt block (u64::MAX) --------

fn make_corrupt_block_ycd(blocksize: usize) -> Vec<u8> {
    let header = format!(
        "FileVersion: 1.1.0\n\
         Base: 10\n\
         FirstDigits: 3.14159265358979323846\n\
         TotalDigits: 0\n\
         Blocksize: {blocksize}\n\
         BlockID: 0\n\
         EndHeader\n\n\0"
    );
    let mut bytes = header.into_bytes();
    bytes.extend(u64::MAX.to_le_bytes());
    bytes
}

fn make_truncated_payload_ycd(blocksize: usize) -> Vec<u8> {
    let header = format!(
        "FileVersion: 1.1.0\n\
         Base: 10\n\
         FirstDigits: 3.14159265358979323846\n\
         TotalDigits: 0\n\
         Blocksize: {blocksize}\n\
         BlockID: 0\n\
         EndHeader\n\n\0"
    );
    let mut bytes = header.into_bytes();
    // Only provide one 8-byte block when blocksize needs 2+ blocks
    bytes.extend(123_u64.to_le_bytes());
    bytes
}

// --- Normal-case tests -------------------------------------------------------

#[test]
fn read_digits_from_position_one() {
    let golden = golden_digits();
    let files = [one_million_path(0)];

    assert_eq!(YcdFileUtil::read_digits(&files, 1, 1).unwrap(), "1");
    assert_eq!(
        YcdFileUtil::read_digits(&files, 1, 19).unwrap(),
        "1415926535897932384"
    );
    assert_eq!(
        YcdFileUtil::read_digits(&files, 1, 20).unwrap(),
        "14159265358979323846"
    );
    assert_eq!(
        YcdFileUtil::read_digits(&files, 1, 1).unwrap(),
        &golden[..1]
    );
    assert_eq!(
        YcdFileUtil::read_digits(&files, 1, 100).unwrap(),
        &golden[..100]
    );
}

#[test]
fn read_digits_arbitrary_positions() {
    let golden = golden_digits();
    let files = [one_million_path(0)];

    // Single digit at an arbitrary position
    assert_eq!(
        YcdFileUtil::read_digits(&files, 500, 1).unwrap(),
        &golden[499..500]
    );
    // Multiple digits at an arbitrary middle position
    assert_eq!(
        YcdFileUtil::read_digits(&files, 12345, 50).unwrap(),
        &golden[12344..12394]
    );
    // Position near the middle of the file
    assert_eq!(
        YcdFileUtil::read_digits(&files, 500_000, 100).unwrap(),
        &golden[499_999..500_099]
    );
}

#[test]
fn read_digits_nineteen_block_boundaries() {
    let golden = golden_digits();
    let files = [one_million_path(0)];

    // Last digit of block 0 (position 19)
    assert_eq!(YcdFileUtil::read_digits(&files, 19, 1).unwrap(), "4");
    assert_eq!(
        YcdFileUtil::read_digits(&files, 19, 1).unwrap(),
        &golden[18..19]
    );
    // First digit of block 1 (position 20)
    assert_eq!(YcdFileUtil::read_digits(&files, 20, 1).unwrap(), "6");
    assert_eq!(
        YcdFileUtil::read_digits(&files, 20, 1).unwrap(),
        &golden[19..20]
    );
    // Crossing block 0 → block 1 (positions 15..24)
    assert_eq!(
        YcdFileUtil::read_digits(&files, 15, 10).unwrap(),
        "3238462643"
    );
    assert_eq!(
        YcdFileUtil::read_digits(&files, 15, 10).unwrap(),
        &golden[14..24]
    );
    // Crossing block 0 → block 1 with only 2 digits (positions 19..20)
    assert_eq!(YcdFileUtil::read_digits(&files, 19, 2).unwrap(), "46");
    assert_eq!(
        YcdFileUtil::read_digits(&files, 19, 2).unwrap(),
        &golden[18..20]
    );
    // Exact first full block
    assert_eq!(
        YcdFileUtil::read_digits(&files, 1, 19).unwrap(),
        &golden[..19]
    );
    // Exact second full block
    assert_eq!(
        YcdFileUtil::read_digits(&files, 20, 19).unwrap(),
        &golden[19..38]
    );
    // Crossing several blocks
    assert_eq!(
        YcdFileUtil::read_digits(&files, 10, 57).unwrap(),
        &golden[9..66]
    );
}

#[test]
fn read_digits_one_million_file_boundaries() {
    let golden = golden_digits();
    let files_0_1 = [one_million_path(0), one_million_path(1)];
    let files_0_1_2 = [
        one_million_path(0),
        one_million_path(1),
        one_million_path(2),
    ];

    // Last digit of file 0 (position 1_000_000)
    assert_eq!(
        YcdFileUtil::read_digits(&files_0_1, 1_000_000, 1).unwrap(),
        "1"
    );
    assert_eq!(
        YcdFileUtil::read_digits(&files_0_1, 1_000_000, 1).unwrap(),
        &golden[999_999..1_000_000]
    );
    // First digit of file 1 (position 1_000_001)
    assert_eq!(
        YcdFileUtil::read_digits(&files_0_1, 1_000_001, 1).unwrap(),
        "3"
    );
    assert_eq!(
        YcdFileUtil::read_digits(&files_0_1, 1_000_001, 1).unwrap(),
        &golden[1_000_000..1_000_001]
    );
    // Crossing the 1M boundary: positions 999_996..1_000_005
    assert_eq!(
        YcdFileUtil::read_digits(&files_0_1, 999_996, 10).unwrap(),
        "5815130927"
    );
    assert_eq!(
        YcdFileUtil::read_digits(&files_0_1, 999_996, 10).unwrap(),
        &golden[999_995..1_000_005]
    );
    // Crossing exactly 1 digit on each side of the boundary
    assert_eq!(
        YcdFileUtil::read_digits(&files_0_1, 1_000_000, 2).unwrap(),
        "13"
    );
    assert_eq!(
        YcdFileUtil::read_digits(&files_0_1, 1_000_000, 2).unwrap(),
        &golden[999_999..1_000_001]
    );
    // The example from the issue comment
    assert_eq!(
        YcdFileUtil::read_digits(&files_0_1, 999_995, 20).unwrap(),
        "45815130927562832084"
    );
    // 2M boundary (within 1M+1M+1M layout)
    assert_eq!(
        YcdFileUtil::read_digits(&files_0_1_2, 2_000_000, 1).unwrap(),
        &golden[1_999_999..2_000_000]
    );
    assert_eq!(
        YcdFileUtil::read_digits(&files_0_1_2, 2_000_001, 1).unwrap(),
        &golden[2_000_000..2_000_001]
    );
    assert_eq!(
        YcdFileUtil::read_digits(&files_0_1_2, 1_999_996, 10).unwrap(),
        &golden[1_999_995..2_000_005]
    );
    assert_eq!(
        YcdFileUtil::read_digits(&files_0_1_2, 2_000_000, 2).unwrap(),
        &golden[1_999_999..2_000_001]
    );
}

#[test]
fn read_digits_two_million_file_boundary() {
    let golden = golden_digits();
    // 2M+1M layout: two_million_path(0) covers 1-2_000_000, one_million_path(2) covers 2_000_001-3_000_000
    let files = [two_million_path(0), one_million_path(2)];

    // Last digit of the 2M file (position 2_000_000)
    assert_eq!(YcdFileUtil::read_digits(&files, 2_000_000, 1).unwrap(), "9");
    assert_eq!(
        YcdFileUtil::read_digits(&files, 2_000_000, 1).unwrap(),
        &golden[1_999_999..2_000_000]
    );
    // First digit of the 1M file (position 2_000_001)
    assert_eq!(YcdFileUtil::read_digits(&files, 2_000_001, 1).unwrap(), "6");
    assert_eq!(
        YcdFileUtil::read_digits(&files, 2_000_001, 1).unwrap(),
        &golden[2_000_000..2_000_001]
    );
    // Crossing the 2M boundary: positions 1_999_996..2_000_005
    assert_eq!(
        YcdFileUtil::read_digits(&files, 1_999_996, 10).unwrap(),
        "9790961217"
    );
    assert_eq!(
        YcdFileUtil::read_digits(&files, 1_999_996, 10).unwrap(),
        &golden[1_999_995..2_000_005]
    );
    // Crossing exactly 1 digit on each side
    assert_eq!(
        YcdFileUtil::read_digits(&files, 2_000_000, 2).unwrap(),
        "96"
    );
    assert_eq!(
        YcdFileUtil::read_digits(&files, 2_000_000, 2).unwrap(),
        &golden[1_999_999..2_000_001]
    );
}

#[test]
fn read_digits_same_range_both_layouts() {
    let golden = golden_digits();
    let layout_1m = [
        one_million_path(0),
        one_million_path(1),
        one_million_path(2),
    ];
    let layout_2m_1m = [two_million_path(0), one_million_path(2)];

    let test_ranges = [
        (1_usize, 100_usize),
        (999_990, 20),
        (1_000_000, 5),
        (1_000_001, 5),
        (1_999_990, 20),
        (2_000_000, 5),
        (2_000_001, 5),
        (2_999_990, 10),
        (3_000_000, 1),
    ];

    for (start, len) in test_ranges {
        let from_1m = YcdFileUtil::read_digits(&layout_1m, start, len).unwrap();
        let from_2m_1m = YcdFileUtil::read_digits(&layout_2m_1m, start, len).unwrap();
        assert_eq!(
            from_1m, from_2m_1m,
            "layouts differ at start={start} len={len}"
        );
        assert_eq!(
            from_1m,
            &golden[start - 1..start - 1 + len],
            "mismatch with golden at start={start} len={len}"
        );
    }
}

#[test]
fn read_digits_non_zero_blockid_first_file() {
    let golden = golden_digits();
    // Start the list at BlockID=1 (positions 1_000_001 to 2_000_000)
    let files = [one_million_path(1), one_million_path(2)];

    // Read exactly at the list start
    assert_eq!(
        YcdFileUtil::read_digits(&files, 1_000_001, 10).unwrap(),
        "3092756283"
    );
    assert_eq!(
        YcdFileUtil::read_digits(&files, 1_000_001, 10).unwrap(),
        &golden[1_000_000..1_000_010]
    );
    // Read across the internal boundary
    assert_eq!(
        YcdFileUtil::read_digits(&files, 1_999_996, 10).unwrap(),
        &golden[1_999_995..2_000_005]
    );
}

#[test]
fn read_digits_last_digit_of_list() {
    let golden = golden_digits();
    let files = [
        one_million_path(0),
        one_million_path(1),
        one_million_path(2),
    ];

    // Read the very last digit
    assert_eq!(YcdFileUtil::read_digits(&files, 3_000_000, 1).unwrap(), "3");
    assert_eq!(
        YcdFileUtil::read_digits(&files, 3_000_000, 1).unwrap(),
        &golden[2_999_999..3_000_000]
    );
    // Read the last 5 digits
    assert_eq!(
        YcdFileUtil::read_digits(&files, 2_999_996, 5).unwrap(),
        "43943"
    );
    assert_eq!(
        YcdFileUtil::read_digits(&files, 2_999_996, 5).unwrap(),
        &golden[2_999_995..3_000_000]
    );
}

#[test]
fn read_digits_leading_zeros_in_result() {
    let golden = golden_digits();
    let files = [one_million_path(0)];

    // Position 32 is the first '0' digit in Pi
    assert_eq!(&golden[31..32], "0"); // assert our test assumption
    assert_eq!(YcdFileUtil::read_digits(&files, 32, 1).unwrap(), "0");
    assert_eq!(
        YcdFileUtil::read_digits(&files, 32, 4).unwrap(),
        &golden[31..35]
    );

    // Position 77 starts a 19-digit block whose first digit is '0'
    assert_eq!(&golden[76..77], "0");
    assert_eq!(
        YcdFileUtil::read_digits(&files, 77, 19).unwrap(),
        &golden[76..95]
    );

    // Position 307 starts with "00"
    assert_eq!(&golden[306..308], "00");
    assert_eq!(YcdFileUtil::read_digits(&files, 307, 5).unwrap(), "00660");
    assert_eq!(
        YcdFileUtil::read_digits(&files, 307, 5).unwrap(),
        &golden[306..311]
    );
}

#[test]
fn read_digits_spanning_multiple_files() {
    let golden = golden_digits();
    // Read across all three 1M files in one call
    let files = [
        one_million_path(0),
        one_million_path(1),
        one_million_path(2),
    ];

    // Span all three files
    assert_eq!(
        YcdFileUtil::read_digits(&files, 999_990, 20).unwrap(),
        &golden[999_989..1_000_009]
    );
    assert_eq!(
        YcdFileUtil::read_digits(&files, 1_999_990, 20).unwrap(),
        &golden[1_999_989..2_000_009]
    );
    // A single large read spanning all boundaries
    let result = YcdFileUtil::read_digits(&files, 999_990, 1_000_020).unwrap();
    assert_eq!(result, &golden[999_989..2_000_009]);
}

// --- Error-case tests --------------------------------------------------------

#[test]
fn read_digits_error_invalid_basic_params() {
    let files = [one_million_path(0)];

    // Empty file list
    let empty: [&str; 0] = [];
    assert_eq!(
        YcdFileUtil::read_digits(&empty, 1, 1).unwrap_err().kind(),
        io::ErrorKind::InvalidInput
    );
    // Position 0
    assert_eq!(
        YcdFileUtil::read_digits(&files, 0, 1).unwrap_err().kind(),
        io::ErrorKind::InvalidInput
    );
    // Length 0
    assert_eq!(
        YcdFileUtil::read_digits(&files, 1, 0).unwrap_err().kind(),
        io::ErrorKind::InvalidInput
    );
}

#[test]
fn read_digits_error_out_of_range() {
    // Use a single 1M file starting at BlockID=1 (positions 1_000_001..=2_000_000)
    let single_file = [one_million_path(1)];

    // One before list start
    assert_eq!(
        YcdFileUtil::read_digits(&single_file, 1_000_000, 1)
            .unwrap_err()
            .kind(),
        io::ErrorKind::InvalidInput
    );
    // One after list end
    assert_eq!(
        YcdFileUtil::read_digits(&single_file, 2_000_001, 1)
            .unwrap_err()
            .kind(),
        io::ErrorKind::InvalidInput
    );
    // Valid start but end exceeds by exactly 1
    assert_eq!(
        YcdFileUtil::read_digits(&single_file, 2_000_000, 2)
            .unwrap_err()
            .kind(),
        io::ErrorKind::InvalidInput
    );
    // Also test with BlockID=0 file
    let first_file = [one_million_path(0)];
    assert_eq!(
        YcdFileUtil::read_digits(&first_file, 1_000_001, 1)
            .unwrap_err()
            .kind(),
        io::ErrorKind::InvalidInput
    );
}

#[test]
fn read_digits_error_overflow() {
    // start + length overflows usize (start=1, length=usize::MAX)
    let files = [one_million_path(0)];
    assert_eq!(
        YcdFileUtil::read_digits(&files, 1, usize::MAX)
            .unwrap_err()
            .kind(),
        io::ErrorKind::InvalidData
    );
}

#[test]
fn read_digits_error_list_continuity() {
    let files_0 = [one_million_path(0)];
    let files_1 = [one_million_path(1)];

    // Gap: files 0 and 2 (missing file 1)
    assert_eq!(
        YcdFileUtil::read_digits(&[one_million_path(0), one_million_path(2)], 1, 10)
            .unwrap_err()
            .kind(),
        io::ErrorKind::InvalidInput
    );
    // Duplicate: file 0 twice
    assert_eq!(
        YcdFileUtil::read_digits(&[one_million_path(0), one_million_path(0)], 1, 10)
            .unwrap_err()
            .kind(),
        io::ErrorKind::InvalidInput
    );
    // Reversed: file 1 then file 0
    assert_eq!(
        YcdFileUtil::read_digits(&[one_million_path(1), one_million_path(0)], 1_000_001, 10)
            .unwrap_err()
            .kind(),
        io::ErrorKind::InvalidInput
    );
    // Unused file is noncontiguous: files 0, 1, 3 — request only touches 0+1,
    // but file 3 (BlockID=3, starts at 3_000_001) breaks continuity after file 1
    assert_eq!(
        YcdFileUtil::read_digits(
            &[
                one_million_path(0),
                one_million_path(1),
                one_million_path(3)
            ],
            1,
            100
        )
        .unwrap_err()
        .kind(),
        io::ErrorKind::InvalidInput
    );
    // Suppress unused-variable warnings for files_0 and files_1
    let _ = (files_0, files_1);
}

#[test]
fn read_digits_error_bad_file_content() {
    // Missing required header field (no Blocksize)
    let no_blocksize = b"FileVersion: 1.1.0\n\
        Base: 10\n\
        FirstDigits: 3.14\n\
        TotalDigits: 0\n\
        BlockID: 0\n\
        EndHeader\n\n\0";
    let f = TempYcd::from_bytes(no_blocksize).unwrap();
    assert_eq!(
        YcdFileUtil::read_digits(&[&f.path], 1, 1)
            .unwrap_err()
            .kind(),
        io::ErrorKind::InvalidData
    );

    // Non-base-10 file
    let base16 = b"FileVersion: 1.1.0\n\
        Base: 16\n\
        FirstDigits: 3.14\n\
        TotalDigits: 0\n\
        Blocksize: 19\n\
        BlockID: 0\n\
        EndHeader\n\n\0";
    let f = TempYcd::from_bytes(base16).unwrap();
    assert_eq!(
        YcdFileUtil::read_digits(&[&f.path], 1, 1)
            .unwrap_err()
            .kind(),
        io::ErrorKind::InvalidData
    );

    // Corrupt block (u64::MAX decodes to 20 decimal digits)
    let f = TempYcd::from_bytes(&make_corrupt_block_ycd(19)).unwrap();
    assert_eq!(
        YcdFileUtil::read_digits(&[&f.path], 1, 19)
            .unwrap_err()
            .kind(),
        io::ErrorKind::InvalidData
    );

    // Truncated payload (Blocksize=38 needs 2 blocks but only 1 is present)
    let f = TempYcd::from_bytes(&make_truncated_payload_ycd(38)).unwrap();
    assert_eq!(
        YcdFileUtil::read_digits(&[&f.path], 1, 38)
            .unwrap_err()
            .kind(),
        io::ErrorKind::UnexpectedEof
    );

    // File does not exist
    assert_eq!(
        YcdFileUtil::read_digits(&["does-not-exist.ycd"], 1, 1)
            .unwrap_err()
            .kind(),
        io::ErrorKind::NotFound
    );
}

// --- Implementation-method verification tests --------------------------------

#[test]
fn seek_params_unit_tests() {
    // Position 1 within file → block_index = 0
    let (bi, oib, btr) = compute_seek_params(0, 1);
    assert_eq!(bi, 0);
    assert_eq!(oib, 0);
    assert_eq!(btr, 1);

    // Full first block
    let (bi, oib, btr) = compute_seek_params(0, 19);
    assert_eq!(bi, 0);
    assert_eq!(oib, 0);
    assert_eq!(btr, 1);

    // First block + one digit into second block → 2 blocks needed
    let (bi, oib, btr) = compute_seek_params(0, 20);
    assert_eq!(bi, 0);
    assert_eq!(oib, 0);
    assert_eq!(btr, 2);

    // blocks_to_read == ceil((offset_in_block + length) / 19) for various inputs
    for local_start in [0_usize, 1, 18, 19, 37, 38, 100, 1_000_000] {
        for length in [1_usize, 2, 18, 19, 20, 38, 39, 100] {
            let (_, offset_in_block, btr) = compute_seek_params(local_start, length);
            let expected_btr = (offset_in_block + length).div_ceil(19);
            assert_eq!(
                btr, expected_btr,
                "local_start={local_start} length={length}"
            );
        }
    }

    // At ~1M position, block_index must be non-zero
    // For 1M file (BlockID=0), local_start of position 999_995 is 999_994
    let local_start_1m = 999_994_usize;
    let (bi, oib, btr) = compute_seek_params(local_start_1m, 6);
    assert!(bi > 0, "block_index should be non-zero near 1M: got {bi}");
    assert_eq!(bi, local_start_1m / 19); // 999994 / 19 = 52631
    assert_eq!(oib, local_start_1m % 19); // 999994 % 19 = 5
                                          // Taking 6 digits with oib=5: need ceil((5+6)/19) = 1 block
    assert_eq!(btr, 1);

    // Cross-file: each file uses only the necessary blocks
    // Reading read_digits(&[file0, file1], 999_995, 20):
    //   file0: local_start=999994, to_take=6  → (52631, 5, 1)
    //   file1: local_start=0,      to_take=14 → (0, 0, 1)
    let (bi0, oib0, btr0) = compute_seek_params(999_994, 6);
    assert_eq!((bi0, oib0, btr0), (52631, 5, 1));

    let (bi1, oib1, btr1) = compute_seek_params(0, 14);
    assert_eq!((bi1, oib1, btr1), (0, 0, 1));
}

// ============================================================================
// Tests for YcdSeqBlockStream::new_from (sequential stream with start position)
// ============================================================================

/// Helper: collect all digits from a YcdSeqBlockStream started at start_position,
/// validating process_no and start_digit fields.
fn collect_single_from(
    path: impl AsRef<Path>,
    unit_size: i32,
    start_position: i64,
) -> io::Result<String> {
    let mut result = String::new();
    let mut stream = YcdSeqBlockStream::new_from(path, unit_size, start_position)?;
    let mut expected_start = start_position;
    let mut expected_process_no: i64 = 1;

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
    assert_eq!(result, expected, "digits from position {start} should match golden");
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
        YcdSeqBlockStream::new_from(&path, 1, 1)
            .unwrap_err()
            .kind(),
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

// ============================================================================
// Tests for YcdMultiFileStream::new_from (multi-file stream with start position)
// ============================================================================

/// Helper: collect digits from a YcdMultiFileStream started at start_position.
fn collect_multi_from<P: AsRef<Path>>(
    paths: &[P],
    unit_size: i32,
    start_position: i64,
) -> io::Result<String> {
    let mut result = String::new();
    let mut stream = YcdMultiFileStream::new_from(paths, unit_size, start_position)?;
    let mut expected_start = start_position;
    let mut expected_process_no: i64 = 1;

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
    let result =
        collect_multi_from(&[&f0.path, &f1.path, &f2.path], 19, 10).unwrap();
    assert_eq!(result, &all[9..]);

    // Start at position 20 (start of f1)
    let result =
        collect_multi_from(&[&f0.path, &f1.path, &f2.path], 19, 20).unwrap();
    assert_eq!(result, &all[19..]);

    // Start at position 30 (mid f1)
    let result =
        collect_multi_from(&[&f0.path, &f1.path, &f2.path], 19, 30).unwrap();
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
