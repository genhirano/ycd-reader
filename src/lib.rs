//! `ycd-reader` is a Rust library for reading decimal digits from base-10
//! compressed digit files (`.ycd`) produced by
//! [y-cruncher](https://www.numberworld.org/y-cruncher/) — the digits of π
//! or of any other constant.
//!
//! y-cruncher computes many mathematical constants — π, e, √2, and more —
//! and writes `.ycd` files in both base 10 and base 16. This library reads
//! the base-10 files of any such constant: nothing in it is π-specific, and
//! the constant stored in the file is never inspected. Base-16 files are
//! rejected.
//!
//! Digit files can contain enormous sequences of decimal digits and may be
//! spread across many files. `ycd-reader` is designed to handle such large
//! datasets comfortably even on low-power hardware.
//!
//! For random-access reads, the library seeks directly to the compressed block
//! containing the requested digits instead of scanning or loading whole files.
//! Memory usage is therefore determined by the requested output and processing
//! unit size rather than by the size of the underlying YCD files.
//!
//! Each 8-byte payload block stores up to 19 decimal digits as an unsigned 64-bit
//! little-endian integer. The library restores those blocks to digit strings and
//! returns them in caller-selected processing units. `unit_size` must be at
//! least 19 (one block's worth of digits); smaller values make the stream
//! constructors return `io::ErrorKind::InvalidInput`. A `unit_size` that is a
//! multiple of 19 is recommended: it lines up with block boundaries, so every
//! unit is filled from whole blocks with no leftover digits carried over
//! (and re-copied) into the next unit.
//!
//! # Choosing an API
//!
//! | API | Use case |
//! | --- | --- |
//! | [`YcdFileUtil::read_digits`] | One-off random-access read, no state retained |
//! | [`YcdIndex::read_digits`] | Repeated random-access reads over a stable file set |
//! | [`YcdMultiFileStream`] | Sequential processing across contiguous files |
//! | [`YcdSeqBlockStream`] | Sequential processing of a single file |
//!
//! Both stream types also expose `new_from`, which starts iteration at an
//! arbitrary 1-based digit position instead of the beginning of the file (or
//! file set), so a stream can resume mid-range without re-reading from the
//! start. See [`YcdSeqBlockStream::new_from`] and
//! [`YcdMultiFileStream::new_from`].
//!
//! # Random-access: read a specific digit range
//!
//! [`YcdFileUtil::read_digits`] reads exactly `length` digits starting at a
//! 1-based absolute position from an ordered list of contiguous YCD files.
//!
//! The entire file list is validated before any payload I/O begins, and the
//! function seeks directly to the compressed block that contains the first
//! requested digit. No bytes before the target block are read.
//!
//! Use this API when you need a one-off random-access read and do not need to
//! retain file-header metadata for subsequent requests.
//!
//! # File header cache: [`YcdIndex`]
//!
//! `YcdIndex` targets large, mostly static collections of YCD files and caches
//! the header information for a contiguous set of files in memory.
//!
//! Subsequent calls such as `read_digits` use the cached header information to
//! locate the target file and seek directly to the desired block, without opening
//! or reading payload data from unrelated YCD files.
//!
//! Build the index once, reuse it for fast reads, and rebuild it when the
//! underlying YCD files change.
//!
//! ## Building and reading the index
//!
//! ```rust,no_run
//! use std::io;
//! use ycd_reader::YcdIndex;
//!
//! fn main() -> io::Result<()> {
//!     // The YCD files that make up the target digit set.
//!     let files = [
//!         "Pi - Dec - Chudnovsky - 0.ycd",
//!         "Pi - Dec - Chudnovsky - 1.ycd",
//!     ];
//!
//!     // Build the index once: reads all headers and validates continuity.
//!     let mut index = YcdIndex::build(&files)?;
//!
//!     // Fast random-access — only the file(s) containing the requested range
//!     // are opened. All other files are skipped entirely.
//!     let digits = index.read_digits(999_995, 20)?;
//!     assert_eq!(digits, "45815130927562832084");
//!
//!     Ok(())
//! }
//! ```
//!
//! ## Stale-index detection
//!
//! Once a YCD file set has been prepared, it is usually kept stable for long
//! periods. If the set is changed while the index is still in use, the cached
//! header metadata may no longer match the current files.
//!
//! Each time a file is actually read, its current `file_size` and last-modified
//! time are compared against the snapshot recorded at build time. If they differ,
//! `read_digits` returns `io::ErrorKind::InvalidData` with an explicit
//! **"index is stale"** message.
//!
//! There is no silent fallback to a full-scan path. The caller must explicitly
//! call `rebuild` to refresh the index.
//!
//! For the full table of error conditions, see [`YcdIndex::read_digits`].
//!
//! # Reading contiguous files (sequential)
//!
//! [`YcdMultiFileStream`] reads a contiguous sequence of YCD files as a single
//! logical digit stream. The files are validated in order before reading begins,
//! so the stream can reliably treat the collection as one continuous range even
//! when a processing unit spans a file boundary.
//!
//! This is useful when you want to consume the digit stream in order, without
//! manually stitching the file boundaries together. Each yielded unit contains a
//! slice of decimal digits, and the stream automatically continues across file
//! boundaries when needed.
//!
//! Processing only one file? [`YcdSeqBlockStream`] is the single-file
//! counterpart that `YcdMultiFileStream` uses internally, and is available
//! directly when a multi-file stream is more than you need.
//!
//! ```rust,no_run
//! use std::io;
//! use ycd_reader::YcdMultiFileStream;
//!
//! fn main() -> io::Result<()> {
//!     let files = [
//!         "Pi - Dec - Chudnovsky - 0.ycd",
//!         "Pi - Dec - Chudnovsky - 1.ycd",
//!         "Pi - Dec - Chudnovsky - 2.ycd",
//!     ];
//!
//!     // 19,000 digits per unit — a multiple of 19, so every unit is filled
//!     // from whole compressed blocks (see the crate-level notes above).
//!     let mut stream = YcdMultiFileStream::new(&files, 19_000)?;
//!
//!     while stream.has_next() {
//!         let unit = stream.next()?;
//!         consume(&unit.value);
//!     }
//!
//!     Ok(())
//! }
//!
//! fn consume(_digits: &str) {}
//! ```
//!
//! # File-range rules
//!
//! | Header field | Logical length of the file |
//! | --- | --- |
//! | `TotalDigits > 0` | `min(Blocksize, TotalDigits − Blocksize × BlockID)` |
//! | `TotalDigits == 0` | `Blocksize` |
//!
//! A file with `TotalDigits == 0` is assumed to contain exactly `Blocksize`
//! digits. If the actual payload is shorter than the logical range,
//! `UnexpectedEof` is returned.
//!
//! # Errors
//!
//! The non-indexed APIs validate the requested range and the ordered YCD file
//! set before reading payload data. Each read API documents its own error
//! conditions in full: see [`YcdFileUtil::read_digits`] for the non-indexed
//! random-access path and [`YcdIndex::read_digits`] for the indexed path.

