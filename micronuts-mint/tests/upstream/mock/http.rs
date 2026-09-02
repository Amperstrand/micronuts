//! One-request-per-connection HTTP/1.1 framing for the mock upstream
//! (std-only; just enough for ureq with `Connection: close`).

use std::io::{Read, Write};
use std::net::TcpStream;

pub(super) struct Request {
    pub(super) method: String,
    pub(super) path: String,
    pub(super) body: Option<String>,
}

pub(super) fn read_request(stream: &mut TcpStream) -> Request {
    let mut buf: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 8192];
    let header_end = loop {
        if let Some(pos) = find_subslice(&buf, b"\r\n\r\n") {
            break pos;
        }
        let n = stream.read(&mut chunk).expect("mock read");
        if n == 0 {
            panic!("mock: connection closed before headers completed");
        }
        buf.extend_from_slice(&chunk[..n]);
    };

    let head = String::from_utf8_lossy(&buf[..header_end]).to_string();
    let mut lines = head.lines();
    let request_line = lines.next().expect("mock request line");
    let mut parts = request_line.split_whitespace();
    let method = parts.next().expect("mock method").to_string();
    let path = parts.next().expect("mock path").to_string();
    let content_length: usize = lines
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse().ok())?
        })
        .unwrap_or(0);

    while buf.len() < header_end + 4 + content_length {
        let n = stream.read(&mut chunk).expect("mock body read");
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    let body = if content_length > 0 {
        Some(
            String::from_utf8_lossy(&buf[header_end + 4..header_end + 4 + content_length])
                .to_string(),
        )
    } else {
        None
    };
    Request { method, path, body }
}

pub(super) fn write_response(stream: &mut TcpStream, status: &str, body: &str) {
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(response.as_bytes()).expect("mock write");
    let _ = stream.flush();
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}
