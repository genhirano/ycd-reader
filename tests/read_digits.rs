mod common;

use std::io;

use common::{golden_digits, one_million_path, two_million_path, TempYcd};
use ycd_reader::{compute_seek_params, YcdFileUtil};

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