#![warn(missing_docs)]

use std::collections::HashMap;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Seek};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use strum_macros::{AsRefStr, EnumString};

const DIGITS_PER_BLOCK: usize = 19;

/// A field name in the text header of a YCD file.
///
/// The header of a `.ycd` file is a sequence of `Name: value` text lines
/// terminated by an `EndHeader` marker. This enum identifies the fields this
/// library reads; it is used as the key type of the map returned by
/// [`YcdFileUtil::get_ycd_header`].
///
/// Note that at the header level a "block" means one file's digit span
/// (`Blocksize` digits per file), not the 8-byte compressed words of the
/// payload.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, AsRefStr, EnumString)]
pub enum YcdHeaderInfoElem {
    /// Version string of the YCD file format (e.g. `1.1.0`). Must be non-empty.
    FileVersion,
    /// Numeric base of the stored digits. y-cruncher writes base 10 and
    /// base 16 files; this library accepts base 10 only.
    Base,
    /// The leading digits of the constant as plain text (e.g. `3.14159...`),
    /// stored for human inspection. Must be non-empty; the content is not
    /// otherwise validated.
    FirstDigits,
    /// Total number of digits in the entire digit set across all files.
    /// May be `0` (unknown), in which case each file is assumed to contain
    /// exactly `Blocksize` digits.
    TotalDigits,
    /// Total number of files (header-level blocks) in the set. Optional;
    /// validated as a nonnegative integer when present but otherwise unused.
    TotalBlocks,
    /// Number of digits each file in the set holds.
    Blocksize,
    /// 0-based index of this file within the set. The file covers absolute
    /// digit positions `Blocksize × BlockID + 1` onward.
    BlockID,
    /// Marker line that terminates the header section. Not a `Name: value`
    /// field; it never appears in the returned header map.
    EndHeader,
}

/// One unit of digits yielded by [`YcdSeqBlockStream`] or [`YcdMultiFileStream`].
///
/// Every call to a stream's `next` produces one `YcdProcessUnit` holding up to
/// `unit_size` decoded digits. All units except possibly the last are exactly
/// `unit_size` digits long.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct YcdProcessUnit {
    /// 1-based sequence number of this unit within the stream
    /// (1 for the first unit returned, incremented by 1 for each `next` call).
    pub process_no: i64,
    /// 1-based absolute digit position of the first digit in `value`.
    pub start_digit: i64,
    /// The decoded digits as an ASCII decimal string.
    pub value: String,
}

impl YcdProcessUnit {
    /// Create a unit from its parts. Normally only used internally by the
    /// stream types; provided for constructing test fixtures.
    pub fn new(process_no: i64, start_digit: i64, value: String) -> Self {
        Self {
            process_no,
            start_digit,
            value,
        }
    }
}

struct YcdMetadata {
    header: HashMap<YcdHeaderInfoElem, String>,
    data_offset: u64,
    digit_length: i64,
    digit_start: i64,
}

/// Sequential reader over a single YCD file, yielding digits in fixed-size units.
///
/// This is the single-file counterpart to [`YcdMultiFileStream`] (which uses
/// it internally). Iterate with the [`has_next`](Self::has_next) /
/// [`next`](Self::next) pair:
///
/// - `has_next` returns `true` while undelivered digits remain.
/// - `next` returns the next [`YcdProcessUnit`]. Every unit except possibly
///   the last is exactly `unit_size` digits long.
/// - Calling `next` after the stream is exhausted (i.e. when `has_next`
///   returns `false`) returns `io::ErrorKind::UnexpectedEof` with a
///   "No more data to read" message.
///
/// The stream deliberately does not implement [`Iterator`]: `next` returns
/// `io::Result<&YcdProcessUnit>` — a fallible, borrowed result whose reference
/// is only valid until the following `next` call — which the `Iterator`
/// contract cannot express.
#[derive(Debug)]
pub struct YcdSeqBlockStream {
    process_unit_size: usize,
    file_stream: BufReader<File>,
    digit_length: i64,
    digit_start: i64,
    decoded_digits: i64,
    next_process_no: i64,
    next_start_digit: i64,
    current_process_unit: Option<YcdProcessUnit>,
    surplus_digit_str: String,
}

impl YcdSeqBlockStream {
    /// Open a YCD file and begin sequential reading from its first digit.
    ///
    /// `unit_size` is the number of digits delivered per [`next`](Self::next)
    /// call. It must be at least 19 (one compressed block's worth of digits);
    /// smaller values return `io::ErrorKind::InvalidInput`. A multiple of 19
    /// is recommended so that every unit is filled from whole blocks (see the
    /// crate-level documentation).
    ///
    /// # Errors
    ///
    /// Returns `InvalidInput` when `unit_size < 19`, `NotFound` when the file
    /// does not exist, and `InvalidData` when the header is malformed or the
    /// file is not base 10.
    pub fn new<P: AsRef<Path>>(file_name: P, unit_size: i32) -> io::Result<Self> {
        let process_unit_size = validate_unit_size(unit_size)?;
        let path = file_name.as_ref();
        let metadata = parse_metadata(path)?;
        let mut file_stream = BufReader::new(File::open(path)?);
        file_stream.seek(io::SeekFrom::Start(metadata.data_offset))?;

        Ok(Self {
            process_unit_size,
            file_stream,
            digit_length: metadata.digit_length,
            digit_start: metadata.digit_start,
            decoded_digits: 0,
            next_process_no: 1,
            next_start_digit: metadata.digit_start,
            current_process_unit: None,
            surplus_digit_str: String::new(),
        })
    }

