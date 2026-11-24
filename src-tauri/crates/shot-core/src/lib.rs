//! 主进程和截屏 sidecar 共用的协议。两边都依赖这个 crate，避免各写一份消息。

pub mod grab;
mod shortcut;

pub use grab::{crop_bgra, encode_png, Frame};

use std::io::{Read, Write};

pub use shortcut::{Hotkey, parse_shortcut};

pub const PROTOCOL_VERSION: u32 = 1;
pub const PIPE_NAME: &str = r"\\.\pipe\daodian-shot-v1";
pub const MUTEX_NAME: &str = r"Global\daodian-shot-v1";
pub const MUTEX_NAME_LOCAL: &str = r"Local\daodian-shot-v1";
/// 管道里只走元数据。大图写临时文件，不进这一帧。
pub const MAX_FRAME_BYTES: usize = 1024 * 1024;

pub const CODE_VERSION: &str = "version_mismatch";
pub const CODE_HOTKEY: &str = "hotkey";
pub const CODE_UNSUPPORTED: &str = "unsupported";
pub const CODE_BAD_MESSAGE: &str = "bad_message";
pub const CODE_CLOSED: &str = "closed";
pub const CODE_CAPTURE: &str = "capture";

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct ShotConfig {
    pub enabled: bool,
    pub shortcut: String,
    pub save_dir: String,
    pub capture_mode: String,
    pub gif_fps: u32,
}

impl Default for ShotConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            shortcut: String::new(),
            save_dir: String::new(),
            capture_mode: "freeze".into(),
            gif_fps: 10,
        }
    }
}

/// 长度前缀帧里的一条消息。`id` 用来把应答配回命令；没有应答的事件不带 `id`。
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Message {
    Hello {
        id: u64,
        version: u32,
    },
    HelloOk {
        id: u64,
        version: u32,
    },
    ReloadConfig {
        id: u64,
        config: ShotConfig,
    },
    Capture {
        id: u64,
        session: u64,
    },
    Longshot {
        id: u64,
        session: u64,
    },
    Gif {
        id: u64,
        session: u64,
    },
    Cancel {
        id: u64,
        session: u64,
    },
    Pin {
        id: u64,
        session: u64,
        path: String,
    },
    Ack {
        id: u64,
        session: u64,
    },
    Ping {
        id: u64,
    },
    Shutdown {
        id: u64,
    },
    Ready {
        id: u64,
        version: u32,
        hotkey: bool,
    },
    State {
        session: u64,
        phase: String,
    },
    Progress {
        session: u64,
        stage: String,
        percent: u8,
    },
    Result {
        id: u64,
        session: u64,
        path: String,
        width: u32,
        height: u32,
        bytes: u64,
        mode: String,
    },
    Error {
        id: u64,
        session: u64,
        code: String,
        message: String,
    },
    Hotkey {
        session: u64,
    },
    Pong {
        id: u64,
    },
    /// 截图进程把取到的文字交给主进程。文字不进日志。
    Text {
        id: u64,
        kind: String,
        text: String,
    },
    TextOk {
        id: u64,
        message: String,
    },
    Ok {
        id: u64,
    },
    Exit {
        reason: String,
    },
}

impl Message {
    pub fn id(&self) -> Option<u64> {
        Some(match self {
            Self::Hello { id, .. }
            | Self::HelloOk { id, .. }
            | Self::ReloadConfig { id, .. }
            | Self::Capture { id, .. }
            | Self::Longshot { id, .. }
            | Self::Gif { id, .. }
            | Self::Cancel { id, .. }
            | Self::Pin { id, .. }
            | Self::Ack { id, .. }
            | Self::Ping { id }
            | Self::Shutdown { id }
            | Self::Ready { id, .. }
            | Self::Result { id, .. }
            | Self::Error { id, .. }
            | Self::Pong { id }
            | Self::Text { id, .. }
            | Self::TextOk { id, .. }
            | Self::Ok { id } => *id,
            Self::State { .. } | Self::Progress { .. } | Self::Hotkey { .. } | Self::Exit { .. } => {
                return None;
            }
        })
    }
}

#[derive(Debug)]
pub enum FrameError {
    Closed,
    TooLarge(usize),
    Json(String),
    Io(std::io::Error),
}

impl std::fmt::Display for FrameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Closed => write!(f, "管道已断开"),
            Self::TooLarge(len) => write!(f, "消息过长: {len}"),
            Self::Json(err) => write!(f, "消息无法解析: {err}"),
            Self::Io(err) => write!(f, "管道读写失败: {err}"),
        }
    }
}

impl std::error::Error for FrameError {}

impl From<std::io::Error> for FrameError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}

