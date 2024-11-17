
#[cfg(test)]
mod tests {

    #[test]
    fn test_get_header_size_valid() {

        let file_name = "tests/ycd/1000000/Pi - Dec - Chudnovsky - 0.ycd";
        let header_size = ycd_reader::YcdFileUtil::get_header_size(file_name);
        assert_eq!(header_size.unwrap(), 195);

        let file_name = "tests/ycd/1000000/Pi - Dec - Chudnovsky - 1.ycd";
        let header_size = ycd_reader::YcdFileUtil::get_header_size(file_name);
        assert_eq!(header_size.unwrap(), 195);

        let file_name = "tests/ycd/1000000/Pi - Dec - Chudnovsky - 14.ycd";
        let header_size = ycd_reader::YcdFileUtil::get_header_size(file_name);
        assert_eq!(header_size.unwrap(), 196); // 195 + 1  桁が増えるとヘッダーが1バイト増える

        // -----
        let file_name = "tests/ycd/2000000/Pi - Dec - Chudnovsky - 0.ycd";
        let header_size = ycd_reader::YcdFileUtil::get_header_size(file_name);
        assert_eq!(header_size.unwrap(), 195);
        
        let file_name = "tests/ycd/2000000/Pi - Dec - Chudnovsky - 2.ycd";
        let header_size = ycd_reader::YcdFileUtil::get_header_size(file_name);
        assert_eq!(header_size.unwrap(), 195);

    }

}
