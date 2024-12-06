 // src/payload_parser/mod.rs
 use crate::payload_field::PayloadField;

// common 크레이트에서 직접 가져옵니다
use common::data_types::krx_msg::KrxMsg;



#[derive(Debug)]
pub enum ParsedValue {
    Double(f64),
    Integer(i32),
    Text(String),
}

fn bytes_to_string(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| b as char).collect()
}

fn bytes_to_f64(bytes: &[u8]) -> f64 {
    let mut result = 0.0;
    let mut is_negative = false;
    let mut i = 0;

    if !bytes.is_empty() && bytes[0] == b'-' {
        is_negative = true;
        i = 1;
    }

    while i < bytes.len() && bytes[i].is_ascii_digit() {
        result = result * 10.0 + (bytes[i] - b'0') as f64;
        i += 1;
    }

    if i < bytes.len() && bytes[i] == b'.' {
        i += 1;
        let mut decimal = 0.1;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            result += (bytes[i] - b'0') as f64 * decimal;
            decimal *= 0.1;
            i += 1;
        }
    }

    if is_negative {
        -result
    } else {
        result
    }
}

fn bytes_to_i32(bytes: &[u8]) -> i32 {
    let mut result = 0;
    let mut is_negative = false;
    let mut i = 0;

    if !bytes.is_empty() && bytes[0] == b'-' {
        is_negative = true;
        i = 1;
    }

    while i < bytes.len() && bytes[i].is_ascii_digit() {
        result = result * 10 + (bytes[i] - b'0') as i32;
        i += 1;
    }

    if is_negative {
        -result
    } else {
        result
    }
}

pub fn parse_data(data: &[u8], data_type: &str) -> ParsedValue {
    match data_type {
        "Double" => ParsedValue::Double(bytes_to_f64(data)),
        "Int" => ParsedValue::Integer(bytes_to_i32(data)),
        "String" => ParsedValue::Text(bytes_to_string(data)),
        _ => ParsedValue::Text("out of data type".to_string())
    }
}

// parse_packet 함수 : 패킷 한줄 / 필드 정보 / 필드 정보 idx 를 바탕으로 필드 정보에 맞게 데이터를 파싱

pub fn parse_packet(packet: &pcap::Packet, fields: &[PayloadField], field_idx: usize) -> Option<ParsedValue> {
    let field = &fields[field_idx];
    let payload = &packet.data[42..]; // Assume payload starts after Ethernet/IP/UDP headers

    if payload.len() < field.cumulative_length as usize {
        return None;
    }

    let data = &payload[field.start_point as usize..field.cumulative_length as usize];
    Some(parse_data(data, &field.data_type))
}

pub fn parse_json_db(krx_msg: &KrxMsg, fields: &[PayloadField], field_idx: usize) -> Option<ParsedValue> {
    let field = &fields[field_idx];
    let payload = &krx_msg.payload; // Assume payload starts after Ethernet/IP/UDP headers

    if payload.len() < field.cumulative_length as usize {
        return None;
    }

    let data = &payload[field.start_point as usize..field.cumulative_length as usize];
    Some(parse_data(data, &field.data_type))
}



#[cfg(test)]
mod tests {

    use super::*;
    use struson::reader::{JsonStreamReader, JsonReader};
    use common::data_types::krx_msg::KrxMsg;
    use pcap::Capture;


    #[test]
    fn test_payload_parser() -> Result<(), Box<dyn std::error::Error>> {
        let csv_path = "data/BF606F_new.csv";
        let pcap_path = "data/USD_Fwd_data.pcap";

        // 파일 존재 여부 체크
        if !std::path::Path::new(csv_path).exists() {
            println!("CSV file not found: {}", csv_path);
            return Ok(());
        }
        if !std::path::Path::new(pcap_path).exists() {
            println!("PCAP file not found: {}", pcap_path);
            return Ok(());
        }

        let fields = PayloadField::load_from_csv(csv_path)?;
        let mut capture = Capture::from_file(pcap_path)?;

        let mut results = Vec::new();
        let mut processed_count = 0;
        let mut skip_num = 0;
        let skip_count = 500;
        let max_count = 20;

        while let Ok(packet) = capture.next_packet() {
            if skip_num < skip_count {
                skip_num += 1;
                continue;
            }

            if packet.data.len() > 42 {
                if let Some(parsed_value) = parse_packet(&packet, &fields, 8) {
                    results.push(parsed_value);
                    processed_count += 1;

                    if processed_count >= max_count {
                        break;
                    }
                }
            }
        }

        assert!(!results.is_empty());
        println!("\nParsed Values:");
        for (i, value) in results.iter().enumerate() {
            match value {
                ParsedValue::Double(v) => println!("{:2}. Value: {:.1}", i + 1, v),
                ParsedValue::Integer(v) => println!("{:2}. Value: {}", i + 1, v),
                ParsedValue::Text(v) => println!("{:2}. Value: {}", i + 1, v),
            }
        }

        Ok(())
    }

    // json 파일을 STREAM 으로 읽어서 Payload FIELD 파싱

    #[test]
    fn stream_json_with_parser() -> anyhow::Result<()> {
        const INST_CODE_PREFIX: &str = "KR4175";
        const FIELD_INDEX: usize = 8;
        const MAX_SAMPLES: usize = 10;

        // 테스트 데이터 준비
        let json_path = "data/multiasset_db.krx_msg.json";
        let csv_path = "data/BF606F_new.csv";

        // 파일 존재 확인
        if !std::path::Path::new(json_path).exists() {
            anyhow::bail!("JSON file not found: {}", json_path);
        }
        if !std::path::Path::new(csv_path).exists() {
            anyhow::bail!("CSV file not found: {}", csv_path);
        }

        // 파일 및 필드 로드
        let file = std::fs::File::open(json_path)?;
        let reader = std::io::BufReader::new(file);
        let mut stream_reader = JsonStreamReader::new(reader);
        
        let fields = PayloadField::load_from_csv(csv_path)
            .map_err(|e| anyhow::anyhow!("Failed to load CSV: {}", e.to_string()))?;

        // 결과 저장용 벡터
        let mut parsed_values = Vec::new();
        
        stream_reader.begin_array()?;
        
        while stream_reader.has_next()? && parsed_values.len() < MAX_SAMPLES {
            let krx_msg: KrxMsg = stream_reader.deserialize_next()?;

            if krx_msg.instcode
                .as_ref()
                .map_or(false, |code| code.starts_with(INST_CODE_PREFIX))
            {
                if let Some(parsed_value) = parse_json_db(&krx_msg, &fields, FIELD_INDEX) {
                    parsed_values.push((krx_msg.instcode.clone(), parsed_value));
                }
            }
        }

        // 결과 검증
        assert!(!parsed_values.is_empty(), "No values were parsed");
        
        // 결과 출력 (디버깅용)
        for (instcode, value) in parsed_values {
            println!("InstCode: {:?}", instcode);
            match value {
                ParsedValue::Double(v) => println!("Value: {:.2}", v),
                ParsedValue::Integer(v) => println!("Value: {}", v),
                ParsedValue::Text(v) => println!("Value: {}", v),
            }
        }

        Ok(())
    }


}