pub fn encode(message: &Message) -> Result<Vec<u8>, FrameError> {
    let body = serde_json::to_vec(message).map_err(|err| FrameError::Json(err.to_string()))?;
    if body.len() > MAX_FRAME_BYTES {
        return Err(FrameError::TooLarge(body.len()));
    }
    let mut frame = Vec::with_capacity(4 + body.len());
    frame.extend_from_slice(&(body.len() as u32).to_le_bytes());
    frame.extend_from_slice(&body);
    Ok(frame)
}

#[derive(Debug, Default)]
pub struct Decoder {
    buf: Vec<u8>,
}

impl Decoder {
    pub fn push(&mut self, bytes: &[u8]) -> Result<(), FrameError> {
        if self.buf.len().saturating_add(bytes.len()) > MAX_FRAME_BYTES + 4 {
            return Err(FrameError::TooLarge(self.buf.len() + bytes.len()));
        }
        self.buf.extend_from_slice(bytes);
        Ok(())
    }

    pub fn pop(&mut self) -> Result<Option<Message>, FrameError> {
        if self.buf.len() < 4 {
            return Ok(None);
        }
        let len = u32::from_le_bytes(self.buf[..4].try_into().expect("4 字节")) as usize;
        if len == 0 || len > MAX_FRAME_BYTES {
            return Err(FrameError::TooLarge(len));
        }
        if self.buf.len() < 4 + len {
            return Ok(None);
        }
        let body = self.buf[4..4 + len].to_vec();
        self.buf.drain(..4 + len);
        let message = serde_json::from_slice(&body).map_err(|err| FrameError::Json(err.to_string()))?;
        Ok(Some(message))
    }
}

pub fn write_frame(writer: &mut impl Write, message: &Message) -> Result<(), FrameError> {
    let frame = encode(message)?;
    writer.write_all(&frame)?;
    writer.flush()?;
    Ok(())
}

pub fn read_frame(reader: &mut impl Read, decoder: &mut Decoder) -> Result<Message, FrameError> {
    let mut chunk = [0u8; 8192];
    loop {
        if let Some(message) = decoder.pop()? {
            return Ok(message);
        }
        let read = reader.read(&mut chunk)?;
        if read == 0 {
            return Err(FrameError::Closed);
        }
        decoder.push(&chunk[..read])?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn roundtrip(message: &Message) {
        let frame = encode(message).expect("编码");
        let mut decoder = Decoder::default();
        decoder.push(&frame).unwrap();
        assert_eq!(decoder.pop().unwrap().as_ref(), Some(message));
        assert!(decoder.pop().unwrap().is_none());
    }

    #[test]
    fn frames_roundtrip_and_split_across_reads() {
        let first = Message::Hello {
            id: 7,
            version: PROTOCOL_VERSION,
        };
        let second = Message::ReloadConfig {
            id: 8,
            config: ShotConfig {
                enabled: true,
                shortcut: "Ctrl+1".into(),
                save_dir: String::new(),
                capture_mode: "freeze".into(),
                gif_fps: 10,
            },
        };
        let mut bytes = encode(&first).unwrap();
        bytes.extend(encode(&second).unwrap());
        let mut cursor = Cursor::new(bytes);
        let mut decoder = Decoder::default();
        {
            let mut limited = (&mut cursor).take(3);
            assert!(matches!(
                read_frame(&mut limited, &mut decoder),
                Err(FrameError::Closed)
            ));
        }
        assert_eq!(read_frame(&mut cursor, &mut decoder).unwrap(), first);
        assert_eq!(read_frame(&mut cursor, &mut decoder).unwrap(), second);
    }

    #[test]
    fn rejects_a_frame_longer_than_metadata() {
        let mut decoder = Decoder::default();
        let huge = (MAX_FRAME_BYTES as u32 + 1).to_le_bytes();
        decoder.push(&huge).unwrap();
        assert!(matches!(decoder.pop(), Err(FrameError::TooLarge(_))));
    }

    #[test]
    fn missing_config_fields_use_defaults() {
        let config: ShotConfig = serde_json::from_str(r#"{"enabled":true}"#).unwrap();
        assert!(config.enabled);
        assert!(config.shortcut.is_empty());
        assert_eq!(config.gif_fps, 10);
        assert_eq!(config.capture_mode, "freeze");
    }

    #[test]
    fn command_ids_stay_on_replies() {
        roundtrip(&Message::Pong { id: 3 });
        roundtrip(&Message::Error {
            id: 4,
            session: 9,
            code: CODE_HOTKEY.into(),
            message: "占用".into(),
        });
        roundtrip(&Message::Hotkey { session: 2 });
        assert_eq!(Message::Hotkey { session: 2 }.id(), None);
        assert_eq!(Message::Ping { id: 5 }.id(), Some(5));
    }
}
