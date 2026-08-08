# ycd_reader

`ycd_reader` is a Rust library for reading base-10 compressed digit files
(`.ycd`) produced by y-cruncher.

Each YCD payload stores up to 19 decimal digits in an unsigned 64-bit
little-endian block. The library restores those blocks to digit strings and
returns them in caller-selected processing units.

## Quick start

This repository includes real YCD fixtures under `tests/ycd`, and the examples
below use those files so they can be run as written from the repository root.
In application code, replace those paths with your own YCD files.

Only base-10 YCD files are supported. Positions are always 1-based and refer
only to decimal digits after the decimal point. The integer part, sign, and
decimal point are never returned.

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
        "tests/ycd/1000000/Pi - Dec - Chudnovsky - 0.ycd",
        "tests/ycd/1000000/Pi - Dec - Chudnovsky - 1.ycd",
    ];

    // Read 20 digits starting at position 999,995.
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
- `one_based_start_position` — 1-based index of the first digit to return. Position 1 is the first decimal digit (immediately after the "3." in Pi).
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
    let mut stream = YcdSeqBlockStream::new(
        "tests/ycd/1000000/Pi - Dec - Chudnovsky - 0.ycd",
        1_000,
    )?;

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
than the requested size. `next()` returns `UnexpectedEof` after the stream is
exhausted, so callers should guard reads with `has_next()`.

### Resuming single-file reading from an arbitrary position

`YcdSeqBlockStream::new_from` opens a YCD file and positions the stream so that
the first digit returned is at a caller-specified 1-based absolute position.
This lets an application save `unit.start_digit` at any point and later resume
reading from exactly that position.

```rust
use std::io;
use ycd_reader::YcdSeqBlockStream;

fn main() -> io::Result<()> {
    // Resume from position 500_000 (1-based, relative to all digits of Pi).
    let mut stream = YcdSeqBlockStream::new_from(
        "tests/ycd/1000000/Pi - Dec - Chudnovsky - 0.ycd",
        1_000,
        500_000,
    )?;

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

`start_position` must be inside the digit range covered by the file
(`digit_start .. digit_start + digit_length - 1`, inclusive). The stream seeks
directly to the compressed block that contains `start_position`; no bytes
before that block are read. As with `new`, callers should stop when
`has_next()` becomes false rather than calling `next()` unconditionally.

## Reading contiguous files (sequential)

`YcdMultiFileStream` joins an explicit ordered list of YCD files. It validates
that each file starts immediately after the preceding file and allows a
processing unit to cross file boundaries.

```rust
use std::io;
use ycd_reader::YcdMultiFileStream;

fn main() -> io::Result<()> {
    let files = [
        "tests/ycd/1000000/Pi - Dec - Chudnovsky - 0.ycd",
        "tests/ycd/1000000/Pi - Dec - Chudnovsky - 1.ycd",
        "tests/ycd/1000000/Pi - Dec - Chudnovsky - 2.ycd",
    ];
    let mut stream = YcdMultiFileStream::new(&files, 1_000)?;

    while stream.has_next() {
        let unit = stream.next()?;
        consume(&unit.value);
    }

    Ok(())
}

fn consume(_digits: &str) {}
```

`YcdMultiFileStream::next()` also returns `UnexpectedEof` after the stream is
exhausted, so the intended usage is the same `has_next()`-guarded loop shown
above.

### Resuming multi-file reading from an arbitrary position

`YcdMultiFileStream::new_from` opens an ordered list of contiguous YCD files
and positions the stream at a caller-specified 1-based absolute digit position.
All files in the list are validated for header correctness and list continuity
before any payload I/O begins. Files that end before `start_position` are
skipped; only the file that contains `start_position` and the files that follow
it are opened for streaming.

```rust
use std::io;
use ycd_reader::YcdMultiFileStream;

fn main() -> io::Result<()> {
    let files = [
        "tests/ycd/1000000/Pi - Dec - Chudnovsky - 0.ycd",
        "tests/ycd/1000000/Pi - Dec - Chudnovsky - 1.ycd",
        "tests/ycd/1000000/Pi - Dec - Chudnovsky - 2.ycd",
    ];

    // Resume from position 1_500_000 (inside the second file).
    let mut stream = YcdMultiFileStream::new_from(&files, 1_000, 1_500_000)?;

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

These APIs operate on a single file. Missing or invalid required fields,
non-base-10 headers, invalid payload blocks, and truncated files are reported
as `std::io::Error`. Contiguity validation for a list of files belongs to
`YcdFileUtil::read_digits` and `YcdMultiFileStream`.

## Tests

```text
cargo test
```

The integration suite uses
`tests/ycd/Pi - Dec - Chudnovsky_3000000.txt` as the golden result and checks
random-access reads, sequential streaming, resume-from-position behavior,
block and file boundary handling, and the documented error contract.
