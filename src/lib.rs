//! Digit positions, digit counts, processing-unit sizes, and process numbers use
//! `usize` throughout the public API. Absolute digit positions and process
//! numbers are 1-based. Values must fit in `usize` on the target platform;
//! byte offsets use `u64` for file seeking.

use std::collections::HashMap;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Seek};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use strum_macros::{AsRefStr, EnumString};

const DIGITS_PER_BLOCK: usize = 19;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, AsRefStr, EnumString)]
pub enum YcdHeaderInfoElem {
    FileVersion,
    Base,
    FirstDigits,
    TotalDigits,
    TotalBlocks,
    Blocksize,
    BlockID,
    EndHeader,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct YcdProcessUnit {
    pub process_no: usize,
    pub start_digit: usize,
    pub value: String,
}

impl YcdProcessUnit {
    pub fn new(process_no: usize, start_digit: usize, value: String) -> Self {
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
    digit_length: usize,
    digit_start: usize,
}

#[derive(Debug)]
pub struct YcdSeqBlockStream {
    process_unit_size: usize,
    file_stream: BufReader<File>,
    digit_length: usize,
    decoded_digits: usize,
    next_process_no: usize,
    next_start_digit: usize,
    current_process_unit: Option<YcdProcessUnit>,
    surplus_digit_str: String,
    surplus_digit_offset: usize,
}

impl YcdSeqBlockStream {
    pub fn new<P: AsRef<Path>>(file_name: P, unit_size: usize) -> io::Result<Self> {
        let process_unit_size = validate_unit_size(unit_size)?;
        let path = file_name.as_ref();
        let metadata = parse_metadata(path)?;
        let mut file_stream = BufReader::new(File::open(path)?);
        file_stream.seek(io::SeekFrom::Start(metadata.data_offset))?;

        Ok(Self {
            process_unit_size,
            file_stream,
            digit_length: metadata.digit_length,
            decoded_digits: 0,
            next_process_no: 1,
            next_start_digit: metadata.digit_start,
            current_process_unit: None,
            surplus_digit_str: String::new(),
            surplus_digit_offset: 0,
        })
    }

    /// Open a YCD file and begin sequential reading from an arbitrary 1-based digit position.
    ///
    /// `start_position` is the 1-based absolute digit index at which reading should start.
    /// It must lie within the range covered by this file
    /// (`digit_start .. digit_start + digit_length - 1`, inclusive).
    ///
    /// The `unit_size` and iteration interface are identical to [`Self::new`].
    pub fn new_from<P: AsRef<Path>>(
        file_name: P,
        unit_size: usize,
        start_position: usize,
    ) -> io::Result<Self> {
        let process_unit_size = validate_unit_size(unit_size)?;
        let path = file_name.as_ref();
        let metadata = parse_metadata(path)?;

        let file_end = inclusive_range_end(metadata.digit_start, metadata.digit_length)?;
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

        let local_start = start_position
            .checked_sub(metadata.digit_start)
            .ok_or_else(|| invalid_data("local_start underflow"))?;

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

        let mut decoded_digits = block_index * DIGITS_PER_BLOCK;
        let mut surplus_digit_str = String::new();

        if offset_in_block > 0 {
            let remaining = metadata.digit_length - decoded_digits;
            let take = remaining.min(DIGITS_PER_BLOCK);
            let rest_len = take - offset_in_block;
            surplus_digit_str
                .try_reserve_exact(rest_len)
                .map_err(io::Error::other)?;

            let mut buffer = [0_u8; 8];
            file_stream.read_exact(&mut buffer)?;
            let number = u64::from_le_bytes(buffer);
            let digits = decode_digit_block(number)?;

            decoded_digits += take;

            surplus_digit_str.push_str(
                std::str::from_utf8(&digits[offset_in_block..take])
                    .expect("decoded digits are ASCII"),
            );
        }

        Ok(Self {
            process_unit_size,
            file_stream,
            digit_length: metadata.digit_length,
            decoded_digits,
            next_process_no: 1,
            next_start_digit: start_position,
            current_process_unit: None,
            surplus_digit_str,
            surplus_digit_offset: 0,
        })
    }

