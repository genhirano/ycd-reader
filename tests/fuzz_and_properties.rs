mod common;

use common::{golden_digits, one_million_path, two_million_path};
use ycd_reader::{compute_seek_params, YcdFileUtil, YcdIndex, YcdMultiFileStream};

/// A fast, deterministic 64-bit pseudo-random number generator (XorShift64).
/// Ensures tests are 100% reproducible without external dependencies.
struct SimpleRng {
    state: u64,
}

impl SimpleRng {
    fn new(seed: u64) -> Self {
        Self {
            state: if seed == 0 {
                0x5EED_CAFE_DEAD_BEEF
            } else {
                seed
            },
        }
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        x
    }

    /// Return a random usize in `[min, max]`.
    fn gen_range(&mut self, min: usize, max: usize) -> usize {
        assert!(min <= max);
        if min == max {
            return min;
        }
        let range = max - min + 1;
        min + (self.next_u64() as usize % range)
    }
}

#[test]
fn compute_seek_params_mathematical_invariants_exhaustive() {
    // Thoroughly test all (local_start, length) combinations in 0..200 x 1..200
    for local_start in 0..=200 {
        for length in 1..=200 {
            let (bi, oib, btr) = compute_seek_params(local_start, length);
            assert_eq!(
                bi * 19 + oib,
                local_start,
                "Position reconstruction failed: local_start={local_start}, length={length}"
            );
            assert!(
                oib < 19,
                "Offset in block must be in 0..19: got {oib} for local_start={local_start}"
            );
            assert!(
                btr * 19 >= oib + length,
                "Covered digits must be at least requested: btr={btr}, oib={oib}, length={length}"
            );
            assert!(
                (btr - 1) * 19 < oib + length,
                "Over-read check failed: btr={btr}, oib={oib}, length={length}"
            );
        }
    }
}

#[test]
fn random_range_reads_match_golden_and_each_other() {
    let golden = golden_digits();
    let total_digits = golden.len();
    assert_eq!(total_digits, 3_000_000);

    let files_1m = [
        one_million_path(0),
        one_million_path(1),
        one_million_path(2),
    ];
    let idx_1m = YcdIndex::build(&files_1m).unwrap();

    let files_2m = [two_million_path(0), one_million_path(2)];
    let idx_2m = YcdIndex::build(&files_2m).unwrap();

    let mut rng = SimpleRng::new(0x2026_1102);

    // Critical boundary points to sample around
    let key_positions = [
        1, 2, 18, 19, 20, 37, 38, 39, 999_980, 999_990, 1_000_000, 1_000_001, 1_000_010, 1_999_980,
        1_999_990, 2_000_000, 2_000_001, 2_000_010, 2_999_950, 2_999_980, 2_999_999, 3_000_000,
    ];

    // 1. Boundary-biased test cases
    for &pos in &key_positions {
        for &len in &[1, 2, 5, 18, 19, 20, 38, 50, 100] {
            if pos + len - 1 <= total_digits {
                let from_idx_1m = idx_1m.read_digits(pos, len).unwrap();
                let from_util_1m = YcdFileUtil::read_digits(&files_1m, pos, len).unwrap();
                let from_idx_2m = idx_2m.read_digits(pos, len).unwrap();
                let from_util_2m = YcdFileUtil::read_digits(&files_2m, pos, len).unwrap();
                let expected = &golden[pos - 1..pos - 1 + len];

                assert_eq!(
                    from_idx_1m, expected,
                    "Boundary mismatch at pos={pos}, len={len}"
                );
                assert_eq!(from_util_1m, expected);
                assert_eq!(from_idx_2m, expected);
                assert_eq!(from_util_2m, expected);
            }
        }
    }

    // 2. Uniform random fuzz testing across the entire 3,000,000 digits range
    const NUM_RANDOM_TRIALS: usize = 1_000;
    for _ in 0..NUM_RANDOM_TRIALS {
        let max_len = 1_500;
        let start = rng.gen_range(1, total_digits);
        let max_possible_len = total_digits - start + 1;
        let len = rng.gen_range(1, max_len.min(max_possible_len));

        let expected = &golden[start - 1..start - 1 + len];

        let from_idx = idx_1m.read_digits(start, len).unwrap();
        assert_eq!(
            from_idx, expected,
            "Random test failed at start={start}, len={len}"
        );

        let from_util = YcdFileUtil::read_digits(&files_1m, start, len).unwrap();
        assert_eq!(
            from_util, expected,
            "Random util test failed at start={start}, len={len}"
        );
    }
}

#[test]
fn random_unit_sizes_stream_invariants() {
    let golden = golden_digits();
    let files = [one_million_path(0), one_million_path(1)];
    let expected_total = 2_000_000;
    let expected = &golden[..expected_total];

    // Test a variety of unit sizes (small, irregular, block multiples, prime numbers)
    let unit_sizes: &[i32] = &[19, 20, 21, 37, 38, 50, 73, 100, 1000, 7777, 50_000];

    for &unit_size in unit_sizes {
        let mut stream = YcdMultiFileStream::new(&files, unit_size).unwrap();
        let mut collected = String::with_capacity(expected_total);
        let mut expected_process_no: i64 = 1;
        let mut expected_start: i64 = 1;

        while stream.has_next() {
            let unit = stream.next().unwrap();
            assert_eq!(unit.process_no, expected_process_no);
            assert_eq!(unit.start_digit, expected_start);
            expected_process_no += 1;
            expected_start += unit.value.len() as i64;
            collected.push_str(&unit.value);
        }

        assert_eq!(
            collected.len(),
            expected_total,
            "Length mismatch for unit_size={unit_size}"
        );
        assert_eq!(
            collected, expected,
            "Content mismatch for unit_size={unit_size}"
        );
    }
}

#[test]
fn random_new_from_positions_match_golden_subranges() {
    let golden = golden_digits();
    let files = [
        one_million_path(0),
        one_million_path(1),
        one_million_path(2),
    ];
    let total_digits = golden.len();

    let mut rng = SimpleRng::new(0xFEED_FACE_2026);

    // Sample random start positions and read the first 500 digits from each
    const NUM_TRIALS: usize = 50;
    for _ in 0..NUM_TRIALS {
        let start_pos = rng.gen_range(1, total_digits - 1000);
        let unit_size = rng.gen_range(19, 200) as i32;

        let mut stream = YcdMultiFileStream::new_from(&files, unit_size, start_pos as i64).unwrap();
        let mut read_digits = String::new();
        let to_read = 500;

        while read_digits.len() < to_read && stream.has_next() {
            read_digits.push_str(&stream.next().unwrap().value);
        }
        read_digits.truncate(to_read);

        let expected = &golden[start_pos - 1..start_pos - 1 + to_read];
        assert_eq!(
            read_digits, expected,
            "Stream new_from mismatch at start_pos={start_pos}, unit_size={unit_size}"
        );
    }
}
