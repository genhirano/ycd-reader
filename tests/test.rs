#[cfg(test)]
mod tests {
    const BASE_PATH_1000000: &str = "tests/ycd/1000000/Pi - Dec - Chudnovsky - ";
    const BASE_PATH_2000000: &str = "tests/ycd/2000000/Pi - Dec - Chudnovsky - ";
    const BASE_EXT: &str = ".ycd";

    const SAMPLE_DATA_FILENAME: &str = "tests/ycd/Pi - Dec - Chudnovsky_3000000.txt";

    #[test]
    fn test_get_header_size() {
        let file_name = &format!("{}0{}", BASE_PATH_1000000, BASE_EXT);
        let header_size = ycd_reader::YcdFileUtil::get_header_size(file_name);
        assert_eq!(header_size.unwrap(), 195);

        let file_name = &format!("{}1{}", BASE_PATH_1000000, BASE_EXT);
        let header_size = ycd_reader::YcdFileUtil::get_header_size(file_name);
        assert_eq!(header_size.unwrap(), 195);

        let file_name = &format!("{}14{}", BASE_PATH_1000000, BASE_EXT);
        let header_size = ycd_reader::YcdFileUtil::get_header_size(file_name);
        assert_eq!(header_size.unwrap(), 196); // 195 + 1  桁が増えるとヘッダーが1バイト増える

        // ----- 2000000桁 -----
        let file_name = &format!("{}0{}", BASE_PATH_2000000, BASE_EXT);
        let header_size = ycd_reader::YcdFileUtil::get_header_size(file_name);
        assert_eq!(header_size.unwrap(), 195);

        let file_name = &format!("{}2{}", BASE_PATH_2000000, BASE_EXT);
        let header_size = ycd_reader::YcdFileUtil::get_header_size(file_name);
        assert_eq!(header_size.unwrap(), 195);
    }

    #[test]
    fn read_first20() {
        // ----- 1000000
        let file_name = &format!("{}0{}", BASE_PATH_1000000, BASE_EXT);
        let mut stream = ycd_reader::YcdSeqBlockStream::new(&file_name, 20).unwrap();

        let c = stream.next().unwrap();

        assert_eq!(c.start_digit, 1);
        assert_eq!(c.value.len(), 20);
        assert_eq!(c.value, "14159265358979323846");

        // ----- 2000000
        let file_name = &format!("{}0{}", BASE_PATH_2000000, BASE_EXT);
        let mut stream = ycd_reader::YcdSeqBlockStream::new(&file_name, 20).unwrap();

        let c = stream.next().unwrap();

        assert_eq!(c.start_digit, 1);
        assert_eq!(c.value.len(), 20);
        assert_eq!(c.value, "14159265358979323846");
    }

    #[test]
    fn read_first19() {
        // ----- 1000000
        let file_name = &format!("{}0{}", BASE_PATH_1000000, BASE_EXT);
        let mut stream = ycd_reader::YcdSeqBlockStream::new(&file_name, 19).unwrap();
        let c = stream.next().unwrap();

        assert_eq!(c.start_digit, 1);
        assert_eq!(c.value.len(), 19);
        assert_eq!(c.value, "1415926535897932384");

        // ----- 2000000
        let file_name = &format!("{}0{}", BASE_PATH_2000000, BASE_EXT);
        let mut stream = ycd_reader::YcdSeqBlockStream::new(&file_name, 19).unwrap();
        let c = stream.next().unwrap();

        assert_eq!(c.start_digit, 1);
        assert_eq!(c.value.len(), 19);
        assert_eq!(c.value, "1415926535897932384");
    }

    use std::fs::File;
    use std::io::{Read, Seek, SeekFrom};

    //正解テキストファイルから指定の位置と長さの文字列を読み込む
    fn read_digits_from_file(start: usize, length: usize) -> Result<String, std::io::Error> {
        // ファイルを開いて指定の位置にシーク
        let mut file = File::open(&SAMPLE_DATA_FILENAME)?;
        file.seek(SeekFrom::Start((start - 1) as u64))?;

        // 指定の長さだけバッファを読み込む
        let mut buffer = vec![0; length];
        file.read_exact(&mut buffer)?;

        // バッファをそのまま文字列化
        Ok(String::from_utf8_lossy(&buffer).to_string())
    }

    #[test]
    fn first_and_second_unit_test() {
        let expected = read_digits_from_file(1, 100).unwrap();

        //Load at UnitSize is 20 14159265358979323846
        let unit_size = 20;
        let file_name = &format!("{}0{}", BASE_PATH_1000000, BASE_EXT);
        let mut stream = ycd_reader::YcdSeqBlockStream::new(&file_name, unit_size).unwrap();

        //first unit
        let mut actualunit = stream.next().unwrap();

        //1st digit comparison
        let s1 = &actualunit.value[0..1];
        let e1 = &expected[0..1];
        assert_eq!(s1, e1);

        //2nd digit comparison
        let s2 = &actualunit.value[1..2];
        let e2 = &expected[1..2];
        assert_eq!(s2, e2);

        let l = 1;
        let r = 2;
        assert_eq!(&actualunit.value[l..r], &expected[l..r]);

        let l = 0;
        let r = 18;
        assert_eq!(&actualunit.value[l..r], &expected[l..r]);

        let l = 0;
        let r = 20;
        assert_eq!(&actualunit.value[l..r], &expected[l..r]);

        //second unit
        actualunit = stream.next().unwrap();

        let l = 0;
        let r = 20;
        assert_eq!(
            &actualunit.value[l..r],
            &expected[l + (unit_size as usize)..r + (unit_size as usize)]
        );
    }

    #[test]
    fn ycd_end_of_file_test() {
        //Tests that span file boundaries

        let expected = read_digits_from_file(999000, 2000).unwrap();

        let unit_size = 21;
        let file_name = &format!("{}0{}", BASE_PATH_1000000, BASE_EXT);
        let mut stream = ycd_reader::YcdSeqBlockStream::new(&file_name, unit_size).unwrap();

        //0315614033 3212728491 9441843715 0696552087 5424505989  :  999,950
        //5678796130 3311646283 9963464604 2209010610 5779458151  :  1,000,000

        while stream.has_next() {
            let actualunit = stream.next().unwrap();

            if actualunit.start_digit >= 999000 {
                let acpoint: usize = actualunit.start_digit as usize - 999000;

                if actualunit.value.len() >= 15 {
                    assert_eq!(&actualunit.value[0..15], &expected[acpoint..acpoint + 15]);
                } else {
                    assert_eq!(
                        &actualunit.value[0..actualunit.value.len()],
                        &expected[acpoint..acpoint + actualunit.value.len()]
                    );
                }
                
            }
        }
    }
}
