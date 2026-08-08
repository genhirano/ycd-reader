mod common;

use std::io;
use std::time::{Duration, SystemTime};

use common::{golden_digits, one_million_path, two_million_path, TempYcd};
use ycd_reader::{YcdFileUtil, YcdIndex};

// ─── Build tests ─────────────────────────────────────────────────────────────

#[test]
fn build_empty_list_returns_error() {
    let empty: [&str; 0] = [];
    assert_eq!(
        YcdIndex::build(&empty).unwrap_err().kind(),
        io::ErrorKind::InvalidInput
    );
}

#[test]
fn build_single_file_succeeds() {
    let idx = YcdIndex::build(&[one_million_path(0)]).unwrap();
    assert_eq!(idx.len(), 1);
    assert!(!idx.is_empty());
}

#[test]
fn build_three_files_succeeds() {
    let files = [
        one_million_path(0),
        one_million_path(1),
        one_million_path(2),
    ];
    let idx = YcdIndex::build(&files).unwrap();
    assert_eq!(idx.len(), 3);
}

#[test]
fn build_noncontiguous_files_returns_error() {
    // Gap: files 0 and 2 (missing file 1)
    let files = [one_million_path(0), one_million_path(2)];
    assert_eq!(
        YcdIndex::build(&files).unwrap_err().kind(),
        io::ErrorKind::InvalidInput
    );
}

#[test]
fn build_stores_correct_metadata() {
    let files = [one_million_path(0), one_million_path(1)];
    let idx = YcdIndex::build(&files).unwrap();
    let entries = idx.entries();

    // First file: BlockID=0, 1_000_000 digits, starts at position 1
    assert_eq!(entries[0].file_start, 1);
    assert_eq!(entries[0].file_length, 1_000_000);

    // Second file: BlockID=1, 1_000_000 digits, starts at position 1_000_001
    assert_eq!(entries[1].file_start, 1_000_001);
    assert_eq!(entries[1].file_length, 1_000_000);
}

#[test]
fn build_stores_absolute_path_when_given_relative() {
    // Create a temporary YCD file to use as a test fixture.
    let file = TempYcd::valid("1415926535897932384", 19, 0, "\n", "").unwrap();

    // Construct a relative path from the current working directory.
    let abs_path = file.path.canonicalize().unwrap();
    let cwd = std::env::current_dir().unwrap();
    let rel_path = abs_path
        .strip_prefix(&cwd)
        .unwrap_or(abs_path.as_path())
        .to_path_buf();

    // Use the relative path to build the index.
    let idx = YcdIndex::build(&[&rel_path]).unwrap();

    // The stored path must be absolute regardless of how it was supplied.
    assert!(
        idx.entries()[0].path.is_absolute(),
        "expected an absolute path, got: {}",
        idx.entries()[0].path.display()
    );
}

#[test]
fn rebuild_replaces_index() {
    let files1 = [one_million_path(0)];
    let mut idx = YcdIndex::build(&files1).unwrap();
    assert_eq!(idx.len(), 1);

    let files3 = [
        one_million_path(0),
        one_million_path(1),
        one_million_path(2),
    ];
    idx.rebuild(&files3).unwrap();
    assert_eq!(idx.len(), 3);
}

#[test]
fn rebuild_leaves_index_unchanged_on_error() {
    let files1 = [one_million_path(0)];
    let mut idx = YcdIndex::build(&files1).unwrap();

    // Attempt to rebuild with an invalid (non-contiguous) list.
    let bad = [one_million_path(0), one_million_path(2)];
    assert!(idx.rebuild(&bad).is_err());

    // Original index is intact.
    assert_eq!(idx.len(), 1);
}

// ─── read_digits correctness ─────────────────────────────────────────────────

#[test]
fn read_digits_matches_ycd_file_util() {
    let golden = golden_digits();
    let files = [
        one_million_path(0),
        one_million_path(1),
        one_million_path(2),
    ];
    let idx = YcdIndex::build(&files).unwrap();

    let test_cases: &[(usize, usize)] = &[
        (1, 1),
        (1, 19),
        (1, 20),
        (1, 100),
        (19, 1),
        (20, 1),
        (19, 2),
        (500, 1),
        (12_345, 50),
        (500_000, 100),
        (999_990, 20),
        (1_000_000, 1),
        (1_000_001, 1),
        (1_000_000, 2),
        (1_999_990, 20),
        (2_000_000, 1),
        (2_000_001, 1),
        (3_000_000, 1),
        (2_999_996, 5),
    ];

    for &(start, len) in test_cases {
        let from_util = YcdFileUtil::read_digits(&files, start, len).unwrap();
        let from_idx = idx.read_digits(start, len).unwrap();
        assert_eq!(from_util, from_idx, "mismatch at start={start} len={len}");
        assert_eq!(
            from_idx,
            &golden[start - 1..start - 1 + len],
            "golden mismatch at start={start} len={len}"
        );
    }
}

#[test]
fn read_digits_two_million_layout() {
    let golden = golden_digits();
    let files = [two_million_path(0), one_million_path(2)];
    let idx = YcdIndex::build(&files).unwrap();

    let test_cases: &[(usize, usize)] = &[
        (1, 100),
        (1_999_990, 20),
        (2_000_000, 1),
        (2_000_001, 1),
        (2_000_000, 2),
        (2_999_996, 5),
    ];
    for &(start, len) in test_cases {
        let from_util = YcdFileUtil::read_digits(&files, start, len).unwrap();
        let from_idx = idx.read_digits(start, len).unwrap();
        assert_eq!(from_util, from_idx, "mismatch at start={start} len={len}");
        assert_eq!(
            from_idx,
            &golden[start - 1..start - 1 + len],
            "golden mismatch at start={start} len={len}"
        );
    }
}

