use std::fs::File;
use std::io::{self, BufReader, Read, Seek, SeekFrom};
use std::collections::HashMap;

use strum_macros::{AsRefStr, EnumString};


// const char YCF_CDF_FileVersion[]        =   "1.1.0";
// const char YCF_CDF_TOKEN_FileVersion[]  =   "FileVersion:";
// const char YCF_CDF_TOKEN_Base[]         =   "Base:";
// const char YCF_CDF_TOKEN_FirstDigits[]  =   "FirstDigits:";
// const char YCF_CDF_TOKEN_TotalDigits[]  =   "TotalDigits:";
// const char YCF_CDF_TOKEN_BlockSize[]    =   "Blocksize:";
// const char YCF_CDF_TOKEN_TotalBlocks[]  =   "TotalBlocks:";
// const char YCF_CDF_TOKEN_BlockID[]      =   "BlockID:";
// const char YCF_CDF_TOKEN_EndHeader[]    =   "EndHeader";


// YCDヘッダー情報の列挙型
#[derive(Debug, Clone, Eq, PartialEq, Hash, AsRefStr, EnumString)]
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

// 処理ユニット構造体
#[derive(Debug)]
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

// シーケンシャルブロックストリーム構造体
pub struct YcdSeqBlockStream {
    process_unit_size: i32,
    file_path: String,
    file_stream: Option<BufReader<File>>,
    digit_length: i64,
    current_process_unit: Option<YcdProcessUnit>,
    surplus_digit_str: String,
    current_block: i64,
    is_closed: bool,
    current_read_block_seq: i64,
}

impl YcdSeqBlockStream {
    pub fn new(file_name: &str, unit_size: i32) -> io::Result<Self> {
        if unit_size < 19 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Unit size must be at least 19",
            ));
        }

        let header_info = YcdFileUtil::get_ycd_header(file_name)?;
        let digit_length = header_info.get(&YcdHeaderInfoElem::Blocksize)
            .and_then(|s| s.parse::<i64>().ok())
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Invalid block size"))?;

        let block_id = header_info.get(&YcdHeaderInfoElem::BlockID)
            .and_then(|s| s.parse::<i32>().ok())
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Invalid block ID"))?;

        let digit_start = 1 + digit_length * i64::from(block_id);

        let mut stream = Self {
            process_unit_size: unit_size,
            file_path: file_name.to_string(),
            file_stream: None,
            digit_length,
            current_process_unit: None,
            surplus_digit_str: String::new(),
            current_block: 1,
            is_closed: true,
            current_read_block_seq: 0,
        };

        stream.open()?;
        stream.current_process_unit = Some(YcdProcessUnit::new(
            0,
            digit_start - i64::from(unit_size),
            String::new(),
        ));

        Ok(stream)
    }

    fn open(&mut self) -> io::Result<()> {
        let header_size = YcdFileUtil::get_header_size(&self.file_path)?;
        let file = File::open(&self.file_path)?;
        let mut reader = BufReader::new(file);
        
        // Seek to the end of the header
        reader.seek(SeekFrom::Start(u64::try_from(header_size).unwrap()))?;
        
        self.file_stream = Some(reader);
        self.is_closed = false;
        Ok(())
    }

    pub fn has_next(&self) -> bool {
        if self.has_next_read_block() {
            true
        } else {
            !self.surplus_digit_str.is_empty()
        }
    }

    fn has_next_read_block(&self) -> bool {
        self.digit_length + 19 >= self.current_block * 19
    }

    pub fn next(&mut self) -> io::Result<&YcdProcessUnit> {
        if !self.has_next() {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "No more data to read",
            ));
        }

        let mut sb = self.surplus_digit_str.clone();
        let mut data = String::new();

        while self.has_next_read_block() || !sb.is_empty() {
            if self.has_next_read_block() {
                let block_data = self.next_block_read()?;
                sb.push_str(&block_data);
            }

            if sb.len() >= self.process_unit_size as usize {
                data = sb[..self.process_unit_size as usize].to_string();
                self.surplus_digit_str = sb[self.process_unit_size as usize..].to_string();
                break;
            } else if !self.has_next_read_block() {
                data = sb;
                self.surplus_digit_str = String::new();
                break;
            }
        }

        if let Some(current_unit) = self.current_process_unit.as_mut() {
            let start = current_unit.start_digit + i64::from(self.process_unit_size);
            current_unit.process_no += 1;
            current_unit.start_digit = start;
            current_unit.value = data;
        }

        Ok(self.current_process_unit.as_ref().unwrap())
    }

    fn next_block_read(&mut self) -> io::Result<String> {
        
        let mut buffer = [0u8; 8];
        
        if let Some(stream) = self.file_stream.as_mut() {
            stream.read_exact(&mut buffer)?;
        } else {
            return Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "Stream not initialized",
            ));
        }

        self.current_read_block_seq += 1;

        // Convert little-endian bytes to u64 and then to string
        let num = u64::from_le_bytes(buffer);
        let mut num_str = num.to_string();

        // Pad with leading zeros to 19 digits
        if num_str.len() < 19 {
            num_str = format!("{:0>19}", num_str);
        }

        // Truncate if needed
        let read_end_digit = self.current_block * 19;
        if read_end_digit > self.digit_length {
            let over = read_end_digit - self.digit_length;
            num_str.truncate((19 - over) as usize);
        }

        self.current_block += 1;
        Ok(num_str)
    }
}

