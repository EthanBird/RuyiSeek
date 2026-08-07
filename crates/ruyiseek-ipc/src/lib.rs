//! Versioned local IPC messages and bounded length-prefixed framing.

use ruyiseek_core::{ItemKind, SearchHit, SearchItem};
use std::fmt;
use std::io::{self, Read, Write};
#[cfg(unix)]
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const PROTOCOL_VERSION: u16 = 1;
pub const MAX_FRAME_SIZE: usize = 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Request {
    Ping,
    Status,
    Shutdown,
    Search { query: String, limit: usize },
}

#[derive(Clone, Debug, PartialEq)]
pub enum Response {
    Acknowledged,
    Pong {
        protocol_version: u16,
    },
    Status(DaemonStatus),
    Search {
        elapsed_us: u64,
        hits: Vec<SearchHit>,
    },
    Error(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DaemonStatus {
    pub indexed_items: usize,
    pub skipped_paths: usize,
    pub truncated: bool,
}

#[derive(Debug)]
pub enum ProtocolError {
    EmptyMessage,
    InvalidMessage(&'static str),
    InvalidNumber,
    InvalidEscape,
    InvalidUtf8,
    FrameTooLarge(usize),
    Io(io::Error),
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyMessage => formatter.write_str("empty IPC message"),
            Self::InvalidMessage(reason) => write!(formatter, "invalid IPC message: {reason}"),
            Self::InvalidNumber => formatter.write_str("invalid numeric IPC field"),
            Self::InvalidEscape => formatter.write_str("invalid percent-encoded IPC field"),
            Self::InvalidUtf8 => formatter.write_str("IPC text field is not valid UTF-8"),
            Self::FrameTooLarge(size) => write!(formatter, "IPC frame is too large: {size} bytes"),
            Self::Io(error) => write!(formatter, "IPC I/O error: {error}"),
        }
    }
}

impl std::error::Error for ProtocolError {}

impl From<io::Error> for ProtocolError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

#[derive(Debug)]
pub enum ClientError {
    Connect { socket: PathBuf, source: io::Error },
    Protocol(ProtocolError),
}

impl fmt::Display for ClientError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Connect { socket, source } => {
                write!(
                    formatter,
                    "cannot connect to {}: {source}",
                    socket.display()
                )
            }
            Self::Protocol(source) => source.fmt(formatter),
        }
    }
}

impl std::error::Error for ClientError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Connect { source, .. } => Some(source),
            Self::Protocol(source) => Some(source),
        }
    }
}

impl From<ProtocolError> for ClientError {
    fn from(source: ProtocolError) -> Self {
        Self::Protocol(source)
    }
}

impl From<io::Error> for ClientError {
    fn from(source: io::Error) -> Self {
        Self::Protocol(source.into())
    }
}

/// Send one request to a local daemon socket and decode its response.
///
/// # Errors
///
/// Returns [`ClientError`] when the socket cannot be reached or the bounded protocol exchange
/// fails.
#[cfg(unix)]
pub fn request_daemon(
    socket: &Path,
    request: &Request,
    timeout: Duration,
) -> Result<Response, ClientError> {
    let mut stream = UnixStream::connect(socket).map_err(|source| ClientError::Connect {
        socket: socket.to_path_buf(),
        source,
    })?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    write_frame(&mut stream, &encode_request(request))?;
    Ok(decode_response(&read_frame(&mut stream)?)?)
}

#[must_use]
pub fn encode_request(request: &Request) -> Vec<u8> {
    match request {
        Request::Ping => b"PING".to_vec(),
        Request::Status => b"STATUS".to_vec(),
        Request::Shutdown => b"SHUTDOWN".to_vec(),
        Request::Search { query, limit } => {
            format!("SEARCH\t{limit}\t{}", encode_bytes(query.as_bytes())).into_bytes()
        }
    }
}