    pub fn has_next(&self) -> bool {
        self.decoded_digits < self.digit_length
            || self.surplus_digit_offset < self.surplus_digit_str.len()
    }

    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> io::Result<&YcdProcessUnit> {
        if !self.has_next() {
            return Err(no_more_data_error());
        }

        let value = self.take_digits(self.process_unit_size)?;
        let process_no = self.next_process_no;
        let start_digit = self.next_start_digit;

        if self.has_next() {
            self.next_process_no = self
                .next_process_no
                .checked_add(1)
                .ok_or_else(|| invalid_data("Process number overflow"))?;
            self.next_start_digit = self
                .next_start_digit
                .checked_add(value.len())
                .ok_or_else(|| invalid_data("Digit position overflow"))?;
        }
        self.current_process_unit = Some(YcdProcessUnit::new(process_no, start_digit, value));

        Ok(self
            .current_process_unit
            .as_ref()
            .expect("unit was assigned"))
    }

    fn take_digits(&mut self, maximum: usize) -> io::Result<String> {
        while self.surplus_digit_str.len() - self.surplus_digit_offset < maximum
            && self.decoded_digits < self.digit_length
        {
            if self.surplus_digit_offset > 0 {
                self.surplus_digit_str.drain(..self.surplus_digit_offset);
                self.surplus_digit_offset = 0;
            }
            self.read_digit_block()?;
        }

        let available = self.surplus_digit_str.len() - self.surplus_digit_offset;
        let take = maximum.min(available);
        let end = self.surplus_digit_offset + take;
        let mut digits = String::new();
        digits.try_reserve_exact(take).map_err(io::Error::other)?;
        digits.push_str(&self.surplus_digit_str[self.surplus_digit_offset..end]);
        self.surplus_digit_offset = end;
        if self.surplus_digit_offset == self.surplus_digit_str.len() {
            self.surplus_digit_str.clear();
            self.surplus_digit_offset = 0;
        }
        Ok(digits)
    }

    fn read_digit_block(&mut self) -> io::Result<()> {
        let remaining = self.digit_length - self.decoded_digits;
        let take = remaining.min(DIGITS_PER_BLOCK);
        self.surplus_digit_str
            .try_reserve(take)
            .map_err(io::Error::other)?;

        let mut buffer = [0_u8; 8];
        self.file_stream.read_exact(&mut buffer)?;

        let number = u64::from_le_bytes(buffer);
        let digits = decode_digit_block(number)?;

        self.surplus_digit_str
            .push_str(std::str::from_utf8(&digits[..take]).expect("decoded digits are ASCII"));
        self.decoded_digits += take;

        Ok(())
    }
}

#[derive(Debug)]
pub struct YcdMultiFileStream {
    process_unit_size: usize,
    file_names: Vec<PathBuf>,
    current_stream: Option<YcdSeqBlockStream>,
    current_file: usize,
    next_process_no: usize,
    next_start_digit: usize,
    current_process_unit: Option<YcdProcessUnit>,
}

impl YcdMultiFileStream {
    pub fn new<P: AsRef<Path>>(file_names: &[P], unit_size: usize) -> io::Result<Self> {
        let process_unit_size = validate_unit_size(unit_size)?;
        if file_names.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "At least one YCD file is required",
            ));
        }

