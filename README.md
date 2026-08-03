# ycd_reader

`ycd_reader` is a Rust library for reading base-10 compressed digit files
(`.ycd`) produced by y-cruncher.

Each YCD payload stores up to 19 decimal digits in an unsigned 64-bit
little-endian block. The library restores those blocks to digit strings and
returns them in caller-selected processing units.

## Random-access: read a specific digit range

`YcdFileUtil::read_digits` reads exactly `length` digits starting at a
1-based absolute position from an ordered list of contiguous YCD files.
The entire file list is validated before any payload I/O begins, and the
function seeks directly to the compressed block that contains the first
requested digit — no bytes before the target block are read.

```rust
use std::io;
use ycd_reader::YcdFileUtil;

fn main() -> io::Result<()> {
    let files = [
        "Pi - Dec - Chudnovsky - 0.ycd",
        "Pi - Dec - Chudnovsky - 1.ycd",
    ];

    // Read 20 digits starting at position 999,995 (1-based, decimal digits only).
    // Position 1 is the first digit after the decimal point — "1" in "3.14159…"
    // The integer part, sign, and decimal point are never included.
    let digits = YcdFileUtil::read_digits(&files, 999_995, 20)?;
    assert_eq!(digits, "45815130927562832084");

    Ok(())
}
```

### Signature

```rust
pub fn read_digits<P: AsRef<Path>>(
    files: &[P],
    one_based_start_position: usize,
    length: usize,
) -> io::Result<String>
```

- `files` — An ordered slice of YCD file paths. The first file does not need
  to have `BlockID = 0`; positions are derived from each file's header.
  The files must be contiguous: each file must start immediately after the
  preceding file ends. All files are validated before any I/O on the payload.
- `one_based_start_position` — 1-based index of the first digit to return.
  Position 1 is the first decimal digit (immediately after the "3." in Pi).
  The integer part, sign, and decimal point are never returned.
- `length` — Number of digits to return. The result string is exactly
  `length` ASCII bytes.

### File-range rules

| Header field | Logical length of the file |
|---|---|
| `TotalDigits > 0` | `min(Blocksize, TotalDigits − Blocksize × BlockID)` |
| `TotalDigits == 0` | `Blocksize` |

A file with `TotalDigits == 0` is assumed to contain exactly `Blocksize`
digits. If the actual payload is shorter than the logical range,
`UnexpectedEof` is returned.

### Errors

| Condition | `io::ErrorKind` |
|---|---|
| Empty file list, position 0, or length 0 | `InvalidInput` |
| Start position out of range, or end exceeds range | `InvalidInput` |
| Gap, duplicate, or reversed files in list | `InvalidInput` |
| Invalid header, non-base-10, or corrupt block | `InvalidData` |
| Position or offset calculation overflow | `InvalidData` |
| File does not exist | `NotFound` |
| Payload truncated within logical range | `UnexpectedEof` |
| Output string pre-allocation failure | `Other` |

### Memory note

`read_digits` allocates a `String` of exactly `length` bytes to hold the
result. A large `length` requires the same amount of heap memory.

## Single-file reading

```rust
use std::io;
use ycd_reader::YcdSeqBlockStream;

fn main() -> io::Result<()> {
    let mut stream = YcdSeqBlockStream::new("digits-0.ycd", 1_000)?;

    while stream.has_next() {
        let unit = stream.next()?;
        println!(
            "unit {} starts at digit {} and contains {} digits",
            unit.process_no,
            unit.start_digit,
            unit.value.len()
        );
    }

    Ok(())
}
```

The processing-unit size must be at least 19. The final unit can be shorter
than the requested size.

## Reading contiguous files (sequential)

`YcdMultiFileStream` joins an explicit ordered list of YCD files. It validates
that each file starts immediately after the preceding file and allows a
processing unit to cross file boundaries.

```rust
use std::io;
use ycd_reader::YcdMultiFileStream;

fn main() -> io::Result<()> {
    let files = ["digits-0.ycd", "digits-1.ycd", "digits-2.ycd"];
    let mut stream = YcdMultiFileStream::new(&files, 1_000)?;

    while stream.has_next() {
        let unit = stream.next()?;
        consume(&unit.value);
    }

    Ok(())
}

fn consume(_digits: &str) {}
```

## Header inspection

`YcdFileUtil::get_ycd_header` returns recognized header fields, and
`YcdFileUtil::get_header_size` returns the byte offset at which compressed
blocks begin. Headers can use CRLF or LF line endings and are not restricted
to a fixed buffer size.

Only base-10 YCD files are supported. Missing or invalid required fields,
invalid payload blocks, truncated files, and noncontiguous file lists are
reported as `std::io::Error`.

## Tests

```text
cargo test
```

The integration suite uses
`tests/ycd/Pi - Dec - Chudnovsky_3000000.txt` as the golden result. It covers:

- `read_digits`: position 1 with various lengths, arbitrary middle positions,
  19-digit block boundaries (before/after/crossing), YCD file boundaries
  (before/after/crossing) in both 1M×3 and 2M+1M layouts, leading zeroes,
  reads spanning multiple files, the complete error-contract table, and
  direct-seek unit tests that verify `block_index`, `offset_in_block`, and
  `blocks_to_read` without running full I/O.
- Sequential streaming: all three million digits with multiple processing-unit
  sizes, file-boundary crossings, final partial units, and malformed inputs.