    /// Open a YCD file and begin sequential reading from an arbitrary 1-based digit position.
    ///
    /// `start_position` is the 1-based absolute digit index at which reading should start.
    /// It must lie within the range covered by this file
    /// (`digit_start ..= digit_start + digit_length - 1`).
    ///
    /// The `unit_size` and iteration interface are identical to [`Self::new`].
    pub fn new_from<P: AsRef<Path>>(
        file_name: P,
        unit_size: i32,
        start_position: i64,
    ) -> io::Result<Self> {
        let process_unit_size = validate_unit_size(unit_size)?;
        let path = file_name.as_ref();
        let metadata = parse_metadata(path)?;

        let file_end = metadata
            .digit_start
            .checked_add(metadata.digit_length)
            .and_then(|e| e.checked_sub(1))
            .ok_or_else(|| invalid_data("Digit position overflow"))?;
        if start_position < metadata.digit_start || start_position > file_end {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "Start position {start_position} is outside the file's range \
                     [{}, {file_end}]",
                    metadata.digit_start
                ),
            ));
        }

        let local_start = usize::try_from(
            start_position
                .checked_sub(metadata.digit_start)
                .ok_or_else(|| invalid_data("local_start underflow"))?,
        )
        .map_err(|_| invalid_data("local_start overflows usize"))?;

        let block_index = local_start / DIGITS_PER_BLOCK;
        let offset_in_block = local_start % DIGITS_PER_BLOCK;

        let seek_pos = metadata
            .data_offset
            .checked_add(
                u64::try_from(block_index)
                    .ok()
                    .and_then(|bi| bi.checked_mul(8))
                    .ok_or_else(|| invalid_data("Seek offset overflow"))?,
            )
            .ok_or_else(|| invalid_data("Seek offset overflow"))?;

        let mut file_stream = BufReader::new(File::open(path)?);
        file_stream.seek(io::SeekFrom::Start(seek_pos))?;

        let mut decoded_digits = (block_index * DIGITS_PER_BLOCK) as i64;
        let mut surplus_digit_str = String::new();

        if offset_in_block > 0 {
            let mut buffer = [0_u8; 8];
            file_stream.read_exact(&mut buffer)?;
            let number = u64::from_le_bytes(buffer);
            let digits = format!("{number:019}");
            if digits.len() != DIGITS_PER_BLOCK {
                return Err(invalid_data(
                    "A compressed block contains more than 19 decimal digits",
                ));
            }

            let remaining = usize::try_from(metadata.digit_length - decoded_digits)
                .map_err(|_| invalid_data("Invalid remaining digit count"))?;
            let take = remaining.min(DIGITS_PER_BLOCK);
            decoded_digits += take as i64;

            surplus_digit_str.push_str(&digits[offset_in_block..take]);
        }

        Ok(Self {
            process_unit_size,
            file_stream,
            digit_length: metadata.digit_length,
            digit_start: metadata.digit_start,
            decoded_digits,
            next_process_no: 1,
            next_start_digit: start_position,
            current_process_unit: None,
            surplus_digit_str,
        })
    }

    /// Returns `true` while undelivered digits remain in this stream.
    pub fn has_next(&self) -> bool {
        self.decoded_digits < self.digit_length || !self.surplus_digit_str.is_empty()
    }

    /// Read and return the next unit of digits.
    ///
    /// Every unit except possibly the last is exactly `unit_size` digits long.
    /// The returned reference is valid until the following `next` call.
    ///
    /// # Errors
    ///
    /// Returns `io::ErrorKind::UnexpectedEof` with a "No more data to read"
    /// message when the stream is already exhausted ([`has_next`](Self::has_next)
    /// returns `false`), `UnexpectedEof` when the payload is shorter than the
    /// logical digit range, and `InvalidData` when a compressed block decodes
    /// to more than 19 digits.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> io::Result<&YcdProcessUnit> {
        if !self.has_next() {
            return Err(no_more_data_error());
        }

        let value = self.take_digits(self.process_unit_size)?;
        let process_no = self.next_process_no;
        let start_digit = self.next_start_digit;

        self.next_process_no = self
            .next_process_no
            .checked_add(1)
            .ok_or_else(|| invalid_data("Process number overflow"))?;
        self.next_start_digit = self
            .next_start_digit
            .checked_add(value.len() as i64)
            .ok_or_else(|| invalid_data("Digit position overflow"))?;
        self.current_process_unit = Some(YcdProcessUnit::new(process_no, start_digit, value));

        Ok(self
            .current_process_unit
            .as_ref()
            .expect("unit was assigned"))
    }

    fn take_digits(&mut self, maximum: usize) -> io::Result<String> {
        while self.surplus_digit_str.len() < maximum && self.decoded_digits < self.digit_length {
            let block = self.read_digit_block()?;
            self.surplus_digit_str.push_str(&block);
        }

        let take = maximum.min(self.surplus_digit_str.len());
        let remainder = self.surplus_digit_str.split_off(take);
        Ok(std::mem::replace(&mut self.surplus_digit_str, remainder))
    }

    fn read_digit_block(&mut self) -> io::Result<String> {
        let mut buffer = [0_u8; 8];
        self.file_stream.read_exact(&mut buffer)?;

        let number = u64::from_le_bytes(buffer);
        let digits = format!("{number:019}");
        if digits.len() != DIGITS_PER_BLOCK {
            return Err(invalid_data(
                "A compressed block contains more than 19 decimal digits",
            ));
        }

        let remaining = usize::try_from(self.digit_length - self.decoded_digits)
            .map_err(|_| invalid_data("Invalid remaining digit count"))?;
        let take = remaining.min(DIGITS_PER_BLOCK);
        self.decoded_digits += take as i64;

        Ok(digits[..take].to_string())
    }
}

/// Sequential reader over a contiguous sequence of YCD files, treated as one
/// logical digit stream.
///
/// All files are validated for header correctness and list continuity when
/// the stream is created, so iteration can safely cross file boundaries —
/// a unit that spans two files is stitched together transparently.
///
/// The iteration contract is the same as [`YcdSeqBlockStream`]:
/// [`has_next`](Self::has_next) returns `true` while undelivered digits
/// remain, [`next`](Self::next) yields units of exactly `unit_size` digits
/// (except possibly the last), and calling `next` after exhaustion returns
/// `io::ErrorKind::UnexpectedEof` with a "No more data to read" message.
///
/// Like `YcdSeqBlockStream`, this type deliberately does not implement
/// [`Iterator`]: `next` returns `io::Result<&YcdProcessUnit>` — a fallible,
/// borrowed result whose reference is only valid until the following `next`
/// call — which the `Iterator` contract cannot express.
#[derive(Debug)]
pub struct YcdMultiFileStream {
    process_unit_size: usize,
    streams: Vec<YcdSeqBlockStream>,
    current_stream: usize,
    next_process_no: i64,
    next_start_digit: i64,
    current_process_unit: Option<YcdProcessUnit>,
}