#[test]
fn read_digits_non_zero_blockid_first_file() {
    let golden = golden_digits();
    let files = [one_million_path(1), one_million_path(2)];
    let idx = YcdIndex::build(&files).unwrap();

    let result = idx.read_digits(1_000_001, 10).unwrap();
    assert_eq!(result, &golden[1_000_000..1_000_010]);
}

// ─── read_digits error cases ─────────────────────────────────────────────────

#[test]
fn read_digits_error_position_zero() {
    let idx = YcdIndex::build(&[one_million_path(0)]).unwrap();
    assert_eq!(
        idx.read_digits(0, 1).unwrap_err().kind(),
        io::ErrorKind::InvalidInput
    );
}

#[test]
fn read_digits_error_length_zero() {
    let idx = YcdIndex::build(&[one_million_path(0)]).unwrap();
    assert_eq!(
        idx.read_digits(1, 0).unwrap_err().kind(),
        io::ErrorKind::InvalidInput
    );
}

#[test]
fn read_digits_error_out_of_range() {
    let idx = YcdIndex::build(&[one_million_path(1)]).unwrap();

    // One before list start (BlockID=1 starts at 1_000_001)
    assert_eq!(
        idx.read_digits(1_000_000, 1).unwrap_err().kind(),
        io::ErrorKind::InvalidInput
    );
    // One after list end
    assert_eq!(
        idx.read_digits(2_000_001, 1).unwrap_err().kind(),
        io::ErrorKind::InvalidInput
    );
    // Valid start but end exceeds by 1
    assert_eq!(
        idx.read_digits(2_000_000, 2).unwrap_err().kind(),
        io::ErrorKind::InvalidInput
    );
}

#[test]
fn read_digits_error_overflow() {
    let idx = YcdIndex::build(&[one_million_path(0)]).unwrap();
    assert_eq!(
        idx.read_digits(1, usize::MAX).unwrap_err().kind(),
        io::ErrorKind::InvalidData
    );
}

// ─── Stale-index detection ───────────────────────────────────────────────────

#[test]
fn stale_index_file_size_change_detected() {
    // Build an index from a temporary YCD file, then modify the file on disk
    // and verify that read_digits returns a stale-index error.
    let file = TempYcd::valid("1415926535897932384", 19, 0, "\n", "").unwrap();
    let mut idx = YcdIndex::build(&[&file.path]).unwrap();

    // Corrupt the file by appending a byte (size changes).
    let mut contents = std::fs::read(&file.path).unwrap();
    contents.push(0xff);
    std::fs::write(&file.path, &contents).unwrap();

    // Refresh the mtime snapshot to an old value to force size mismatch.
    // (The file size already differs, so the error should fire before mtime is checked.)
    let err = idx.read_digits(1, 1).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    assert!(
        err.to_string().contains("stale"),
        "expected 'stale' in error message, got: {err}"
    );

    // After rebuilding, reads succeed again.
    idx.rebuild(&[&file.path]).unwrap();
    assert!(idx.read_digits(1, 1).is_ok());
}

#[test]
fn stale_index_mtime_change_detected() {
    let file = TempYcd::valid("1415926535897932384", 19, 0, "\n", "").unwrap();
    let idx = YcdIndex::build(&[&file.path]).unwrap();

    // Manually set a modified time that differs from the snapshot.
    let new_mtime = SystemTime::UNIX_EPOCH
        .checked_add(Duration::from_secs(1))
        .unwrap();
    let new_mtime_ft = filetime::FileTime::from_system_time(new_mtime);
    filetime::set_file_mtime(&file.path, new_mtime_ft).unwrap();

    let err = idx.read_digits(1, 1).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    assert!(
        err.to_string().contains("stale"),
        "expected 'stale' in error message, got: {err}"
    );
}

// ─── Binary-search: unopened-file verification ───────────────────────────────
//
// This test verifies that read_digits only validates/opens the file(s) it
// actually needs.  We build an index from two temporary copies of the fixture
// files, then append a byte to the second copy so that its on-disk size differs
// from the snapshot.  A read confined entirely to the first file must succeed
// (the stale second file is never touched); a read that spans into the second
// file must fail with a stale-index error.

#[test]
fn index_does_not_open_unneeded_files() {
    // Make writable temporary copies so we can modify them without touching the
    // shared fixture files.
    let src0 = std::fs::read(one_million_path(0)).unwrap();
    let src1 = std::fs::read(one_million_path(1)).unwrap();
    let tmp0 = TempYcd::from_bytes(&src0).unwrap();
    let tmp1 = TempYcd::from_bytes(&src1).unwrap();
    let files = [&tmp0.path, &tmp1.path];

    // Build the index before modifying either file.
    let idx = YcdIndex::build(&files).unwrap();

    // Corrupt the second file on disk by appending a byte so that its size
    // differs from the snapshot. A read touching only the first file must
    // succeed; the stale second file is never opened.
    let mut contents1 = std::fs::read(&tmp1.path).unwrap();
    contents1.push(0xff);
    std::fs::write(&tmp1.path, &contents1).unwrap();

    // A read confined entirely to the first file must succeed.
    let result = idx.read_digits(1, 100);
    assert!(
        result.is_ok(),
        "read within first file should not check second file, but got: {:?}",
        result.unwrap_err()
    );

    // A read that touches the second file must fail with stale-index.
    let err = idx.read_digits(1_000_000, 2).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    assert!(err.to_string().contains("stale"));
}
