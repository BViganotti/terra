//! HTTP hardening acceptance through raw sockets.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

use terra_studio::server::{start, ServerConfig};

fn server() -> SocketAddr {
    start(ServerConfig {
        port: 0,
        token: "limits-token".into(),
    })
    .expect("server")
    .addr
}

fn send_raw(addr: SocketAddr, raw: &[u8]) -> u16 {
    let mut stream = TcpStream::connect(addr).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    stream.write_all(raw).unwrap();
    let mut buf = Vec::new();
    let mut chunk = [0u8; 2048];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                buf.extend_from_slice(&chunk[..n]);
                if buf.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            Err(_) => break,
        }
    }
    let head = String::from_utf8_lossy(&buf);
    head.lines()
        .next()
        .and_then(|l| l.split(' ').nth(1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

fn get(addr: SocketAddr, path: &str) -> u16 {
    let raw = format!("GET {path} HTTP/1.1\r\nHost: x\r\n\r\n");
    send_raw(addr, raw.as_bytes())
}

#[test]
fn methods_and_malformed_requests_are_rejected() {
    let addr = server();
    assert_eq!(
        send_raw(
            addr,
            b"PUT /api/health?t=limits-token HTTP/1.1\r\nHost: x\r\n\r\n"
        ),
        405
    );
    assert_eq!(send_raw(addr, b"DELETE / HTTP/1.1\r\nHost: x\r\n\r\n"), 405);
    assert_eq!(send_raw(addr, b"garbage\r\n\r\n"), 400);
    assert_eq!(
        send_raw(addr, b"GET / HTTP/1.1\r\nBrokenHeader\r\n\r\n"),
        400
    );
    assert_eq!(get(addr, "/api/health"), 401);
    assert_eq!(get(addr, "/api/health?t=nope"), 401);
    assert_eq!(get(addr, "/api/health?t=limits-token"), 200);
}

#[test]
fn url_and_body_limits_are_enforced() {
    let addr = server();
    let long_path = format!("/{}", "a".repeat(600));
    assert_eq!(get(addr, &long_path), 414);
    let request =
        "POST /api/run?t=limits-token HTTP/1.1\r\nHost: x\r\nContent-Length: 999999\r\n\r\n";
    assert_eq!(send_raw(addr, request.as_bytes()), 413);
    assert_eq!(
        send_raw(
            addr,
            b"POST /api/run?t=limits-token HTTP/1.1\r\nHost: x\r\nTransfer-Encoding: chunked\r\n\r\n"
        ),
        400
    );
    // The server must still answer after rejecting the above.
    assert_eq!(get(addr, "/api/health?t=limits-token"), 200);
}

#[test]
fn header_size_and_count_are_bounded() {
    let addr = server();
    let mut request = String::from("GET /api/health?t=limits-token HTTP/1.1\r\nHost: x\r\n");
    for i in 0..70 {
        request.push_str(&format!("X-Pad-{i}: value\r\n"));
    }
    request.push_str("\r\n");
    assert_eq!(send_raw(addr, request.as_bytes()), 400);

    let mut huge = String::from("GET /api/health?t=limits-token HTTP/1.1\r\nHost: x\r\n");
    huge.push_str("X-Huge: ");
    huge.push_str(&"p".repeat(20_000));
    huge.push_str("\r\n\r\n");
    assert_eq!(send_raw(addr, huge.as_bytes()), 431);
    assert_eq!(get(addr, "/api/health?t=limits-token"), 200);
}

#[test]
fn traversal_and_percent_encoding_are_rejected() {
    let addr = server();
    for path in [
        "/../etc/passwd?t=limits-token",
        "/%2e%2e/etc/passwd?t=limits-token",
        "/%2f/etc/passwd?t=limits-token",
        "/%zz?t=limits-token",
        "/%2?t=limits-token",
    ] {
        let status = get(addr, path);
        assert!(status == 400 || status == 404, "{path} returned {status}");
    }
}
