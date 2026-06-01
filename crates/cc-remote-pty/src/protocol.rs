use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum PtyMessage {
    #[serde(rename = "input")]
    Input { data: String },
    #[serde(rename = "output")]
    Output { data: String },
    #[serde(rename = "resize")]
    Resize { cols: u16, rows: u16 },
    #[serde(rename = "state")]
    State { waiting: bool },
    #[serde(rename = "exit")]
    Exit { code: i32 },
}

pub fn encode(msg: &PtyMessage) -> String {
    let mut s = serde_json::to_string(msg).expect("serialize PtyMessage");
    s.push('\n');
    s
}

pub fn decode(line: &str) -> Option<PtyMessage> {
    serde_json::from_str(line.trim()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_input() {
        let msg = PtyMessage::Input { data: "hello\n".into() };
        let encoded = encode(&msg);
        let decoded = decode(&encoded).unwrap();
        match decoded {
            PtyMessage::Input { data } => assert_eq!(data, "hello\n"),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn roundtrip_state() {
        let msg = PtyMessage::State { waiting: true };
        let encoded = encode(&msg);
        let decoded = decode(&encoded).unwrap();
        match decoded {
            PtyMessage::State { waiting } => assert!(waiting),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn decode_invalid_returns_none() {
        assert!(decode("not json").is_none());
    }
}
