#[cfg(test)]
mod tests {
    const BASE_PATH: &str = "tests/ycd/";
    const BASE_FILENAME: &str = "/Pi - Dec - Chudnovsky - ";
    const BASE_EXT: &str = ".ycd";

    const SAMPLE_DATA_FILENAME: &str = "tests/ycd/Pi - Dec - Chudnovsky_3000000.txt";
    

    #[test]
    fn test_get_header_size() {
        let file_name = &format!("{}1000000{}0{}", BASE_PATH, BASE_FILENAME, BASE_EXT);
        let header_size = ycd_reader::YcdFileUtil::get_header_size(file_name);
        assert_eq!(header_size.unwrap(), 195);

        let file_name = &format!("{}1000000{}1{}", BASE_PATH, BASE_FILENAME, BASE_EXT);
        let header_size = ycd_reader::YcdFileUtil::get_header_size(file_name);
        assert_eq!(header_size.unwrap(), 195);

        let file_name = &format!("{}1000000{}14{}", BASE_PATH, BASE_FILENAME, BASE_EXT);
        let header_size = ycd_reader::YcdFileUtil::get_header_size(file_name);
        assert_eq!(header_size.unwrap(), 196); // 195 + 1  桁が増えるとヘッダーが1バイト増える

        // ----- 2000000桁 -----
        let file_name = &format!("{}2000000{}0{}", BASE_PATH, BASE_FILENAME, BASE_EXT);
        let header_size = ycd_reader::YcdFileUtil::get_header_size(file_name);
        assert_eq!(header_size.unwrap(), 195);

        let file_name = &format!("{}2000000{}2{}", BASE_PATH, BASE_FILENAME, BASE_EXT);
        let header_size = ycd_reader::YcdFileUtil::get_header_size(file_name);
        assert_eq!(header_size.unwrap(), 195);
    }

    #[test]
    fn read_first20() {
        // ----- 1000000
        let file_name = format!("{}1000000{}0{}", BASE_PATH, BASE_FILENAME, BASE_EXT);
        let mut stream = ycd_reader::YcdSeqBlockStream::new(&file_name, 20).unwrap();

        let c = stream.next().unwrap();

        assert_eq!(c.start_digit, 1);
        assert_eq!(c.value.len(), 20);
        assert_eq!(c.value, "14159265358979323846");

        // ----- 2000000
        let file_name = format!("{}2000000{}0{}", BASE_PATH, BASE_FILENAME, BASE_EXT);
        let mut stream = ycd_reader::YcdSeqBlockStream::new(&file_name, 20).unwrap();

        let c = stream.next().unwrap();

        assert_eq!(c.start_digit, 1);
        assert_eq!(c.value.len(), 20);
        assert_eq!(c.value, "14159265358979323846");
    }

    #[test]
    fn read_first19() {
        // ----- 1000000
        let file_name = format!("{}1000000{}0{}", BASE_PATH, BASE_FILENAME, BASE_EXT);
        let mut stream = ycd_reader::YcdSeqBlockStream::new(&file_name, 19).unwrap();
        let c = stream.next().unwrap();

        assert_eq!(c.start_digit, 1);
        assert_eq!(c.value.len(), 19);
        assert_eq!(c.value, "1415926535897932384");

        // ----- 2000000
        let file_name = format!("{}2000000{}0{}", BASE_PATH, BASE_FILENAME, BASE_EXT);
        let mut stream = ycd_reader::YcdSeqBlockStream::new(&file_name, 19).unwrap();
        let c = stream.next().unwrap();

        assert_eq!(c.start_digit, 1);
        assert_eq!(c.value.len(), 19);
        assert_eq!(c.value, "1415926535897932384");
    }

    use std::fs::File;
    use std::io::{ self, Read, Seek, SeekFrom };


    //正解テキストファイルから指定の位置と長さの文字列を読み込む
    fn read_digits_from_file(path: &str, start: usize, length: usize) -> io::Result<String> {

        // ファイルを開いて指定の位置にシーク
        let mut file = File::open(path)?;
        file.seek(SeekFrom::Start((start-1) as u64))?;

        // 指定の長さだけバッファを読み込む
        let mut buffer = vec![0; length];
        file.read_exact(&mut buffer)?;

        // バッファをそのまま文字列化
        Ok(String::from_utf8_lossy(&buffer).to_string())
    }


    #[test]
    fn read_txt() {

        let ans = read_digits_from_file(&SAMPLE_DATA_FILENAME, 1, 20).unwrap();

        let file_name = format!("{}1000000{}0{}", BASE_PATH, BASE_FILENAME, BASE_EXT);
        let mut stream = ycd_reader::YcdSeqBlockStream::new(&file_name, 20).unwrap();
        let c = stream.next().unwrap();

        assert_eq!(c.value, ans);

    }


}
