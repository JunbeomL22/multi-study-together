pub mod range_helper;

use mongodb::bson::{Binary, spec::BinarySubtype};
use std::{fmt, str};
use serde::{Deserialize, Serialize};
use encoding_rs::EUC_KR;
use crate::UnixNano;
use crate::mongodb_collection::krx_msg::range_helper::{
    krx_messages_instcode_range,
    krx_message_dist_index_range,
};

/// # Arguments
/// * `date` - yyyymmdd
/// * `trcode` - 5 bytes (first two bytes are data type, last three bytes are asset code, e.g., B606F)
/// * `instcode` - 12 bytes (e.g., KR4165N30007)
/// * `dist_index` - distribution index, the order of the message regarding the same trcode.
/// * `packet_timestamp` - UnixNano (the time when the packet is received)
/// * `timestamp` - UnixNano (the time when the message is received on the processor)
/// * `payload` - binary data
/// 
/// 
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct KrxMsg {
    pub date: i32,
    pub trcode: String,
    pub distidx: Option<i32>,
    pub instcode: Option<String>,
    pub packet_timestamp: Option<UnixNano>,
    pub timestamp: Option<UnixNano>,
    #[serde(with = "binary_serde")]
    pub payload: Vec<u8>,
}

impl fmt::Display for KrxMsg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let date_str = format!(
            "{:04}-{:02}-{:02}",
            self.date / 10000,
            (self.date % 10000) / 100,
            self.date % 100
        );

        let payload_len = self.payload.len();

        // 1. EUC-KR/CP949로 디코딩 시도
        let (cow, _, _) = EUC_KR.decode(&self.payload[..payload_len.saturating_sub(1)]);
        let payload_str = cow.into_owned();

        write!(
            f,
            "KrxMsg {{\n  date: {}\n  trcode: {}\n  distidx: {}\n instcode: {}\n  packet_timestamp: {}\n  timestamp: {}\n  payload: {} ({} bytes)\n}}",
            date_str,
            self.trcode,
            self.distidx.map_or("None".to_string(), |idx| idx.to_string()),
            self.instcode.as_deref().unwrap_or("None"),
            self.packet_timestamp.map_or("None".to_string(), |ts| ts.to_string()),
            self.timestamp.map_or("None".to_string(), |ts| ts.to_string()),
            payload_str,
            payload_len,
        )
    }
}

impl KrxMsg {
    pub fn new_from_payload(
        date: i32, 
        payload: &[u8], 
        packet_timestamp: Option<UnixNano>,
        timestamp: Option<UnixNano>,
    ) -> Result<Self, std::string::FromUtf8Error> {
        let trcode = if let Ok(trcode) = String::from_utf8(payload[..5].to_vec()) {
            trcode
        } else {
            let pay_clone = payload.to_vec();
            flashlog::flash_info!("DECODE";"Failed to decode trcode"; payload = pay_clone);
            "".to_string()
        };
        let instcode_range = krx_messages_instcode_range(payload);
        let instcode = match instcode_range {
            Some(range) => {
                if payload.len() < range.end {
                    None
                } else {
                    match String::from_utf8(payload[range.start..range.end].to_vec()) {
                        Ok(instcode) => Some(instcode),
                        Err(_) => {
                            let pay_clone = payload.to_vec();
                            flashlog::flash_info!("DECODE";"Failed to decode instcode"; payload = pay_clone);
                            None
                        }
                    }
                }
            },
            None => None,
        };

        let distidx_range = krx_message_dist_index_range(payload);
        let distidx: Option<i32> = match distidx_range {
            Some(range) => {
                if payload.len() < range.end {
                    None
                } else {
                    let clipped = &payload[range.start..range.end];
                    let res = match String::from_utf8_lossy(clipped).parse::<i32>() {
                        Ok(distidx) => Some(distidx),
                        Err(_) => {
                            None
                            /* there are quite a few messages that are not numbers
                            // if all whitespace, return None
                            if clipped.iter().all(|&c| c == 0x20) {
                                None
                            } else {
                                let pay_clone = encoding_rs::EUC_KR.decode(payload).0.to_string();
                                flashlog::flash_info!("DECODE";"{}: Failed to parse distidx", e; payload = pay_clone);
                                None
                            }
                            */
                        }
                    };
                    res
                }
            },
            None => None,
        };

        Ok(Self {
            date,
            trcode,
            distidx,
            instcode,
            packet_timestamp,
            timestamp,
            payload: payload.to_vec(),
        })
    }
}
mod binary_serde {
    use super::*;
    use serde::{Deserializer, Serializer};

