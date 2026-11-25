//! End-to-end HTTP tests against the public Studio server API.
//!
//! These use only `std::net` and the public crate interface, so they catch
//! accidental dependence on test-only internals.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::{Duration, Instant};

use terra_studio::json;
use terra_studio::server::{start, ServerConfig};

fn get(addr: SocketAddr, path: &str) -> (u16, String, Vec<u8>) {
    let mut stream = TcpStream::connect(addr).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(15)))
        .unwrap();
    let request = format!("GET {path} HTTP/1.1\r\nHost: localhost\r\n\r\n");
    stream.write_all(request.as_bytes()).unwrap();
    read_response(&mut stream)
}

fn post(addr: SocketAddr, path: &str, body: &str) -> (u16, String, Vec<u8>) {
    let mut stream = TcpStream::connect(addr).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(15)))
        .unwrap();
    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body
    );
    stream.write_all(request.as_bytes()).unwrap();
    read_response(&mut stream)
}

fn read_response(stream: &mut TcpStream) -> (u16, String, Vec<u8>) {
    let mut raw = Vec::new();
    let mut buf = [0u8; 8192];
    loop {
        match stream.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                raw.extend_from_slice(&buf[..n]);
                if let Some(pos) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&raw[..pos]).to_string();
                    let len = head
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(|v| v.trim().parse::<usize>().unwrap_or(0))
                        })
                        .unwrap_or(0);
                    if raw.len() >= pos + 4 + len {
                        return (status_of(&head), head, raw[pos + 4..pos + 4 + len].to_vec());
                    }
                }
            }
            Err(_) => break,
        }
    }
    let head = String::from_utf8_lossy(&raw).to_string();
    (status_of(&head), head, Vec::new())
}

fn status_of(head: &str) -> u16 {
    head.lines()
        .next()
        .and_then(|l| l.split(' ').nth(1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

fn server() -> SocketAddr {
    start(ServerConfig {
        port: 0,
        token: "integration-token".into(),
    })
    .expect("server")
    .addr
}

fn wait_for_world(addr: SocketAddr, id: &str) -> json::Json {
    let deadline = Instant::now() + Duration::from_secs(60);
    while Instant::now() < deadline {
        let (status, _, body) = get(addr, &format!("/api/job?id={id}&t=integration-token"));
        assert_eq!(status, 200);
        let value = json::parse(&String::from_utf8_lossy(&body)).unwrap();
        match value.get("status").unwrap().as_str() {
            Some("done") => return value,
            Some("failed") => panic!("job failed: {body:?}"),
            _ => std::thread::sleep(Duration::from_millis(40)),
        }
    }
    panic!("job {id} timed out");
}

#[test]
fn assets_auth_and_examples_work() {
    let addr = server();
    let (status, head, body) = get(addr, "/");
    assert_eq!(status, 200);
    assert!(head.contains("text/html"));
    assert!(String::from_utf8_lossy(&body).contains("Terra"));
    assert_eq!(get(addr, "/app.css").0, 200);
    assert_eq!(get(addr, "/app.js").0, 200);
    assert_eq!(get(addr, "/api/health").0, 401);
    assert_eq!(get(addr, "/api/health?t=integration-token").0, 200);
    assert_eq!(get(addr, "/api/examples?t=integration-token").0, 200);
    assert_eq!(get(addr, "/api/nope?t=integration-token").0, 404);
    assert_eq!(get(addr, "/%2e%2e/secret").0, 400);
}

#[test]
fn forge_map_trace_and_export_over_http() {
    let addr = server();
    let (status, _, body) = post(
        addr,
        "/api/run?t=integration-token",
        "{\"seed\":6,\"years\":300,\"subdiv\":2,\"civs\":3}",
    );
    assert_eq!(status, 202);
    let started = json::parse(&String::from_utf8_lossy(&body)).unwrap();
    let id = started.get("id").unwrap().as_str().unwrap().to_string();
    let job = wait_for_world(addr, &id);
    assert_eq!(job.get("status").unwrap().as_str(), Some("done"));
    assert!(
        job.get("world")
            .unwrap()
            .get("events")
            .unwrap()
            .as_u64()
            .unwrap()
            > 3
    );

    let (status, _, body) = get(addr, &format!("/api/world?id={id}&t=integration-token"));
    assert_eq!(status, 200);
    let world = json::parse(&String::from_utf8_lossy(&body)).unwrap();
    assert_eq!(world.get("id").unwrap().as_str(), Some(id.as_str()));
    assert!(world.get("layers").unwrap().as_array().unwrap().len() >= 10);

    let (status, head, png) = get(
        addr,
        &format!("/api/map?id={id}&layer=atlas&w=320&h=160&t=integration-token"),
    );
    assert_eq!(status, 200);
    assert!(head.contains("image/png"));
    assert_eq!(&png[..8], &terra_studio::png::PNG_SIGNATURE);

    let (status, _, body) = get(
        addr,
        &format!("/api/chronicle?id={id}&from=0&to=500&limit=20&t=integration-token"),
    );
    assert_eq!(status, 200);
    let page = json::parse(&String::from_utf8_lossy(&body)).unwrap();
    assert!(page.get("total").unwrap().as_u64().unwrap() >= 4);

    let first = page
        .get("events")
        .unwrap()
        .index(0)
        .unwrap()
        .get("id")
        .unwrap()
        .as_u64()
        .unwrap();
    let (status, _, body) = get(
        addr,
        &format!("/api/event?id={id}&event={first}&t=integration-token"),
    );
    assert_eq!(status, 200);
    let detail = json::parse(&String::from_utf8_lossy(&body)).unwrap();
    assert!(detail.get("chain").unwrap().get("nodes").is_some());
    assert!(!detail.get("explain").unwrap().as_str().unwrap().is_empty());

    for extension in ["md", "json", "csv"] {
        let (status, head, body) = get(
            addr,
            &format!("/api/export/chronicle.{extension}?id={id}&t=integration-token"),
        );
        assert_eq!(status, 200, "{extension}");
        assert!(!body.is_empty());
        assert!(head.to_lowercase().contains("attachment"));
    }
    let (status, _, body) = get(
        addr,
        &format!("/api/export/world.json?id={id}&t=integration-token"),
    );
    assert_eq!(status, 200);
    assert!(json::parse(&String::from_utf8_lossy(&body)).is_ok());
}