impl YcdMultiFileStream {
    /// Open an ordered, contiguous list of YCD files and begin sequential
    /// reading from the first digit of the first file.
    ///
    /// `unit_size` is the number of digits delivered per [`next`](Self::next)
    /// call. It must be at least 19 (one compressed block's worth of digits);
    /// smaller values return `io::ErrorKind::InvalidInput`. A multiple of 19
    /// is recommended so that every unit is filled from whole blocks (see the
    /// crate-level documentation).
    ///
    /// # Errors
    ///
    /// Returns `InvalidInput` when `unit_size < 19`, when `file_names` is
    /// empty, or when the files are not contiguous; `NotFound` when a file
    /// does not exist; and `InvalidData` when a header is malformed or a file
    /// is not base 10.
    pub fn new<P: AsRef<Path>>(file_names: &[P], unit_size: i32) -> io::Result<Self> {
        let process_unit_size = validate_unit_size(unit_size)?;
        if file_names.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "At least one YCD file is required",
            ));
        }

        let mut streams = Vec::with_capacity(file_names.len());
        for file_name in file_names {
            streams.push(YcdSeqBlockStream::new(file_name, unit_size)?);
        }

        for pair in streams.windows(2) {
            let expected_start = pair[0]
                .digit_start
                .checked_add(pair[0].digit_length)
                .ok_or_else(|| invalid_data("Digit position overflow"))?;
            if pair[1].digit_start != expected_start {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "YCD files are not contiguous: expected digit {expected_start}, found {}",
                        pair[1].digit_start
                    ),
                ));
            }
        }

        let next_start_digit = streams[0].digit_start;
        Ok(Self {
            process_unit_size,
            streams,
            current_stream: 0,
            next_process_no: 1,
            next_start_digit,
            current_process_unit: None,
        })
    }

    /// Open a contiguous list of YCD files and begin sequential reading from
    /// an arbitrary 1-based digit position.
    ///
    /// `start_position` is the 1-based absolute digit index at which reading
    /// should start.  It must lie within the combined range of all files in
    /// the list.  All files in `file_names` are validated for header
    /// correctness and list continuity before any payload I/O begins.
    ///
    /// Files that end before `start_position` are skipped entirely; only the
    /// file that contains `start_position` (and all subsequent files) are
    /// opened for streaming.
    ///
    /// The `unit_size` and iteration interface are identical to [`Self::new`].
    pub fn new_from<P: AsRef<Path>>(
        file_names: &[P],
        unit_size: i32,
        start_position: i64,
    ) -> io::Result<Self> {
        let process_unit_size = validate_unit_size(unit_size)?;
        if file_names.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "At least one YCD file is required",
            ));
        }
        if start_position < 1 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Start position must be >= 1",
            ));
        }

        let file_infos = collect_file_infos(file_names)?;

        let list_start = file_infos[0].file_start as i64;
        let last = file_infos
            .last()
            .expect("non-empty after collect_file_infos");
        let list_end = last
            .file_start
            .checked_add(last.file_length)
            .and_then(|e| e.checked_sub(1))
            .ok_or_else(|| invalid_data("Digit range end overflow"))? as i64;

        if start_position < list_start || start_position > list_end {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "Start position {start_position} is outside the available range \
                     [{list_start}, {list_end}]"
                ),
            ));
        }

        let start_file_idx = file_infos
            .partition_point(|fi| fi.file_start + fi.file_length - 1 < start_position as usize);

        let mut streams = Vec::with_capacity(file_names.len() - start_file_idx);
        for (i, file_name) in file_names[start_file_idx..].iter().enumerate() {
            if i == 0 {
                streams.push(YcdSeqBlockStream::new_from(
                    file_name,
                    unit_size,
                    start_position,
                )?);
            } else {
                streams.push(YcdSeqBlockStream::new(file_name, unit_size)?);
            }
        }

        Ok(Self {
            process_unit_size,
            streams,
            current_stream: 0,
            next_process_no: 1,
            next_start_digit: start_position,
            current_process_unit: None,
        })
    }

    /// Returns `true` while undelivered digits remain in any of the files.
    pub fn has_next(&self) -> bool {
        self.streams[self.current_stream..]
            .iter()
            .any(YcdSeqBlockStream::has_next)
    }

    /// Read and return the next unit of digits, crossing file boundaries
    /// transparently when a unit spans two files.
    ///
    /// Every unit except possibly the last is exactly `unit_size` digits long.
    /// The returned reference is valid until the following `next` call.
    ///
    /// # Errors
    ///
    /// Returns `io::ErrorKind::UnexpectedEof` with a "No more data to read"
    /// message when the stream is already exhausted ([`has_next`](Self::has_next)
    /// returns `false`), `UnexpectedEof` when a payload is shorter than its
    /// logical digit range, and `InvalidData` when a compressed block decodes
    /// to more than 19 digits.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> io::Result<&YcdProcessUnit> {
        if !self.has_next() {
            return Err(no_more_data_error());
        }

        let mut value = String::with_capacity(self.process_unit_size);
        while value.len() < self.process_unit_size && self.current_stream < self.streams.len() {
            let remaining = self.process_unit_size - value.len();
            let stream = &mut self.streams[self.current_stream];
            value.push_str(&stream.take_digits(remaining)?);

            if !stream.has_next() {
                self.current_stream += 1;
            }
        }

        let process_no = self.next_process_no;
        let start_digit = self.next_start_digit;
        self.next_process_no = self
            .next_process_no
            .checked_add(1)
            .ok_or_else(|| invalid_data("Process number overflow"))?;
        self.next_start_digit = self
            .next_start_digit
            .checked_add(value.len() as i64)
            .ok_or_else(|| invalid_data("Digit position overflow"))?;
        self.current_process_unit = Some(YcdProcessUnit::new(process_no, start_digit, value));

        Ok(self
            .current_process_unit
            .as_ref()
            .expect("unit was assigned"))
    }
}