        let file_infos = collect_file_infos(file_names)?;
        let file_names: Vec<PathBuf> = file_names
            .iter()
            .map(|file_name| file_name.as_ref().to_path_buf())
            .collect();
        let current_stream = Some(YcdSeqBlockStream::new(&file_names[0], unit_size)?);
        let next_start_digit = file_infos[0].file_start;
        Ok(Self {
            process_unit_size,
            file_names,
            current_stream,
            current_file: 0,
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
    /// correctness and list continuity before any payload I/O begins. Only
    /// the file containing `start_position` is opened initially; later files
    /// are opened as they are reached.
    ///
    /// Files that end before `start_position` are skipped entirely; only the
    /// file that contains `start_position` (and all subsequent files) are
    /// opened for streaming.
    ///
    /// The `unit_size` and iteration interface are identical to [`Self::new`].
    pub fn new_from<P: AsRef<Path>>(
        file_names: &[P],
        unit_size: usize,
        start_position: usize,
    ) -> io::Result<Self> {
        let process_unit_size = validate_unit_size(unit_size)?;
        if file_names.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "At least one YCD file is required",
            ));
        }
        if start_position == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Start position must be >= 1",
            ));
        }

        let file_infos = collect_file_infos(file_names)?;

        let list_start = file_infos[0].file_start;
        let last = file_infos
            .last()
            .expect("non-empty after collect_file_infos");
        let list_end = inclusive_range_end(last.file_start, last.file_length)?;

        if start_position < list_start || start_position > list_end {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "Start position {start_position} is outside the available range \
                     [{list_start}, {list_end}]"
                ),
            ));
        }

        let start_file_idx =
            file_infos.partition_point(|fi| fi.file_start + (fi.file_length - 1) < start_position);

        let file_names: Vec<PathBuf> = file_names[start_file_idx..]
            .iter()
            .map(|file_name| file_name.as_ref().to_path_buf())
            .collect();
        let current_stream = Some(YcdSeqBlockStream::new_from(
            &file_names[0],
            unit_size,
            start_position,
        )?);

        Ok(Self {
            process_unit_size,
            file_names,
            current_stream,
            current_file: 0,
            next_process_no: 1,
            next_start_digit: start_position,
            current_process_unit: None,
        })
    }

    pub fn has_next(&self) -> bool {
        match &self.current_stream {
            Some(stream) => stream.has_next() || self.current_file + 1 < self.file_names.len(),
            None => self.current_file < self.file_names.len(),
        }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> io::Result<&YcdProcessUnit> {
        if !self.has_next() {
            return Err(no_more_data_error());
        }

        let mut value = String::new();
        value
            .try_reserve_exact(self.process_unit_size)
            .map_err(io::Error::other)?;
        while value.len() < self.process_unit_size && self.current_file < self.file_names.len() {
            if self.current_stream.is_none() {
                self.current_stream = Some(YcdSeqBlockStream::new(
                    &self.file_names[self.current_file],
                    self.process_unit_size,
                )?);
            }

            let remaining = self.process_unit_size - value.len();
            let stream = self
                .current_stream
                .as_mut()
                .expect("current stream was opened");
            value.push_str(&stream.take_digits(remaining)?);

            if !stream.has_next() {
                self.current_stream = None;
                self.current_file += 1;
            }
        }

        let process_no = self.next_process_no;
        let start_digit = self.next_start_digit;
        if self.has_next() {
            self.next_process_no = self
                .next_process_no
                .checked_add(1)
                .ok_or_else(|| invalid_data("Process number overflow"))?;
            self.next_start_digit = self
                .next_start_digit
                .checked_add(value.len())
                .ok_or_else(|| invalid_data("Digit position overflow"))?;
        }
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
        let list_end = inclusive_range_end(last.file_start, last.file_length)?;

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
            .partition_point(|e| e.file_start + (e.file_length - 1) < one_based_start_position);

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
                let block_digits = decode_digit_block(number)?;
                let block_str =
                    std::str::from_utf8(&block_digits).expect("decoded digits are ASCII");

                let usable = &block_str[skip..];
                skip = 0;

                let can_take = usable.len().min(to_take - taken);
                result.push_str(&usable[..can_take]);
                taken += can_take;
            }

            remaining -= to_take;
        }

        if result.len() != length {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "Decoded digits length is shorter than requested length",
            ));
        }
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
///
/// A zero `length` requires no blocks, regardless of the offset.
///
/// This function has no side effects and is exposed for unit-testing the direct-seek logic.
///
/// # Panics
///
/// Panics if `offset_in_block + length` overflows `usize`.
pub fn compute_seek_params(local_start: usize, length: usize) -> (usize, usize, usize) {
    let block_index = local_start / DIGITS_PER_BLOCK;
    let offset_in_block = local_start % DIGITS_PER_BLOCK;
    if length == 0 {
        return (block_index, offset_in_block, 0);
    }
    let total_digits = offset_in_block
        .checked_add(length)
        .expect("offset_in_block + length overflow");
    let blocks_to_read = total_digits.div_ceil(DIGITS_PER_BLOCK);
    (block_index, offset_in_block, blocks_to_read)
}

pub struct YcdFileUtil;

impl YcdFileUtil {
    pub fn get_header_size<P: AsRef<Path>>(file_name: P) -> io::Result<u64> {
        Ok(parse_metadata(file_name.as_ref())?.data_offset)
    }

    pub fn get_ycd_header<P: AsRef<Path>>(
        file_name: P,
    ) -> io::Result<HashMap<YcdHeaderInfoElem, String>> {
        Ok(parse_metadata(file_name.as_ref())?.header)
    }

    /// Read exactly `length` decimal digits of Pi starting at 1-based position
    /// `one_based_start_position` from the concatenation of the given YCD files.
    ///
    /// # Arguments
    ///
    /// * `files` — An ordered, contiguous slice of YCD file paths (BlockID order).
    ///   The first file need not have BlockID 0; absolute positions are derived
    ///   from each file's header.
    /// * `one_based_start_position` — 1-based index of the first digit to return.
    ///   Position 1 is the first decimal digit (the digit immediately after "3.").
    ///   The integer part, sign, and decimal point are never included.
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
        let list_end = inclusive_range_end(last.file_start, last.file_length)?;

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
            // file's last digit = fi.file_start + (fi.file_length - 1)
            fi.file_start + (fi.file_length - 1) < one_based_start_position
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
                let block_digits = decode_digit_block(number)?;
                let block_str =
                    std::str::from_utf8(&block_digits).expect("decoded digits are ASCII");