    pub fn serialize<S>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        // Generic/User-defined 바이너리 서브타입 사용
        Binary {
            subtype: BinarySubtype::Generic,
            bytes: bytes.to_vec(),
        }.serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
    where
        D: Deserializer<'de>,
    {
        Binary::deserialize(deserializer).map(|binary| binary.bytes)
    }
}

#[cfg(test)]
mod test {
    //i want data that payload in ../data/multiasset_db.krx_msg.json include "KR4167"
    use super::*;
    use struson::reader::{JsonStreamReader, JsonReader};

    #[test]
    fn test_3yr_ktbf() -> anyhow::Result<()> {
        // read from multiasset_db.krx_msg.json
        let file_name = "data/krx_msg.json";
        let file_path = format!("{}", file_name);
        let file = std::fs::File::open(file_path).unwrap();
        let reader = std::io::BufReader::new(file);
        /* non-streaming 
        let krx_msgs: Vec<KrxMsg> = serde_json::from_reader(reader).unwrap();
        for krx_msg in krx_msgs {
            println!("{}", krx_msg);
        }
        */
        // streaming
        let mut stream_reader = JsonStreamReader::new(reader);
        stream_reader.begin_array()?;
        
        while stream_reader.has_next()? {
            let krx_msg: KrxMsg = stream_reader.deserialize_next()?;
            if krx_msg.instcode.is_some() && krx_msg.instcode.as_ref().unwrap().contains("KR4165") {
                println!("{}", krx_msg);
            }
        }

        stream_reader.end_array()?;
        
        Ok(())

    }

    #[test]
    fn test_10yr_ktbf() -> anyhow::Result<()> {
        // read from multiasset_db.krx_msg.json
        let file_name = "data/krx_msg.json";
        let file_path = format!("{}", file_name);
        let file = std::fs::File::open(file_path).unwrap();
        let reader = std::io::BufReader::new(file);
        /* non-streaming 
        let krx_msgs: Vec<KrxMsg> = serde_json::from_reader(reader).unwrap();
        for krx_msg in krx_msgs {
            println!("{}", krx_msg);
        }
        */
        // streaming
        let mut stream_reader = JsonStreamReader::new(reader);
        stream_reader.begin_array()?;
        
        while stream_reader.has_next()? {
            let krx_msg: KrxMsg = stream_reader.deserialize_next()?;
            if krx_msg.instcode.is_some() && krx_msg.instcode.as_ref().unwrap().contains("KR4167") {
                println!("{}", krx_msg);
            }
        }

        stream_reader.end_array()?;
        
        Ok(())

    }

    #[test]
    fn test_30yr_ktbf() -> anyhow::Result<()> {
        // read from multiasset_db.krx_msg.json
        let file_name = "data/krx_msg.json";
        let file_path = format!("{}", file_name);
        let file = std::fs::File::open(file_path).unwrap();
        let reader = std::io::BufReader::new(file);
        /* non-streaming 
        let krx_msgs: Vec<KrxMsg> = serde_json::from_reader(reader).unwrap();
        for krx_msg in krx_msgs {
            println!("{}", krx_msg);
        }
        */
        // streaming
        let mut stream_reader = JsonStreamReader::new(reader);
        stream_reader.begin_array()?;
        
        while stream_reader.has_next()? {
            let krx_msg: KrxMsg = stream_reader.deserialize_next()?;
            if krx_msg.instcode.is_some() && krx_msg.instcode.as_ref().unwrap().contains("KR4170") {
                println!("{}", krx_msg);
            }
        }

        stream_reader.end_array()?;
        
        Ok(())

    }
}