/// One entry in a [`YcdIndex`], representing a single YCD file.
///
/// The snapshot fields (`file_size`, `modified`) are recorded at index-build
/// time and compared against the filesystem when a file is actually read.
/// Any mismatch causes `read_digits` to return an explicit "stale index" error
/// rather than silently reading potentially incorrect data.
#[derive(Debug, Clone)]
pub struct YcdIndexEntry {
    /// Absolute path to the YCD file.
    pub path: PathBuf,
    /// Byte offset at which compressed 8-byte blocks start.
    pub data_offset: u64,
    /// 1-based absolute digit position of this file's first digit.
    pub file_start: usize,
    /// Total number of digits stored in this file.
    pub file_length: usize,
    /// File size in bytes recorded at index-build time.
    pub file_size: u64,
    /// Last-modified time recorded at index-build time.
    ///
    /// On platforms where `std::fs::Metadata::modified()` is unavailable,
    /// this field is set to `SystemTime::UNIX_EPOCH` at build time and
    /// `read_digits` will also read `UNIX_EPOCH` for the current mtime,
    /// so the comparison will always succeed.  On those platforms stale-index
    /// detection relies solely on `file_size`.
    pub modified: SystemTime,
}

/// An in-memory index over a contiguous set of YCD files.
///
/// Building the index reads every file's header once and stores the
/// resulting metadata.  Subsequent `read_digits` calls use binary search
/// to locate the relevant file(s) and seek directly to the target block,
/// skipping every other file entirely.
///
/// # Stale-index detection
///
/// Each time a file is actually read, its current `file_size` and
/// last-modified time are compared against the snapshot taken at build
/// time.  If any difference is detected, `read_digits` returns
/// `io::ErrorKind::InvalidData` with an explicit "stale index" message.
/// There is no silent fallback to a full-scan path.
///
/// # Example
///
/// ```rust,no_run
/// use std::io;
/// use ycd_reader::YcdIndex;
///
/// fn main() -> io::Result<()> {
///     let files = [
///         "Pi - Dec - Chudnovsky - 0.ycd",
///         "Pi - Dec - Chudnovsky - 1.ycd",
///     ];
///
///     // Build the index once (reads all headers).
///     let index = YcdIndex::build(&files)?;
///
///     // Fast random-access — only the relevant file(s) are opened.
///     let digits = index.read_digits(999_995, 20)?;
///     assert_eq!(digits, "45815130927562832084");
///
///     Ok(())
/// }
/// ```
#[derive(Debug, Clone)]
pub struct YcdIndex {
    entries: Vec<YcdIndexEntry>,
}

impl YcdIndex {
    /// Build an index from an ordered, contiguous slice of YCD file paths.
    ///
    /// Every file's header is read and validated (base-10 constraint,
    /// contiguity, no duplicates, no gaps).  On success the index is ready
    /// for `read_digits` calls.
    ///
    /// # Errors
    ///
    /// Returns the same errors as [`YcdFileUtil::read_digits`] for header and
    /// continuity problems.  Additionally returns `InvalidInput` when `files`
    /// is empty.
    pub fn build<P: AsRef<Path>>(files: &[P]) -> io::Result<Self> {
        if files.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "File list must not be empty",
            ));
        }

        let raw_infos = collect_file_infos(files)?;
        let mut entries = Vec::with_capacity(files.len());

        for (path, fi) in files.iter().zip(raw_infos.iter()) {
            let fs_meta = std::fs::metadata(path.as_ref())?;
            let file_size = fs_meta.len();
            let modified = fs_meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);

            entries.push(YcdIndexEntry {
                path: std::fs::canonicalize(path.as_ref())?,
                data_offset: fi.data_offset,
                file_start: fi.file_start,
                file_length: fi.file_length,
                file_size,
                modified,
            });
        }

        Ok(Self { entries })
    }

    /// Discard the current index and rebuild it from a new file list.
    ///
    /// On success `self` is replaced with the freshly built index.  If the
    /// rebuild fails, `self` is left unchanged.
    pub fn rebuild<P: AsRef<Path>>(&mut self, files: &[P]) -> io::Result<()> {
        *self = Self::build(files)?;
        Ok(())
    }

    /// Returns the number of YCD files in the index.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns `true` if the index contains no files.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Returns a slice of all index entries in digit order.
    pub fn entries(&self) -> &[YcdIndexEntry] {
        &self.entries
    }

    /// Read exactly `length` decimal digits starting at 1-based position
    /// `one_based_start_position`.
    ///
    /// Binary search locates the first file that contains the start position.
    /// Only the file(s) actually needed are opened; all other files are
    /// skipped entirely.
    ///
    /// Before reading each file, its current `file_size` and last-modified
    /// time are compared against the index snapshot.  A mismatch returns
    /// `io::ErrorKind::InvalidData` with an "index is stale" message.
    ///
    /// # Errors
    ///
    /// | Condition | `io::ErrorKind` |
    /// |---|---|
    /// | Index is empty, position 0, or length 0 | `InvalidInput` |
    /// | Start or end position outside the indexed range | `InvalidInput` |
    /// | `start + length` overflows `usize` | `InvalidData` |
    /// | File size or mtime differs from index snapshot | `InvalidData` |
    /// | File does not exist | `NotFound` |
    /// | Payload truncated within logical range | `UnexpectedEof` |
    /// | Output string pre-allocation failure | `Other` |
    pub fn read_digits(
        &self,
        one_based_start_position: usize,
        length: usize,
    ) -> io::Result<String> {
        if self.entries.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Index is empty",
            ));
        }
        if one_based_start_position == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Start position must be >= 1",
            ));
        }
        if length == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Length must be >= 1",
            ));
        }

        let list_start = self.entries[0].file_start;
        let last = self.entries.last().expect("non-empty");
        let list_end = last
            .file_start
            .checked_add(last.file_length)
            .and_then(|e| e.checked_sub(1))
            .ok_or_else(|| invalid_data("Digit range end overflow"))?;

        if one_based_start_position < list_start || one_based_start_position > list_end {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "Start position {one_based_start_position} is outside the available range \
                     [{list_start}, {list_end}]"
                ),
            ));
        }

        let end_position = one_based_start_position
            .checked_add(length)
            .ok_or_else(|| invalid_data("End position overflow (start + length)"))?
            .checked_sub(1)
            .expect("length >= 1");

        if end_position > list_end {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "Requested range ends at {end_position} which exceeds the available \
                     range end {list_end}"
                ),
            ));
        }

        let mut result = String::new();
        result.try_reserve_exact(length).map_err(io::Error::other)?;

        let start_idx = self
            .entries
            .partition_point(|e| e.file_start + e.file_length - 1 < one_based_start_position);

        let mut remaining = length;

        for entry in &self.entries[start_idx..] {
            if remaining == 0 {
                break;
            }

            let fs_meta = std::fs::metadata(&entry.path)?;
            if fs_meta.len() != entry.file_size {
                return Err(invalid_data(format!(
                    "Index is stale: file size of '{}' changed \
                     (expected {} bytes, found {} bytes)",
                    entry.path.display(),
                    entry.file_size,
                    fs_meta.len(),
                )));
            }
            let current_modified = fs_meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            if current_modified != entry.modified {
                return Err(invalid_data(format!(
                    "Index is stale: last-modified time of '{}' changed",
                    entry.path.display(),
                )));
            }

            let digits_read = length - remaining;
            let current_abs = one_based_start_position
                .checked_add(digits_read)
                .expect("validated end_position <= list_end");

            let local_start = current_abs
                .checked_sub(entry.file_start)
                .ok_or_else(|| invalid_data("local_start underflow"))?;
            let available = entry
                .file_length
                .checked_sub(local_start)
                .ok_or_else(|| invalid_data("local_start exceeds file length"))?;
            let to_take = available.min(remaining);

            let (block_index, offset_in_block, blocks_to_read) =
                compute_seek_params(local_start, to_take);

            let seek_pos = entry
                .data_offset
                .checked_add(
                    u64::try_from(block_index)
                        .ok()
                        .and_then(|bi| bi.checked_mul(8))
                        .ok_or_else(|| invalid_data("Seek offset overflow"))?,
                )
                .ok_or_else(|| invalid_data("Seek offset overflow"))?;

            let mut reader = BufReader::new(File::open(&entry.path)?);
            reader.seek(io::SeekFrom::Start(seek_pos))?;

            let mut skip = offset_in_block;
            let mut taken = 0usize;

            for _ in 0..blocks_to_read {
                if taken >= to_take {
                    break;
                }

                let mut buf = [0_u8; 8];
                reader.read_exact(&mut buf).map_err(|e| match e.kind() {
                    io::ErrorKind::UnexpectedEof => io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "YCD payload is shorter than the logical digit range",
                    ),
                    _ => e,
                })?;

                let number = u64::from_le_bytes(buf);
                let block_str = format!("{number:019}");
                if block_str.len() != DIGITS_PER_BLOCK {
                    return Err(invalid_data(
                        "A compressed block contains more than 19 decimal digits",
                    ));
                }

                let usable = &block_str[skip..];
                skip = 0;

                let can_take = usable.len().min(to_take - taken);
                result.push_str(&usable[..can_take]);
                taken += can_take;
            }

            remaining -= to_take;
        }

        debug_assert_eq!(result.len(), length);
        Ok(result)
    }
}