/// Decode one request payload.
///
/// # Errors
///
/// Returns [`ProtocolError`] when the payload is malformed or exceeds a protocol bound.
pub fn decode_request(bytes: &[u8]) -> Result<Request, ProtocolError> {
    let message = std::str::from_utf8(bytes).map_err(|_| ProtocolError::InvalidUtf8)?;
    let mut fields = message.split('\t');
    match fields.next().ok_or(ProtocolError::EmptyMessage)? {
        "PING" if fields.next().is_none() => Ok(Request::Ping),
        "STATUS" if fields.next().is_none() => Ok(Request::Status),
        "SHUTDOWN" if fields.next().is_none() => Ok(Request::Shutdown),
        "SEARCH" => {
            let limit = parse(fields.next())?;
            if limit > 1_000 {
                return Err(ProtocolError::InvalidMessage("search limit exceeds 1000"));
            }
            let query = decode_string(fields.next())?;
            if fields.next().is_some() {
                return Err(ProtocolError::InvalidMessage("too many SEARCH fields"));
            }
            Ok(Request::Search { query, limit })
        }
        _ => Err(ProtocolError::InvalidMessage("unknown request")),
    }
}

#[must_use]
pub fn encode_response(response: &Response) -> Vec<u8> {
    match response {
        Response::Acknowledged => b"ACK".to_vec(),
        Response::Pong { protocol_version } => format!("PONG\t{protocol_version}").into_bytes(),
        Response::Status(status) => format!(
            "STATUS\t{}\t{}\t{}",
            status.indexed_items,
            status.skipped_paths,
            u8::from(status.truncated)
        )
        .into_bytes(),
        Response::Error(message) => {
            format!("ERROR\t{}", encode_bytes(message.as_bytes())).into_bytes()
        }
        Response::Search { elapsed_us, hits } => {
            let mut output = format!("RESULTS\t{elapsed_us}\t{}", hits.len());
            for hit in hits {
                output.push('\n');
                output.push_str(&format!(
                    "{}\t{:.6}\t{}\t{}\t{}\t{}",
                    hit.item.id,
                    hit.score,
                    hit.item.kind.protocol_name(),
                    u8::from(hit.item.hidden),
                    encode_bytes(hit.item.name.as_bytes()),
                    encode_path(&hit.item.path)
                ));
            }
            output.into_bytes()
        }
    }
}

/// Decode one response payload.
///
/// # Errors
///
/// Returns [`ProtocolError`] when the payload is malformed or internally inconsistent.
pub fn decode_response(bytes: &[u8]) -> Result<Response, ProtocolError> {
    let message = std::str::from_utf8(bytes).map_err(|_| ProtocolError::InvalidUtf8)?;
    let mut lines = message.lines();
    let header = lines.next().ok_or(ProtocolError::EmptyMessage)?;
    let mut fields = header.split('\t');
    match fields.next().ok_or(ProtocolError::EmptyMessage)? {
        "ACK" => {
            ensure_end(&mut fields)?;
            Ok(Response::Acknowledged)
        }
        "PONG" => Ok(Response::Pong {
            protocol_version: parse_exactly_one(fields)?,
        }),
        "STATUS" => {
            let indexed_items = parse(fields.next())?;
            let skipped_paths = parse(fields.next())?;
            let truncated = parse_bool(fields.next())?;
            ensure_end(&mut fields)?;
            Ok(Response::Status(DaemonStatus {
                indexed_items,
                skipped_paths,
                truncated,
            }))
        }
        "ERROR" => {
            let error = decode_string(fields.next())?;
            ensure_end(&mut fields)?;
            Ok(Response::Error(error))
        }
        "RESULTS" => {
            let elapsed_us = parse(fields.next())?;
            let count: usize = parse(fields.next())?;
            ensure_end(&mut fields)?;
            let hits = lines.map(decode_hit).collect::<Result<Vec<_>, _>>()?;
            if hits.len() != count {
                return Err(ProtocolError::InvalidMessage("result count mismatch"));
            }
            Ok(Response::Search { elapsed_us, hits })
        }
        _ => Err(ProtocolError::InvalidMessage("unknown response")),
    }
}

