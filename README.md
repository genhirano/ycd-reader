# ycd-reader

[![Crates.io](https://img.shields.io/crates/v/ycd-reader.svg)](https://crates.io/crates/ycd-reader)
[![docs.rs](https://docs.rs/ycd-reader/badge.svg)](https://docs.rs/ycd-reader)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

This library reads decimal digits stored in y-cruncher YCD files — the
digits of π or of any other constant.

`ycd-reader` is a Rust library for reading base-10 compressed digit files
(`.ycd`) produced by [y-cruncher](https://www.numberworld.org/y-cruncher/).
y-cruncher computes many mathematical constants — π, e, √2, and more — and
writes `.ycd` files in both base 10 and base 16. This library reads the
base-10 files of any such constant: nothing in it is π-specific, and the
constant stored in the file is never inspected. Base-16 files are rejected.

Digit files can contain enormous sequences of decimal digits and may be
spread across many files. `ycd-reader` is designed to handle such large
datasets comfortably even on low-power hardware, seeking directly to the
compressed block containing the requested digits instead of scanning or
loading whole files.

For API details, usage guidance for each type, and the full error reference,
see the [documentation on docs.rs](https://docs.rs/ycd-reader).

## Installation

```toml
[dependencies]
ycd-reader = "0.2"
```

Requires Rust 1.85 or later (2021 edition).

## Quick start

### Random-access read

```rust
use std::io;
use ycd_reader::YcdIndex;

fn main() -> io::Result<()> {
    // List the split .ycd files in order.
    let files = [
        "Pi - Dec - Chudnovsky - 0.ycd",
        "Pi - Dec - Chudnovsky - 1.ycd",
    ];

    // Build the index once: reads all headers and validates continuity.
    let mut index = YcdIndex::build(&files)?;

    // Fast random-access — only the file(s) containing the requested range
    // are opened. All other files are skipped entirely.
    // Read 20 digits starting at the 999,995th decimal digit of π (1-indexed,
    // so this is digits 999,995–1,000,014). This range straddles both files
    // above (file 0 ends at digit 1,000,000), but only those two are opened.
    let digits = index.read_digits(999_995, 20)?;
    assert_eq!(digits, "45815130927562832084");

    Ok(())
}
```

### Sequential read

For streaming through digits from the start (e.g. exporting all digits),
use `YcdMultiFileStream` instead of building an index for random access.

```rust
use std::io;
use ycd_reader::YcdMultiFileStream;

fn main() -> io::Result<()> {
    let files = [
        "Pi - Dec - Chudnovsky - 0.ycd",
        "Pi - Dec - Chudnovsky - 1.ycd",
    ];

    // Walk the digits in fixed-size chunks of 19,000 digits (1,000 compressed
    // blocks per chunk), advancing across file boundaries automatically.
    // The chunk size must be at least 19 (one compressed block's worth of
    // digits); smaller values are rejected with InvalidInput.
    // A chunk size that is a multiple of 19 is recommended: each compressed
    // block decodes to exactly 19 digits, so a multiple of 19 lines up with
    // block boundaries and every chunk is filled from whole blocks, with no
    // leftover digits carried over (and re-copied) into the next chunk.
    let mut stream = YcdMultiFileStream::new(&files, 19000)?;
    while stream.has_next() {
        let unit = stream.next()?;
        println!("digit {}: {}", unit.start_digit, unit.value);
    }

    Ok(())
}
```

### Other APIs

Beyond the two examples above, the crate also provides:

- `YcdSeqBlockStream` — sequential read over a single `.ycd` file (the single-file counterpart to `YcdMultiFileStream`).
- `new_from` (on both stream types) — start sequential reading from an arbitrary digit position instead of the first.
- `YcdFileUtil::read_digits` — a one-off random-access read across files without building/keeping a `YcdIndex`.
- `YcdFileUtil::get_ycd_header` / `get_header_size` — inspect a file's raw header fields (digit count, block size, etc.) directly.

## Contributing

Issues and pull requests are welcome. Before submitting a change, run:

```sh
cargo test
```

## License

Licensed under either of the following, at your option:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

## Acknowledgments

This library exists to read digit files produced by
[y-cruncher](https://www.numberworld.org/y-cruncher/), Alexander J. Yee's
multi-threaded program for high-performance and record-setting computation
of mathematical constants.

The `.ycd` format and its semantics originate from y-cruncher. This project is
an independent, unofficial reader and is not affiliated with or endorsed by
its author.