/// Per-file metadata used by [`YcdFileUtil::read_digits`].
struct FileInfo {
    /// Byte offset at which compressed 8-byte blocks start.
    data_offset: u64,
    /// 1-based absolute digit position of this file's first digit.
    file_start: usize,
    /// Total number of digits stored in this file.
    file_length: usize,
}

/// Compute the compressed-block access parameters for a direct seek into a YCD payload.
///
/// Given a 0-based `local_start` offset within a file and the number of digits
/// to read (`length`), returns `(block_index, offset_in_block, blocks_to_read)` where:
///
/// - `block_index`: 0-based index of the first 8-byte block to read.
/// - `offset_in_block`: digits to skip at the start of the first decoded block.
/// - `blocks_to_read`: the minimum number of blocks that cover the requested range.
pub(crate) fn compute_seek_params(local_start: usize, length: usize) -> (usize, usize, usize) {
    let block_index = local_start / DIGITS_PER_BLOCK;
    let offset_in_block = local_start % DIGITS_PER_BLOCK;
    let blocks_to_read = (offset_in_block + length).div_ceil(DIGITS_PER_BLOCK);
    (block_index, offset_in_block, blocks_to_read)
}

/// Stateless utility functions for one-off reads and header inspection.
///
/// Unlike [`YcdIndex`], these functions keep no state between calls: every
/// call re-reads and re-validates the headers of the files it is given. For
/// repeated random-access reads over the same file set, build a [`YcdIndex`]
/// once instead.
pub struct YcdFileUtil;

impl YcdFileUtil {
    /// Return the size of the file's header section in bytes.
    ///
    /// This is the byte offset at which the compressed digit payload (the
    /// first 8-byte block) begins: it covers the `Name: value` header lines,
    /// the `EndHeader` line, and the NUL data marker that follows it.
    ///
    /// # Errors
    ///
    /// Returns `NotFound` when the file does not exist, and `InvalidData`
    /// when the header is malformed, the file is not base 10, or the header
    /// size does not fit in `i32`.
    pub fn get_header_size<P: AsRef<Path>>(file_name: P) -> io::Result<i32> {
        i32::try_from(parse_metadata(file_name.as_ref())?.data_offset)
            .map_err(|_| invalid_data("YCD header is too large"))
    }

    /// Parse the file's header and return its fields as a map.
    ///
    /// The map is keyed by [`YcdHeaderInfoElem`]; values are the raw
    /// (trimmed) strings from the `Name: value` header lines. Only the
    /// fields this library recognizes are included, and `EndHeader` never
    /// appears as a key.
    ///
    /// # Errors
    ///
    /// Returns `NotFound` when the file does not exist, and `InvalidData`
    /// when the header is malformed or the file is not base 10.
    pub fn get_ycd_header<P: AsRef<Path>>(
        file_name: P,
    ) -> io::Result<HashMap<YcdHeaderInfoElem, String>> {
        Ok(parse_metadata(file_name.as_ref())?.header)
    }