/// Write one bounded, length-prefixed frame.
///
/// # Errors
///
/// Returns [`ProtocolError`] if the payload exceeds [`MAX_FRAME_SIZE`] or writing fails.
pub fn write_frame(writer: &mut impl Write, payload: &[u8]) -> Result<(), ProtocolError> {
    if payload.len() > MAX_FRAME_SIZE {
        return Err(ProtocolError::FrameTooLarge(payload.len()));
    }
    let size =
        u32::try_from(payload.len()).map_err(|_| ProtocolError::FrameTooLarge(payload.len()))?;
    writer.write_all(&size.to_be_bytes())?;
    writer.write_all(payload)?;
    writer.flush()?;
    Ok(())
}

/// Read one bounded, length-prefixed frame.
///
/// # Errors
///
/// Returns [`ProtocolError`] if the announced size exceeds [`MAX_FRAME_SIZE`] or reading fails.
pub fn read_frame(reader: &mut impl Read) -> Result<Vec<u8>, ProtocolError> {
    let mut length_bytes = [0_u8; 4];
    reader.read_exact(&mut length_bytes)?;
    let size = u32::from_be_bytes(length_bytes) as usize;
    if size > MAX_FRAME_SIZE {
        return Err(ProtocolError::FrameTooLarge(size));
    }
    let mut payload = vec![0; size];
    reader.read_exact(&mut payload)?;
    Ok(payload)
}

#[must_use]
pub fn default_socket_path() -> PathBuf {
    if let Some(directory) = std::env::var_os("XDG_RUNTIME_DIR").filter(|value| !value.is_empty()) {
        return PathBuf::from(directory).join("ruyiseek/daemon.sock");
    }

    let identity = std::env::var("UID")
        .ok()
        .filter(|value| value.chars().all(|character| character.is_ascii_digit()))
        .or_else(|| {
            std::env::var("USER").ok().map(|value| {
                value
                    .chars()
                    .filter(|character| {
                        character.is_ascii_alphanumeric() || matches!(character, '-' | '_')
                    })
                    .collect()
            })
        })
        .filter(|value: &String| !value.is_empty())
        .unwrap_or_else(|| "unknown".to_owned());
    PathBuf::from(format!("/tmp/ruyiseek-{identity}/daemon.sock"))
}

fn decode_hit(line: &str) -> Result<SearchHit, ProtocolError> {
    let mut fields = line.split('\t');
    let id = parse(fields.next())?;
    let score = parse(fields.next())?;
    let kind = ItemKind::from_protocol_name(
        fields
            .next()
            .ok_or(ProtocolError::InvalidMessage("missing result kind"))?,
    )
    .ok_or(ProtocolError::InvalidMessage("unknown result kind"))?;
    let hidden = parse_bool(fields.next())?;
    let name = decode_string(fields.next())?;
    let path = decode_path(fields.next())?;
    ensure_end(&mut fields)?;

    Ok(SearchHit {
        item: SearchItem {
            id,
            name,
            path,
            kind,
            hidden,
        },
        score,
    })
}

fn parse<T: std::str::FromStr>(value: Option<&str>) -> Result<T, ProtocolError> {
    value
        .ok_or(ProtocolError::InvalidMessage("missing field"))?
        .parse()
        .map_err(|_| ProtocolError::InvalidNumber)
}

fn parse_exactly_one<T: std::str::FromStr>(
    mut fields: std::str::Split<'_, char>,
) -> Result<T, ProtocolError> {
    let value = parse(fields.next())?;
    ensure_end(&mut fields)?;
    Ok(value)
}

fn parse_bool(value: Option<&str>) -> Result<bool, ProtocolError> {
    match value {
        Some("0") => Ok(false),
        Some("1") => Ok(true),
        _ => Err(ProtocolError::InvalidMessage("invalid boolean field")),
    }
}

fn ensure_end(fields: &mut std::str::Split<'_, char>) -> Result<(), ProtocolError> {
    if fields.next().is_none() {
        Ok(())
    } else {
        Err(ProtocolError::InvalidMessage("too many fields"))
    }
}

fn encode_bytes(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len());
    for byte in bytes {
        if byte.is_ascii_alphanumeric() || matches!(*byte, b'-' | b'_' | b'.' | b'/' | b'~' | b' ')
        {
            output.push(char::from(*byte));
        } else {
            output.push('%');
            output.push(hex(byte >> 4));
            output.push(hex(byte & 0x0f));
        }
    }
    output
}