                // Skip unwanted leading digits in the first block.
                let usable = &block_str[skip..];
                skip = 0;

                let can_take = usable.len().min(to_take - taken);
                result.push_str(&usable[..can_take]);
                taken += can_take;
            }

            remaining -= to_take;
        }

        if result.len() != length {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "Decoded digits length is shorter than requested length",
            ));
        }
        Ok(result)
    }
}

fn validate_unit_size(unit_size: usize) -> io::Result<usize> {
    if unit_size < DIGITS_PER_BLOCK {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Unit size must be at least 19",
        ));
    }
    Ok(unit_size)
}

fn inclusive_range_end(start: usize, length: usize) -> io::Result<usize> {
    length
        .checked_sub(1)
        .and_then(|span| start.checked_add(span))
        .ok_or_else(|| invalid_data("Digit range end overflow"))
}

fn decode_digit_block(number: u64) -> io::Result<[u8; DIGITS_PER_BLOCK]> {
    let mut digits = [b'0'; DIGITS_PER_BLOCK];
    let mut remaining = number;
    for digit in digits.iter_mut().rev() {
        *digit += (remaining % 10) as u8;
        remaining /= 10;
    }

    if remaining != 0 {
        return Err(invalid_data(
            "A compressed block contains more than 19 decimal digits",
        ));
    }

    Ok(digits)
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
        let text = text.trim();
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
    let block_size = parse_positive_usize(&header, YcdHeaderInfoElem::Blocksize)?;
    let block_id = parse_usize(&header, YcdHeaderInfoElem::BlockID)?;

    let version = required_value(&header, YcdHeaderInfoElem::FileVersion)?;
    if version.is_empty() {
        return Err(invalid_data("FileVersion must not be empty"));
    }
    if required_value(&header, YcdHeaderInfoElem::FirstDigits)?.is_empty() {
        return Err(invalid_data("FirstDigits must not be empty"));
    }

    let base = parse_usize(&header, YcdHeaderInfoElem::Base)?;
    if base != 10 {
        return Err(invalid_data("Only base 10 YCD files are supported"));
    }
    let total_digits = parse_usize(&header, YcdHeaderInfoElem::TotalDigits)?;
    if header.contains_key(&YcdHeaderInfoElem::TotalBlocks) {
        parse_usize(&header, YcdHeaderInfoElem::TotalBlocks)?;
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
    let _ = inclusive_range_end(digit_start, digit_length)?;

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

fn parse_positive_usize(
    header: &HashMap<YcdHeaderInfoElem, String>,
    key: YcdHeaderInfoElem,
) -> io::Result<usize> {
    let value = parse_usize(header, key)?;
    if value == 0 {
        return Err(invalid_data(format!("{} must be positive", key.as_ref())));
    }
    Ok(value)
}

fn parse_usize(
    header: &HashMap<YcdHeaderInfoElem, String>,
    key: YcdHeaderInfoElem,
) -> io::Result<usize> {
    required_value(header, key)?
        .parse::<usize>()
        .map_err(|_| invalid_data(format!("{} must be a nonnegative integer", key.as_ref())))
}

/// Parse metadata for every file in `files`, validate header correctness and
/// list continuity, and return a `Vec<FileInfo>` in the same order.
///
/// All files are validated regardless of whether the requested range touches them.
fn collect_file_infos<P: AsRef<Path>>(files: &[P]) -> io::Result<Vec<FileInfo>> {
    let mut infos: Vec<FileInfo> = Vec::with_capacity(files.len());

    for path in files {
        let meta = parse_metadata(path.as_ref())?;

        let file_start = meta.digit_start;
        let file_length = meta.digit_length;

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
    use super::decode_digit_block;
    use std::io;

    #[test]
    fn decode_digit_block_pads_to_nineteen_digits() {
        assert_eq!(decode_digit_block(42).unwrap(), *b"0000000000000000042");
        assert_eq!(
            decode_digit_block(9_999_999_999_999_999_999).unwrap(),
            *b"9999999999999999999"
        );
    }

    #[test]
    fn decode_digit_block_rejects_twenty_digits() {
        assert_eq!(
            decode_digit_block(u64::MAX).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }
}