impl Drop for YcdSeqBlockStream {
    fn drop(&mut self) {
        if !self.is_closed {
            let _ = self.file_stream.take();
            self.is_closed = true;
        }
    }
}




// YCDファイルユーティリティ
pub struct YcdFileUtil;

impl YcdFileUtil {
    pub fn get_header_size(file_name: &str) -> io::Result<i32> {
        let file = File::open(file_name)?;
        let mut reader = BufReader::new(file);
        let mut buffer = [0u8; 300];
        reader.read(&mut buffer)?;

        let header = String::from_utf8_lossy(&buffer);
        if let Some(pos) = header.rfind("\r\n") {
            Ok(pos as i32 + 3) // +3 for CRLF and extra byte
        } else {
            Err(io::Error::new(io::ErrorKind::InvalidData, "Invalid header format"))
        }
    }

    pub fn get_ycd_header(file_name: &str) -> io::Result<HashMap<YcdHeaderInfoElem, String>> {
        let file = File::open(file_name)?;
        let reader = BufReader::new(file);
        let mut map = HashMap::new();

        for line in io::BufRead::lines(reader) {
            let line = line?;
            if line == "EndHeader" {
                break;
            }
            if line.is_empty() {
                continue;
            }

            let parts: Vec<&str> = line.split(':').collect();
            if parts.len() != 2 {
                continue;
            }

            let key = match parts[0].trim() {
                s if s == YcdHeaderInfoElem::FileVersion.as_ref() => Some(YcdHeaderInfoElem::FileVersion),
                s if s == YcdHeaderInfoElem::Base.as_ref() => Some(YcdHeaderInfoElem::Base),
                s if s == YcdHeaderInfoElem::FirstDigits.as_ref() => Some(YcdHeaderInfoElem::FirstDigits),
                s if s == YcdHeaderInfoElem::TotalDigits.as_ref() => Some(YcdHeaderInfoElem::TotalDigits),
                s if s == YcdHeaderInfoElem::Blocksize.as_ref() => Some(YcdHeaderInfoElem::Blocksize),
                s if s == YcdHeaderInfoElem::BlockID.as_ref() => Some(YcdHeaderInfoElem::BlockID),
                _ => None,
            };

            if let Some(key) = key {
                map.insert(key, parts[1].trim().to_string());
            }
        }

        if map.contains_key(&YcdHeaderInfoElem::FileVersion) &&
           map.contains_key(&YcdHeaderInfoElem::Base) &&
           map.contains_key(&YcdHeaderInfoElem::FirstDigits) &&
           map.contains_key(&YcdHeaderInfoElem::TotalDigits) &&
           map.contains_key(&YcdHeaderInfoElem::Blocksize) &&
           map.contains_key(&YcdHeaderInfoElem::BlockID) {
            Ok(map)
        } else {
            println!("{:?}", map);
            Err(io::Error::new(io::ErrorKind::InvalidData, "Missing required header fields"))
        }
    }
}