    /// Read exactly `length` decimal digits starting at 1-based position
    /// `one_based_start_position` from the concatenation of the given YCD files.
    ///
    /// # Arguments
    ///
    /// * `files` — An ordered, contiguous slice of YCD file paths (BlockID order).
    ///   The first file need not have BlockID 0; absolute positions are derived
    ///   from each file's header.
    /// * `one_based_start_position` — 1-based index of the first digit to return.
    ///   Position 1 is the first stored digit of the constant's fractional part
    ///   (for π, the digit immediately after "3."). The integer part, sign, and
    ///   decimal point are never included.
    /// * `length` — Number of digits to return. The result string is exactly
    ///   `length` bytes of ASCII digits.
    ///
    /// # Returns
    ///
    /// A [`String`] containing exactly `length` ASCII decimal digits on success.
    ///
    /// # Errors
    ///
    /// | Condition | `io::ErrorKind` |
    /// |---|---|
    /// | Empty file list, position 0, or length 0 | `InvalidInput` |
    /// | Start position out of range, or end exceeds range | `InvalidInput` |
    /// | Gap, duplicate, or reversed files in list | `InvalidInput` |
    /// | Invalid header, non-base-10, or corrupt compressed value | `InvalidData` |
    /// | Position, offset, or end calculation overflow | `InvalidData` |
    /// | File does not exist | `NotFound` |
    /// | Payload truncated within logical range | `UnexpectedEof` |
    /// | Output string pre-allocation failure | `Other` |
    ///
    /// # Notes
    ///
    /// * Files with `TotalDigits == 0` are treated as having exactly `Blocksize`
    ///   digits. A "shortened" final file (where the actual payload is smaller than
    ///   `Blocksize`) cannot be detected via the header alone; payload truncation
    ///   within the logical range is reported as `UnexpectedEof`.
    /// * The entire file list is validated before any I/O on the payload begins.
    /// * Only the compressed blocks that cover the requested range are decoded;
    ///   no byte before the target block is read.
    /// * A large `length` requires the same amount of heap memory for the result.
    pub fn read_digits<P: AsRef<Path>>(
        files: &[P],
        one_based_start_position: usize,
        length: usize,
    ) -> io::Result<String> {
        // --- Basic argument validation ---
        if files.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "File list must not be empty",
            ));
        }
        if one_based_start_position == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Start position must be >= 1",
            ));
        }
        if length == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Length must be >= 1",
            ));
        }

        // --- Parse and validate all file metadata up front ---
        let file_infos = collect_file_infos(files)?;

        // --- Compute available range ---
        let list_start = file_infos[0].file_start;
        let last = file_infos
            .last()
            .expect("non-empty after collect_file_infos");
        let list_end = last
            .file_start
            .checked_add(last.file_length)
            .and_then(|e| e.checked_sub(1))
            .ok_or_else(|| invalid_data("Digit range end overflow"))?;

        if one_based_start_position < list_start || one_based_start_position > list_end {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "Start position {one_based_start_position} is outside the available range \
                     [{list_start}, {list_end}]"
                ),
            ));
        }

        // end_position is the 1-based index of the last digit we want (inclusive).
        let end_position = one_based_start_position
            .checked_add(length)
            .ok_or_else(|| invalid_data("End position overflow (start + length)"))?
            .checked_sub(1)
            .expect("length >= 1 so this cannot underflow");

        if end_position > list_end {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "Requested range ends at {end_position} which exceeds the available \
                     range end {list_end}"
                ),
            ));
        }

        // --- Pre-allocate output ---
        let mut result = String::new();
        result.try_reserve_exact(length).map_err(io::Error::other)?;

        // --- Find the first file that contains one_based_start_position ---
        // partition_point returns the index of the first element for which the predicate is false.
        // We want the first file whose last digit >= one_based_start_position.
        let start_file_idx = file_infos.partition_point(|fi| {
            // file's last digit = fi.file_start + fi.file_length - 1
            fi.file_start + fi.file_length - 1 < one_based_start_position
        });

        // --- Read from each needed file ---
        let mut remaining = length;

        for (idx, fi) in file_infos[start_file_idx..].iter().enumerate() {
            if remaining == 0 {
                break;
            }

            // Absolute position of the digit we want to start reading from in this file.
            let digits_read = length - remaining;
            let current_abs = one_based_start_position
                .checked_add(digits_read)
                .expect("already validated end_position <= list_end so no overflow here");

            // 0-based offset within this file.
            let local_start = current_abs
                .checked_sub(fi.file_start)
                .ok_or_else(|| invalid_data("local_start underflow"))?;

            // For subsequent files (idx > 0) local_start must be 0.
            // For the first file, local_start may be anywhere inside the file.
            let available = fi
                .file_length
                .checked_sub(local_start)
                .ok_or_else(|| invalid_data("local_start exceeds file length"))?;
            let to_take = available.min(remaining);

            // Compute block-level seek parameters.
            let (block_index, offset_in_block, blocks_to_read) =
                compute_seek_params(local_start, to_take);

            // Seek offset in bytes from the start of the file.
            let seek_pos = fi
                .data_offset
                .checked_add(
                    u64::try_from(block_index)
                        .ok()
                        .and_then(|bi| bi.checked_mul(8))
                        .ok_or_else(|| invalid_data("Seek offset overflow"))?,
                )
                .ok_or_else(|| invalid_data("Seek offset overflow"))?;

            // Open the file corresponding to this FileInfo.
            // file_infos[start_file_idx + idx] corresponds to files[start_file_idx + idx].
            let path = files[start_file_idx + idx].as_ref();
            let mut reader = BufReader::new(File::open(path)?);
            reader.seek(io::SeekFrom::Start(seek_pos))?;

            // Decode compressed blocks, skipping the unwanted leading digits in the first block.
            let mut skip = offset_in_block;
            let mut taken = 0usize;

            for _ in 0..blocks_to_read {
                if taken >= to_take {
                    break;
                }

                let mut buf = [0_u8; 8];
                reader.read_exact(&mut buf).map_err(|e| match e.kind() {
                    io::ErrorKind::UnexpectedEof => io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "YCD payload is shorter than the logical digit range",
                    ),
                    _ => e,
                })?;

                let number = u64::from_le_bytes(buf);
                let block_str = format!("{number:019}");
                if block_str.len() != DIGITS_PER_BLOCK {
                    return Err(invalid_data(
                        "A compressed block contains more than 19 decimal digits",
                    ));
                }

                // Skip unwanted leading digits in the first block.
                let usable = &block_str[skip..];
                skip = 0;

                let can_take = usable.len().min(to_take - taken);
                result.push_str(&usable[..can_take]);
                taken += can_take;
            }

            remaining -= to_take;
        }

        debug_assert_eq!(result.len(), length);
        Ok(result)
    }
}

fn validate_unit_size(unit_size: i32) -> io::Result<usize> {
    if unit_size < DIGITS_PER_BLOCK as i32 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Unit size must be at least 19",
        ));
    }
    Ok(unit_size as usize)
}

