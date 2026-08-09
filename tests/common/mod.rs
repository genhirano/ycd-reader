#![allow(dead_code)]

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use ycd_reader::{YcdFileUtil, YcdHeaderInfoElem, YcdMultiFileStream, YcdSeqBlockStream};

const GOLDEN_PATH: &str = "tests/ycd/Pi - Dec - Chudnovsky_3000000.txt";
const ONE_MILLION_BASE: &str = "tests/ycd/1000000/Pi - Dec - Chudnovsky - ";
const TWO_MILLION_BASE: &str = "tests/ycd/2000000/Pi - Dec - Chudnovsky - ";

static TEMP_FILE_ID: AtomicU64 = AtomicU64::new(0);

pub struct TempYcd {
    pub path: PathBuf,
}

impl TempYcd {
    pub fn from_bytes(bytes: &[u8]) -> io::Result<Self> {
        let id = TEMP_FILE_ID.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("ycd-reader_{}_{}.ycd", std::process::id(), id));
        fs::write(&path, bytes)?;
        Ok(Self { path })
    }

    pub fn valid(
        digits: &str,
        block_size: usize,
        block_id: usize,
        line_ending: &str,
        extra_header: &str,
    ) -> io::Result<Self> {
        assert_eq!(digits.len(), block_size);
        let header = format!(
            "#Compressed Digit File{e}{e}\
             FileVersion:\t1.1.0{e}{e}\
             Base:\t10{e}{e}\
             FirstDigits:\t3.14159265358979323846{e}{e}\
             TotalDigits:\t0{e}{e}\
             {extra_header}\
             Blocksize:\t{block_size}{e}\
             BlockID:\t{block_id}{e}{e}\
             EndHeader{e}{e}",
            e = line_ending
        );
        let mut bytes = header.into_bytes();
        bytes.push(0);
        bytes.extend(encode_digits(digits));
        Self::from_bytes(&bytes)
    }

    pub fn final_block(
        digits: &str,
        block_size: usize,
        block_id: usize,
        total_digits: usize,
    ) -> io::Result<Self> {
        let header = format!(
            "FileVersion: 1.1.0\n\
             Base: 10\n\
             FirstDigits: 3.14159265358979323846\n\
             TotalDigits: {total_digits}\n\
             Blocksize: {block_size}\n\
             BlockID: {block_id}\n\
             EndHeader\n\n\0"
        );
        let mut bytes = header.into_bytes();
        bytes.extend(encode_digits(digits));
        Self::from_bytes(&bytes)
    }
}

impl Drop for TempYcd {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

pub fn one_million_path(block_id: usize) -> String {
    format!("{ONE_MILLION_BASE}{block_id}.ycd")
}

pub fn two_million_path(block_id: usize) -> String {
    format!("{TWO_MILLION_BASE}{block_id}.ycd")
}

pub fn golden_digits() -> String {
    fs::read_to_string(GOLDEN_PATH).expect("golden digit file must be readable")
}

fn encode_digits(digits: &str) -> Vec<u8> {
    let mut encoded = Vec::new();
    for chunk in digits.as_bytes().chunks(19) {
        let mut block = String::from_utf8(chunk.to_vec()).expect("test digits are ASCII");
        while block.len() < 19 {
            block.push('0');
        }
        let number = block.parse::<u64>().expect("test block must fit in u64");
        encoded.extend(number.to_le_bytes());
    }
    encoded
}

pub fn collect_single(path: impl AsRef<Path>, unit_size: i32) -> io::Result<String> {
    let header = YcdFileUtil::get_ycd_header(path.as_ref())?;
    let block_size = header[&YcdHeaderInfoElem::Blocksize]
        .parse::<i64>()
        .unwrap();
    let block_id = header[&YcdHeaderInfoElem::BlockID].parse::<i64>().unwrap();
    let mut expected_start = 1 + block_size * block_id;
    let mut expected_process_no = 1;
    let mut result = String::new();
    let mut stream = YcdSeqBlockStream::new(path, unit_size)?;

    while stream.has_next() {
        let unit = stream.next()?.clone();
        assert_eq!(unit.process_no, expected_process_no);
        assert_eq!(unit.start_digit, expected_start);
        expected_process_no += 1;
        expected_start += unit.value.len() as i64;
        result.push_str(&unit.value);
    }

    assert_eq!(
        stream.next().unwrap_err().kind(),
        io::ErrorKind::UnexpectedEof
    );
    Ok(result)
}

pub fn collect_single_prefix(
    path: impl AsRef<Path>,
    unit_size: i32,
    length: usize,
) -> io::Result<String> {
    let mut result = String::new();
    let mut stream = YcdSeqBlockStream::new(path, unit_size)?;
    while result.len() < length {
        result.push_str(&stream.next()?.value);
    }
    result.truncate(length);
    Ok(result)
}

pub fn collect_multi<P: AsRef<Path>>(paths: &[P], unit_size: i32) -> io::Result<String> {
    let first_header = YcdFileUtil::get_ycd_header(paths[0].as_ref())?;
    let block_size = first_header[&YcdHeaderInfoElem::Blocksize]
        .parse::<i64>()
        .unwrap();
    let block_id = first_header[&YcdHeaderInfoElem::BlockID]
        .parse::<i64>()
        .unwrap();
    let mut expected_start = 1 + block_size * block_id;
    let mut expected_process_no = 1;
    let mut result = String::new();
    let mut stream = YcdMultiFileStream::new(paths, unit_size)?;

    while stream.has_next() {
        let unit = stream.next()?.clone();
        assert_eq!(unit.process_no, expected_process_no);
        assert_eq!(unit.start_digit, expected_start);
        expected_process_no += 1;
        expected_start += unit.value.len() as i64;
        result.push_str(&unit.value);
    }

    assert_eq!(
        stream.next().unwrap_err().kind(),
        io::ErrorKind::UnexpectedEof
    );
    Ok(result)
}

pub fn collect_single_from(
    path: impl AsRef<Path>,
    unit_size: i32,
    start_position: i64,
) -> io::Result<String> {
    let mut result = String::new();
    let mut stream = YcdSeqBlockStream::new_from(path, unit_size, start_position)?;
    let mut expected_start = start_position;
    let mut expected_process_no: i64 = 1;

    while stream.has_next() {
        let unit = stream.next()?.clone();
        assert_eq!(unit.process_no, expected_process_no);
        assert_eq!(unit.start_digit, expected_start);
        expected_process_no += 1;
        expected_start += unit.value.len() as i64;
        result.push_str(&unit.value);
    }
    assert_eq!(
        stream.next().unwrap_err().kind(),
        io::ErrorKind::UnexpectedEof
    );
    Ok(result)
}

pub fn collect_multi_from<P: AsRef<Path>>(
    paths: &[P],
    unit_size: i32,
    start_position: i64,
) -> io::Result<String> {
    let mut result = String::new();
    let mut stream = YcdMultiFileStream::new_from(paths, unit_size, start_position)?;
    let mut expected_start = start_position;
    let mut expected_process_no: i64 = 1;

    while stream.has_next() {
        let unit = stream.next()?.clone();
        assert_eq!(unit.process_no, expected_process_no);
        assert_eq!(unit.start_digit, expected_start);
        expected_process_no += 1;
        expected_start += unit.value.len() as i64;
        result.push_str(&unit.value);
    }
    assert_eq!(
        stream.next().unwrap_err().kind(),
        io::ErrorKind::UnexpectedEof
    );
    Ok(result)
}
