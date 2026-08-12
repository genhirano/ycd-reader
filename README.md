# ycd-reader

This library reads decimal digits of π stored in y-cruncher YCD files.

`ycd-reader` is a Rust library for reading base-10 compressed digit files
(`.ycd`) produced by [y-cruncher](https://www.numberworld.org/y-cruncher/).
These files can contain enormous sequences of decimal digits, including the
digits of π, and may be spread across many files. `ycd-reader` is designed
to handle such large datasets comfortably even on low-power hardware.

For random-access reads, the library seeks directly to the compressed block
containing the requested digits instead of scanning or loading whole files.
Memory usage is therefore determined by the requested output and processing
unit size rather than by the size of the underlying YCD files.

Each 8-byte payload block stores up to 19 decimal digits as an unsigned 64-bit
little-endian integer. The library restores those blocks to digit strings and
returns them in caller-selected processing units.

## Choosing an API

| API | Use case |
| --- | --- |
| `YcdIndex::read_digits` | Repeated random-access reads over a stable file set |
| `YcdMultiFileStream` | Sequential processing across contiguous files |

## Random-access: read a specific digit range

`YcdFileUtil::read_digits` reads exactly `length` digits starting at a
1-based absolute position from an ordered list of contiguous YCD files.

The entire file list is validated before any payload I/O begins, and the
function seeks directly to the compressed block that contains the first
requested digit. No bytes before the target block are read.

Use this API when you need a one-off random-access read and do not need to
retain file-header metadata for subsequent requests.

## File header cache: `YcdIndex`

`YcdIndex` targets large, mostly static collections of YCD files and caches
the header information for a contiguous set of files in memory.

Subsequent calls such as `read_digits` use the cached header information to
locate the target file and seek directly to the desired block, without opening
or reading payload data from unrelated YCD files.

Build the index once, reuse it for fast reads, and rebuild it when the
underlying YCD files change.

### Building and reading the index

```rust
use std::io;
use ycd_reader::YcdIndex;

fn main() -> io::Result<()> {
    // The YCD files that make up the target digit set.
    let files = [
        "Pi - Dec - Chudnovsky - 0.ycd",
        "Pi - Dec - Chudnovsky - 1.ycd",
    ];

    // Build the index once: reads all headers and validates continuity.
    let mut index = YcdIndex::build(&files)?;

    // Fast random-access — only the file(s) containing the requested range
    // are opened. All other files are skipped entirely.
    let digits = index.read_digits(999_995, 20)?;
    assert_eq!(digits, "45815130927562832084");

    Ok(())
}
```

### Stale-index detection

Once a YCD file set has been prepared, it is usually kept stable for long
periods. If the set is changed while the index is still in use, the cached
header metadata may no longer match the current files.

Each time a file is actually read, its current `file_size` and last-modified
time are compared against the snapshot recorded at build time. If they differ,
`read_digits` returns `io::ErrorKind::InvalidData` with an explicit
**"index is stale"** message.

There is no silent fallback to a full-scan path. The caller must explicitly
call `rebuild` to refresh the index.

### `YcdIndex::read_digits` errors

| Condition | `io::ErrorKind` |
| --- | --- |
| Index is empty, position 0, or length 0 | `InvalidInput` |
| Start or end position outside indexed range | `InvalidInput` |
| `start + length` overflows `usize` | `InvalidData` |
| File size or mtime differs from index snapshot | `InvalidData` |
| File does not exist | `NotFound` |
| Payload truncated within logical range | `UnexpectedEof` |
| Output string pre-allocation failure | `Other` |

## Reading contiguous files (sequential)

`YcdMultiFileStream` reads a contiguous sequence of YCD files as a single
logical digit stream. The files are validated in order before reading begins,
so the stream can reliably treat the collection as one continuous range even
when a processing unit spans a file boundary.

This is useful when you want to consume the digit stream in order, without
manually stitching the file boundaries together. Each yielded unit contains a
slice of decimal digits, and the stream automatically continues across file
boundaries when needed.

```rust
use std::io;
use ycd_reader::YcdMultiFileStream;

fn main() -> io::Result<()> {
    let files = [
        "Pi - Dec - Chudnovsky - 0.ycd",
        "Pi - Dec - Chudnovsky - 1.ycd",
        "Pi - Dec - Chudnovsky - 2.ycd",
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

## File-range rules

| Header field | Logical length of the file |
| --- | --- |
| `TotalDigits > 0` | `min(Blocksize, TotalDigits − Blocksize × BlockID)` |
| `TotalDigits == 0` | `Blocksize` |

A file with `TotalDigits == 0` is assumed to contain exactly `Blocksize`
digits. If the actual payload is shorter than the logical range,
`UnexpectedEof` is returned.

## Errors

The non-indexed APIs validate the requested range and the ordered YCD file set
before reading payload data.

| Condition | `io::ErrorKind` |
| --- | --- |
| Empty file list, position 0, or length 0 | `InvalidInput` |
| Start position out of range, or end exceeds range | `InvalidInput` |
| Gap, duplicate, or reversed files in list | `InvalidInput` |
| Invalid header, non-base-10, or corrupt block | `InvalidData` |
| Position or offset calculation overflow | `InvalidData` |
| File does not exist | `NotFound` |
| Payload truncated within logical range | `UnexpectedEof` |
| Output string pre-allocation failure | `Other` |

## License

Licensed under either of the following, at your option:

- Apache License, Version 2.0 (LICENSE-APACHE)
- MIT license (LICENSE-MIT)

## Acknowledgments

This library exists to read digit files produced by
[y-cruncher](https://www.numberworld.org/y-cruncher/), Alexander J. Yee's
multi-threaded program for high-performance and record-setting computation
of mathematical constants.

The `.ycd` format and its semantics originate from y-cruncher. This project is
an independent, unofficial reader and is not affiliated with or endorsed by
its author.