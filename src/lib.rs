use std::collections::HashMap;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Seek};
use std::path::Path;

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
    pub process_no: i64,
    pub start_digit: i64,
    pub value: String,
}

impl YcdProcessUnit {
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

    pub fn has_next(&self) -> bool {
        self.decoded_digits < self.digit_length || !self.surplus_digit_str.is_empty()
    }

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

    pub fn has_next(&self) -> bool {
        self.streams[self.current_stream..]
            .iter()
            .any(YcdSeqBlockStream::has_next)
    }

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

pub struct YcdFileUtil;

impl YcdFileUtil {
    pub fn get_header_size<P: AsRef<Path>>(file_name: P) -> io::Result<i32> {
        i32::try_from(parse_metadata(file_name.as_ref())?.data_offset)
            .map_err(|_| invalid_data("YCD header is too large"))
    }

    pub fn get_ycd_header<P: AsRef<Path>>(
        file_name: P,
    ) -> io::Result<HashMap<YcdHeaderInfoElem, String>> {
        Ok(parse_metadata(file_name.as_ref())?.header)
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

fn invalid_data(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

fn no_more_data_error() -> io::Error {
    io::Error::new(io::ErrorKind::UnexpectedEof, "No more data to read")
}
