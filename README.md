# ycd_reader

`ycd_reader` is a Rust library for reading base-10 compressed digit files
(`.ycd`) produced by y-cruncher.

Each YCD payload stores up to 19 decimal digits in an unsigned 64-bit
little-endian block. The library restores those blocks to digit strings and
returns them in caller-selected processing units.

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

## Reading contiguous files

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
`tests/ycd/Pi - Dec - Chudnovsky_3000000.txt` as the golden result. It checks
all three million digits with multiple processing-unit sizes, arbitrary
ranges, both one-million- and two-million-digit YCD layouts, file-boundary
crossings, final partial units, leading zeroes, and malformed inputs.