fn decode_bytes(value: &str) -> Result<Vec<u8>, ProtocolError> {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len() {
                return Err(ProtocolError::InvalidEscape);
            }
            output.push((unhex(bytes[index + 1])? << 4) | unhex(bytes[index + 2])?);
            index += 3;
        } else {
            output.push(bytes[index]);
            index += 1;
        }
    }
    Ok(output)
}

fn decode_string(value: Option<&str>) -> Result<String, ProtocolError> {
    String::from_utf8(decode_bytes(
        value.ok_or(ProtocolError::InvalidMessage("missing text field"))?,
    )?)
    .map_err(|_| ProtocolError::InvalidUtf8)
}

#[cfg(unix)]
fn encode_path(path: &Path) -> String {
    use std::os::unix::ffi::OsStrExt;
    encode_bytes(path.as_os_str().as_bytes())
}

#[cfg(not(unix))]
fn encode_path(path: &Path) -> String {
    encode_bytes(path.to_string_lossy().as_bytes())
}

#[cfg(unix)]
fn decode_path(value: Option<&str>) -> Result<PathBuf, ProtocolError> {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;
    Ok(PathBuf::from(OsString::from_vec(decode_bytes(
        value.ok_or(ProtocolError::InvalidMessage("missing path field"))?,
    )?)))
}

#[cfg(not(unix))]
fn decode_path(value: Option<&str>) -> Result<PathBuf, ProtocolError> {
    Ok(PathBuf::from(decode_string(value)?))
}

const fn hex(value: u8) -> char {
    match value {
        0..=9 => (b'0' + value) as char,
        _ => (b'A' + value - 10) as char,
    }
}

const fn unhex(value: u8) -> Result<u8, ProtocolError> {
    match value {
        b'0'..=b'9' => Ok(value - b'0'),
        b'a'..=b'f' => Ok(value - b'a' + 10),
        b'A'..=b'F' => Ok(value - b'A' + 10),
        _ => Err(ProtocolError::InvalidEscape),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn request_round_trip_preserves_unicode_and_tabs() {
        let request = Request::Search {
            query: "年度\t报告 100%".to_owned(),
            limit: 25,
        };
        assert_eq!(
            decode_request(&encode_request(&request)).expect("decode request"),
            request
        );
    }

    #[test]
    fn shutdown_and_acknowledgement_round_trip() {
        assert_eq!(
            decode_request(&encode_request(&Request::Shutdown)).expect("decode shutdown"),
            Request::Shutdown
        );
        assert_eq!(
            decode_response(&encode_response(&Response::Acknowledged))
                .expect("decode acknowledgement"),
            Response::Acknowledged
        );
    }

    #[test]
    fn response_round_trip_preserves_hits() {
        let response = Response::Search {
            elapsed_us: 42,
            hits: vec![SearchHit {
                item: SearchItem {
                    id: 7,
                    name: "设计\n文档.md".to_owned(),
                    path: PathBuf::from("/工作/设计\n文档.md"),
                    kind: ItemKind::File,
                    hidden: false,
                },
                score: 0.875,
            }],
        };
        assert_eq!(
            decode_response(&encode_response(&response)).expect("decode response"),
            response
        );
    }

    #[test]
    fn frame_round_trip_and_limit() {
        let mut bytes = Vec::new();
        write_frame(&mut bytes, b"hello").expect("write frame");
        assert_eq!(
            read_frame(&mut Cursor::new(bytes)).expect("read frame"),
            b"hello"
        );

        let error = write_frame(&mut Vec::new(), &vec![0; MAX_FRAME_SIZE + 1])
            .expect_err("oversized frame must fail");
        assert!(matches!(error, ProtocolError::FrameTooLarge(_)));
    }

    #[test]
    fn rejects_excessive_search_limit() {
        assert!(matches!(
            decode_request(b"SEARCH\t1001\tquery"),
            Err(ProtocolError::InvalidMessage(_))
        ));
    }
}