fn parse_metadata(path: &Path) -> io::Result<YcdMetadata> {
    let mut reader = BufReader::new(File::open(path)?);
    let mut header = HashMap::new();
    let mut line = Vec::new();

    loop {
        line.clear();
        if reader.read_until(b'\n', &mut line)? == 0 {
            return Err(invalid_data("Missing EndHeader marker"));
        }

        let text = std::str::from_utf8(&line)
            .map_err(|_| invalid_data("YCD header is not valid UTF-8"))?;
        let text = text.trim_end_matches(&['\r', '\n'][..]);
        if text == YcdHeaderInfoElem::EndHeader.as_ref() {
            break;
        }
        if text.is_empty() || text.starts_with('#') {
            continue;
        }

        let Some((name, value)) = text.split_once(':') else {
            continue;
        };
        if let Some(key) = header_key(name.trim()) {
            header.insert(key, value.trim().to_string());
        }
    }

    consume_data_marker(&mut reader)?;
    let data_offset = reader.stream_position()?;
    let block_size = parse_positive_i64(&header, YcdHeaderInfoElem::Blocksize)?;
    let block_id = parse_nonnegative_i64(&header, YcdHeaderInfoElem::BlockID)?;

    let version = required_value(&header, YcdHeaderInfoElem::FileVersion)?;
    if version.is_empty() {
        return Err(invalid_data("FileVersion must not be empty"));
    }
    if required_value(&header, YcdHeaderInfoElem::FirstDigits)?.is_empty() {
        return Err(invalid_data("FirstDigits must not be empty"));
    }

    let base = parse_nonnegative_i64(&header, YcdHeaderInfoElem::Base)?;
    if base != 10 {
        return Err(invalid_data("Only base 10 YCD files are supported"));
    }
    let total_digits = parse_nonnegative_i64(&header, YcdHeaderInfoElem::TotalDigits)?;
    if header.contains_key(&YcdHeaderInfoElem::TotalBlocks) {
        parse_nonnegative_i64(&header, YcdHeaderInfoElem::TotalBlocks)?;
    }

    let digit_offset = block_size
        .checked_mul(block_id)
        .ok_or_else(|| invalid_data("Digit position overflow"))?;
    let digit_length = if total_digits == 0 {
        block_size
    } else {
        let remaining = total_digits
            .checked_sub(digit_offset)
            .ok_or_else(|| invalid_data("BlockID starts beyond TotalDigits"))?;
        if remaining == 0 {
            return Err(invalid_data("BlockID starts beyond TotalDigits"));
        }
        remaining.min(block_size)
    };
    let digit_start = digit_offset
        .checked_add(1)
        .ok_or_else(|| invalid_data("Digit position overflow"))?;

    Ok(YcdMetadata {
        header,
        data_offset,
        digit_length,
        digit_start,
    })
}

fn consume_data_marker(reader: &mut BufReader<File>) -> io::Result<()> {
    let first = read_marker_byte(reader)?;
    match first {
        0 => Ok(()),
        b'\n' => require_nul(reader),
        b'\r' => {
            if read_marker_byte(reader)? != b'\n' {
                return Err(invalid_data("Invalid line ending after EndHeader"));
            }
            require_nul(reader)
        }
        _ => Err(invalid_data("Missing NUL data marker after EndHeader")),
    }
}

fn require_nul(reader: &mut BufReader<File>) -> io::Result<()> {
    if read_marker_byte(reader)? == 0 {
        Ok(())
    } else {
        Err(invalid_data("Missing NUL data marker after EndHeader"))
    }
}

fn read_marker_byte(reader: &mut BufReader<File>) -> io::Result<u8> {
    let mut byte = [0_u8; 1];
    reader
        .read_exact(&mut byte)
        .map_err(|error| match error.kind() {
            io::ErrorKind::UnexpectedEof => invalid_data("Incomplete YCD header"),
            _ => error,
        })?;
    Ok(byte[0])
}

fn header_key(name: &str) -> Option<YcdHeaderInfoElem> {
    match name {
        "FileVersion" => Some(YcdHeaderInfoElem::FileVersion),
        "Base" => Some(YcdHeaderInfoElem::Base),
        "FirstDigits" => Some(YcdHeaderInfoElem::FirstDigits),
        "TotalDigits" => Some(YcdHeaderInfoElem::TotalDigits),
        "TotalBlocks" => Some(YcdHeaderInfoElem::TotalBlocks),
        "Blocksize" => Some(YcdHeaderInfoElem::Blocksize),
        "BlockID" => Some(YcdHeaderInfoElem::BlockID),
        _ => None,
    }
}

fn required_value(
    header: &HashMap<YcdHeaderInfoElem, String>,
    key: YcdHeaderInfoElem,
) -> io::Result<&str> {
    header
        .get(&key)
        .map(String::as_str)
        .ok_or_else(|| invalid_data(format!("Missing required header field: {}", key.as_ref())))
}

fn parse_positive_i64(
    header: &HashMap<YcdHeaderInfoElem, String>,
    key: YcdHeaderInfoElem,
) -> io::Result<i64> {
    let value = parse_nonnegative_i64(header, key)?;
    if value == 0 {
        return Err(invalid_data(format!("{} must be positive", key.as_ref())));
    }
    Ok(value)
}

fn parse_nonnegative_i64(
    header: &HashMap<YcdHeaderInfoElem, String>,
    key: YcdHeaderInfoElem,
) -> io::Result<i64> {
    required_value(header, key)?
        .parse::<i64>()
        .map_err(|_| invalid_data(format!("{} must be a nonnegative integer", key.as_ref())))
        .and_then(|value| {
            if value < 0 {
                Err(invalid_data(format!(
                    "{} must be a nonnegative integer",
                    key.as_ref()
                )))
            } else {
                Ok(value)
            }
        })
}

/// Parse metadata for every file in `files`, validate header correctness and
/// list continuity, and return a `Vec<FileInfo>` in the same order.
///
/// All files are validated regardless of whether the requested range touches them.
fn collect_file_infos<P: AsRef<Path>>(files: &[P]) -> io::Result<Vec<FileInfo>> {
    let mut infos: Vec<FileInfo> = Vec::with_capacity(files.len());

    for path in files {
        let meta = parse_metadata(path.as_ref())?;

        let file_start = usize::try_from(meta.digit_start)
            .map_err(|_| invalid_data("Digit start position overflows usize"))?;
        let file_length = usize::try_from(meta.digit_length)
            .map_err(|_| invalid_data("Digit length overflows usize"))?;

        if let Some(prev) = infos.last() {
            let expected = prev
                .file_start
                .checked_add(prev.file_length)
                .ok_or_else(|| invalid_data("Digit position overflow in continuity check"))?;
            if file_start != expected {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "YCD files are not contiguous: expected start {expected}, found {file_start}"
                    ),
                ));
            }
        }

        infos.push(FileInfo {
            data_offset: meta.data_offset,
            file_start,
            file_length,
        });
    }

    Ok(infos)
}

fn invalid_data(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

fn no_more_data_error() -> io::Error {
    io::Error::new(io::ErrorKind::UnexpectedEof, "No more data to read")
}

#[cfg(test)]
mod tests {
    use super::compute_seek_params;

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
}
