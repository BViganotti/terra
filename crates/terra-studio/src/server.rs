//! Loopback HTTP server for Terra Studio.
//!
//! The server binds only to `127.0.0.1`, requires a random bearer token on every
//! API request, and serves the embedded client. It is intentionally small:
//! HTTP/1.1, one request per connection, bounded headers and bodies, and no
//! keep-alive or chunked transfer encoding. Generation runs on worker threads
//! and streams progress over Server-Sent Events.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::assets;
use crate::json::{self, Json};
use crate::render::{self, Layer, RenderOptions};
use terra_core::world::{World, WorldConfig};
use terra_history::chronicle::export as chronicle_export;
use terra_history::chronicle::{Cause, Event, EventKind};
use terra_history::civ::sim::{simulate, HistoryConfig};

/// Maximum request header size in bytes.
pub const MAX_HEADER_BYTES: usize = 16 * 1024;
/// Maximum request body size in bytes.
pub const MAX_BODY_BYTES: usize = 64 * 1024;
/// Maximum query parameters.
pub const MAX_QUERY_PARAMS: usize = 32;
/// Maximum URL length.
pub const MAX_URL_BYTES: usize = 512;

/// Server configuration.
#[derive(Clone, Debug)]
pub struct ServerConfig {
    /// Port to bind; zero asks the operating system for a free port.
    pub port: u16,
    /// Token to require. Empty means generate a random token.
    pub token: String,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            port: 0,
            token: String::new(),
        }
    }
}

/// A running Studio server.
pub struct Server {
    /// Address the listener is bound to.
    pub addr: SocketAddr,
    /// Bearer token required by the API.
    pub token: String,
}

/// Starts the server in a background thread.
pub fn start(config: ServerConfig) -> std::io::Result<Server> {
    let listener = TcpListener::bind(("127.0.0.1", config.port))?;
    let addr = listener.local_addr()?;
    let token = if config.token.is_empty() {
        random_token()
    } else {
        config.token
    };
    let app = Arc::new(App {
        token: token.clone(),
        started: Instant::now(),
        jobs: Mutex::new(Jobs::default()),
        next_id: AtomicU64::new(1),
    });
    let app_thread = Arc::clone(&app);
    std::thread::Builder::new()
        .name("terra-http".into())
        .spawn(move || {
            for conn in listener.incoming() {
                match conn {
                    Ok(stream) => {
                        let app = Arc::clone(&app_thread);
                        std::thread::Builder::new()
                            .name("terra-conn".into())
                            .spawn(move || {
                                let _ = handle_connection(stream, &app);
                            })
                            .ok();
                    }
                    Err(_) => break,
                }
            }
        })?;
    // Keep the app alive for the process lifetime.
    std::mem::forget(app);
    Ok(Server { addr, token })
}

/// Generates a random 128-bit token as lowercase hex.
pub fn random_token() -> String {
    let mut bytes = [0u8; 16];
    let from_urandom = std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut bytes))
        .is_ok();
    if !from_urandom {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(1);
        let pid = std::process::id();
        let mixed = now as u64 ^ (u64::from(pid) << 32) ^ 0x7465_7272_6100_0000;
        for (i, slot) in bytes.iter_mut().enumerate() {
            *slot = terra_core::rng::mix64(mixed.wrapping_add(i as u64)) as u8;
        }
    }
    let mut out = String::with_capacity(32);
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

/// The shared application state.
struct App {
    token: String,
    started: Instant,
    jobs: Mutex<Jobs>,
    next_id: AtomicU64,
}

#[derive(Default)]
struct Jobs {
    map: std::collections::BTreeMap<String, Job>,
    order: Vec<String>,
    running: usize,
}

struct Job {
    status: JobStatus,
    stage: String,
    progress: f64,
    error: Option<String>,
    started: Instant,
    world: Option<Arc<Artifact>>,
    request: RunRequest,
}

impl Job {
    fn status_json(&self) -> Json {
        let world = match &self.world {
            Some(a) => Json::obj(vec![
                ("planet", Json::str(a.world.name.clone())),
                ("hash", Json::str(a.world.hashes.combined.clone())),
                ("cells", Json::num(a.world.len() as f64)),
                ("years", Json::num(f64::from(a.history.config.years))),
                ("events", Json::num(a.history.chronicle.len() as f64)),
                ("generationMs", Json::num(a.generation_ms as f64)),
            ]),
            None => Json::Null,
        };
        Json::obj(vec![
            ("id", Json::str("")),
            ("status", Json::str(self.status.code())),
            ("stage", Json::str(self.stage.clone())),
            ("progress", Json::num(self.progress)),
            (
                "error",
                match &self.error {
                    Some(e) => Json::str(e.clone()),
                    None => Json::Null,
                },
            ),
            ("config", self.request.to_json()),
            ("world", world),
            (
                "elapsedMs",
                Json::num(self.started.elapsed().as_millis() as f64),
            ),
        ])
    }
}

/// A generated world and its history, shared across requests.
struct Artifact {
    world: terra_core::world::World,
    history: terra_history::civ::History,
    generation_ms: u64,
}

#[derive(Clone, Debug, Default)]
struct RunRequest {
    seed: Option<u64>,
    years: Option<u32>,
    subdiv: Option<u32>,
    initial_civs: Option<u32>,
    aggression: Option<f64>,
    expansion: Option<f64>,
    preset: Option<String>,
    scenario: Option<String>,
}

impl RunRequest {
    fn from_json(value: &Json) -> Result<Self, String> {
        let object = value
            .as_object()
            .ok_or_else(|| "request body must be a JSON object".to_string())?;
        let mut req = RunRequest::default();
        for (key, value) in object {
            match key.as_str() {
                "seed" => {
                    req.seed = Some(
                        value
                            .as_u64()
                            .ok_or_else(|| "seed must be a non-negative integer".to_string())?,
                    )
                }
                "years" => {
                    req.years = Some(
                        value
                            .as_u64()
                            .ok_or_else(|| "years must be an integer".to_string())?
                            as u32,
                    )
                }
                "subdiv" => {
                    req.subdiv = Some(
                        value
                            .as_u64()
                            .ok_or_else(|| "subdiv must be an integer".to_string())?
                            as u32,
                    )
                }
                "civs" => {
                    req.initial_civs = Some(
                        value
                            .as_u64()
                            .ok_or_else(|| "civs must be an integer".to_string())?
                            as u32,
                    )
                }
                "aggression" => {
                    req.aggression = Some(
                        value
                            .as_f64()
                            .ok_or_else(|| "aggression must be a number".to_string())?,
                    )
                }
                "expansion" => {
                    req.expansion = Some(
                        value
                            .as_f64()
                            .ok_or_else(|| "expansion must be a number".to_string())?,
                    )
                }
                "preset" => {
                    req.preset = Some(
                        value
                            .as_str()
                            .ok_or_else(|| "preset must be a string".to_string())?
                            .to_string(),
                    )
                }
                "scenario" => {
                    let text = value
                        .as_str()
                        .ok_or_else(|| "scenario must be a string".to_string())?;
                    if text.len() > terra_core::scenario::MAX_INPUT_BYTES {
                        return Err("scenario text is too large".into());
                    }
                    req.scenario = Some(text.to_string());
                }
                other => return Err(format!("unknown field {other}")),
            }
        }
        Ok(req)
    }

    fn resolve(&self) -> Result<(WorldConfig, HistoryConfig), String> {
        let mut scenario = if let Some(text) = &self.scenario {
            terra_core::scenario::parse(text).map_err(|e| e.to_string())?
        } else if let Some(preset) = &self.preset {
            terra_core::scenario::Scenario::preset(
                preset,
                self.seed.unwrap_or(42),
                self.years.unwrap_or(10_000),
            )
            .ok_or_else(|| format!("unknown preset {preset}"))?
        } else {
            terra_core::scenario::Scenario::from_config(WorldConfig::default())
        };
        if let Some(seed) = self.seed {
            scenario.config.seed = seed.min(u64::from(u32::MAX));
        }
        if let Some(years) = self.years {
            scenario.config.years = years.clamp(100, 20_000);
        }
        if let Some(subdiv) = self.subdiv {
            scenario.config.subdiv = subdiv.clamp(2, 5);
        }
        let world = scenario.config.sanitized();
        let years = world.years;
        let history = HistoryConfig {
            seed: world.seed,
            years,
            initial_civs: self.initial_civs.unwrap_or(8).clamp(2, 12),
            aggression: self.aggression.unwrap_or(1.0),
            expansion: self.expansion.unwrap_or(1.0),
            step_years: (years / 2_000).clamp(5, 25),
            snapshot_interval: (years / 100).clamp(50, 1_000),
        }
        .sanitized();
        Ok((world, history))
    }

    fn to_json(&self) -> Json {
        let seed = match self.seed {
            Some(s) => Json::num(s as f64),
            None => Json::Null,
        };
        let years = match self.years {
            Some(y) => Json::num(f64::from(y)),
            None => Json::Null,
        };
        Json::obj(vec![
            ("seed", seed),
            ("years", years),
            (
                "subdiv",
                match self.subdiv {
                    Some(s) => Json::num(f64::from(s)),
                    None => Json::Null,
                },
            ),
            (
                "civs",
                match self.initial_civs {
                    Some(c) => Json::num(f64::from(c)),
                    None => Json::Null,
                },
            ),
            (
                "preset",
                match &self.preset {
                    Some(p) => Json::str(p.clone()),
                    None => Json::Null,
                },
            ),
        ])
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum JobStatus {
    Running,
    Done,
    Failed,
}

impl JobStatus {
    fn code(self) -> &'static str {
        match self {
            JobStatus::Running => "running",
            JobStatus::Done => "done",
            JobStatus::Failed => "failed",
        }
    }
}

#[derive(Debug)]
struct HttpRequest {
    method: String,
    path: String,
    query: std::collections::BTreeMap<String, String>,
    headers: std::collections::BTreeMap<String, String>,
    body: Vec<u8>,
}

struct Response {
    status: u16,
    content_type: &'static str,
    body: Vec<u8>,
    headers: Vec<(&'static str, String)>,
}

impl Response {
    fn new(status: u16, content_type: &'static str, body: impl Into<Vec<u8>>) -> Self {
        Self {
            status,
            content_type,
            body: body.into(),
            headers: Vec::new(),
        }
    }

    fn json(value: &Json) -> Self {
        Self::new(200, "application/json; charset=utf-8", value.to_string())
    }

    fn error(status: u16, message: &str) -> Self {
        let body = Json::obj(vec![
            ("ok", Json::Bool(false)),
            ("error", Json::str(message)),
        ]);
        Self::new(status, "application/json; charset=utf-8", body.to_string())
    }

    fn html(body: String) -> Self {
        let mut r = Self::new(200, "text/html; charset=utf-8", body);
        r.headers.push((
            "Content-Security-Policy",
            "default-src 'self'; img-src 'self' data:; style-src 'self' 'unsafe-inline'; script-src 'self'"
                .to_string(),
        ));
        r
    }

    fn with_header(mut self, name: &'static str, value: impl Into<String>) -> Self {
        self.headers.push((name, value.into()));
        self
    }
}

fn handle_connection(mut stream: TcpStream, app: &Arc<App>) -> std::io::Result<()> {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(15)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(30)));
    let mut buffer = Vec::with_capacity(2_048);
    let mut chunk = [0u8; 2_048];
    let header_end = loop {
        if buffer.len() > MAX_HEADER_BYTES {
            let _ = send(
                &mut stream,
                Response::error(431, "request headers too large"),
            );
            return Ok(());
        }
        let n = match stream.read(&mut chunk) {
            Ok(0) => {
                return Ok(());
            }
            Ok(n) => n,
            Err(_) => return Ok(()),
        };
        buffer.extend_from_slice(&chunk[..n]);
        if let Some(pos) = find_header_end(&buffer) {
            break pos;
        }
    };

    let request = match parse_request(&buffer, header_end) {
        Ok(mut req) => {
            // Read declared body bytes.
            if let Some(len) = req
                .headers
                .get("content-length")
                .and_then(|v| v.parse::<usize>().ok())
            {
                if len > MAX_BODY_BYTES {
                    let _ = send(&mut stream, Response::error(413, "request body too large"));
                    return Ok(());
                }
                if req.body.len() < len {
                    let mut rest = vec![0u8; len - req.body.len()];
                    stream.read_exact(&mut rest)?;
                    req.body.extend_from_slice(&rest);
                }
            }
            req
        }
        Err(status) => {
            let message = match status {
                400 => "malformed request",
                405 => "method not allowed",
                414 => "url too long",
                _ => "bad request",
            };
            let _ = send(&mut stream, Response::error(status, message));
            return Ok(());
        }
    };

    // Authentication applies to every API route. Static assets and the page
    // itself are public on loopback; the token is what protects state changes.
    let is_public = matches!(
        request.path.as_str(),
        "/" | "/index.html" | "/app.css" | "/app.js" | "/favicon.svg"
    );
    if !is_public && !authorized(&request, &app.token) {
        let _ = send(
            &mut stream,
            Response::error(401, "missing or invalid token")
                .with_header("WWW-Authenticate", "Bearer realm=\"terra\""),
        );
        return Ok(());
    }

    if request.path.starts_with("/api/events") {
        stream_sse(&mut stream, app, &request);
        return Ok(());
    }

    let response = route(app, &request);
    send(&mut stream, response)
}

fn find_header_end(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|w| w == b"\r\n\r\n")
}

fn parse_request(buffer: &[u8], header_end: usize) -> Result<HttpRequest, u16> {
    let head = std::str::from_utf8(&buffer[..header_end]).map_err(|_| 400u16)?;
    let mut lines = head.split("\r\n");
    let request_line = lines.next().ok_or(400u16)?;
    let mut parts = request_line.split(' ');
    let method = parts.next().ok_or(400u16)?.to_string();
    let target = parts.next().ok_or(400u16)?.to_string();
    let version = parts.next().unwrap_or("");
    if parts.next().is_some() || !version.starts_with("HTTP/1.") {
        return Err(400);
    }
    if method != "GET" && method != "POST" {
        return Err(405);
    }
    if target.len() > MAX_URL_BYTES {
        return Err(414);
    }
    if !target.starts_with('/') || target.contains("..") || target.contains('\0') {
        return Err(400);
    }
    let (path, query) = parse_target(&target)?;
    let mut headers = std::collections::BTreeMap::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        if headers.len() >= 64 {
            return Err(400);
        }
        let Some((name, value)) = line.split_once(':') else {
            return Err(400);
        };
        let name = name.trim().to_ascii_lowercase();
        if name.is_empty() || name.len() > 64 {
            return Err(400);
        }
        let value = value.trim();
        if value.len() > 1_024 {
            return Err(400);
        }
        headers.insert(name, value.to_string());
    }
    if headers.contains_key("transfer-encoding") {
        return Err(400);
    }
    Ok(HttpRequest {
        method,
        path,
        query,
        headers,
        body: buffer[header_end + 4..].to_vec(),
    })
}

fn parse_target(target: &str) -> Result<(String, std::collections::BTreeMap<String, String>), u16> {
    let (path, raw_query) = target.split_once('?').unwrap_or((target, ""));
    let path = percent_decode(path).ok_or(400u16)?;
    if path.len() > MAX_URL_BYTES || !path.starts_with('/') || path.contains("..") {
        return Err(400);
    }
    let mut query = std::collections::BTreeMap::new();
    for pair in raw_query.split('&') {
        if pair.is_empty() {
            continue;
        }
        if query.len() >= MAX_QUERY_PARAMS {
            return Err(400);
        }
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        let key = percent_decode(key).ok_or(400u16)?;
        let value = percent_decode(value).ok_or(400u16)?;
        if key.len() > 64 || value.len() > 4_096 {
            return Err(400);
        }
        query.insert(key, value);
    }
    Ok((path, query))
}

fn percent_decode(input: &str) -> Option<String> {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                if i + 2 >= bytes.len() {
                    return None;
                }
                let hi = hex_value(bytes[i + 1])?;
                let lo = hex_value(bytes[i + 2])?;
                out.push((hi << 4) | lo);
                i += 3;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

fn hex_value(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn authorized(request: &HttpRequest, token: &str) -> bool {
    let supplied = request
        .query
        .get("t")
        .map(String::as_str)
        .or_else(|| request.headers.get("x-token").map(String::as_str))
        .or_else(|| {
            request
                .headers
                .get("authorization")
                .and_then(|v| v.strip_prefix("Bearer "))
        });
    match supplied {
        Some(value) => constant_time_eq(value.as_bytes(), token.as_bytes()),
        None => false,
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b) {
        diff |= x ^ y;
    }
    diff == 0
}

fn send(stream: &mut TcpStream, response: Response) -> std::io::Result<()> {
    let mut head = String::with_capacity(256);
    head.push_str(&format!(
        "HTTP/1.1 {} {}\r\n",
        response.status,
        reason(response.status)
    ));
    head.push_str(&format!("Content-Type: {}\r\n", response.content_type));
    head.push_str(&format!("Content-Length: {}\r\n", response.body.len()));
    head.push_str("Cache-Control: no-store\r\n");
    head.push_str("X-Content-Type-Options: nosniff\r\n");
    head.push_str("Referrer-Policy: no-referrer\r\n");
    head.push_str("Connection: close\r\n");
    for (name, value) in &response.headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes())?;
    stream.write_all(&response.body)?;
    stream.flush()
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        202 => "Accepted",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        405 => "Method Not Allowed",
        413 => "Payload Too Large",
        414 => "URI Too Long",
        429 => "Too Many Requests",
        431 => "Request Header Fields Too Large",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        _ => "Error",
    }
}

fn route(app: &Arc<App>, request: &HttpRequest) -> Response {
    match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/") => Response::html(assets::INDEX_HTML.to_string()),
        ("GET", "/index.html") => Response::html(assets::INDEX_HTML.to_string()),
        ("GET", "/app.css") => Response::new(
            200,
            "text/css; charset=utf-8",
            assets::APP_CSS.as_bytes().to_vec(),
        ),
        ("GET", "/app.js") => Response::new(
            200,
            "text/javascript; charset=utf-8",
            assets::APP_JS.as_bytes().to_vec(),
        ),
        ("GET", "/favicon.svg") => Response::new(
            200,
            "image/svg+xml",
            assets::FAVICON_SVG.as_bytes().to_vec(),
        ),
        ("GET", "/api/health") => health(app),
        ("POST", "/api/run") => run_endpoint(app, request),
        ("GET", "/api/job") => job_endpoint(app, request),
        ("GET", "/api/world") => world_endpoint(app, request),
        ("GET", "/api/map") => map_endpoint(app, request),
        ("GET", "/api/chronicle") => chronicle_endpoint(app, request),
        ("GET", "/api/event") => event_endpoint(app, request),
        ("GET", "/api/cell") => cell_endpoint(app, request),
        ("GET", "/api/civs") => civs_endpoint(app, request),
        ("GET", "/api/civ") => civ_endpoint(app, request),
        ("GET", "/api/timeline") => timeline_endpoint(app, request),
        ("GET", "/api/export/chronicle.md") => export_markdown(app, request),
        ("GET", "/api/export/chronicle.json") => export_json(app, request),
        ("GET", "/api/export/chronicle.csv") => export_csv(app, request),
        ("GET", "/api/export/world.json") => export_world(app, request),
        _ => Response::error(404, "not found"),
    }
}

fn health(app: &Arc<App>) -> Response {
    let jobs = app.jobs.lock().unwrap_or_else(|e| e.into_inner());
    let body = Json::obj(vec![
        ("ok", Json::Bool(true)),
        ("version", Json::str(crate::VERSION)),
        ("terra", Json::str(terra_core::VERSION)),
        ("history", Json::str(terra_history::VERSION)),
        ("worlds", Json::num(jobs.map.len() as f64)),
        ("running", Json::num(jobs.running as f64)),
        (
            "uptimeSec",
            Json::num(app.started.elapsed().as_secs_f64().round()),
        ),
    ]);
    Response::json(&body)
}

fn run_endpoint(app: &Arc<App>, request: &HttpRequest) -> Response {
    if request.body.len() > MAX_BODY_BYTES {
        return Response::error(413, "request body too large");
    }
    let body = match std::str::from_utf8(&request.body) {
        Ok(s) => s,
        Err(_) => return Response::error(400, "body must be UTF-8"),
    };
    let value = match json::parse(body) {
        Ok(v) => v,
        Err(e) => return Response::error(400, &format!("invalid JSON: {e}")),
    };
    let req = match RunRequest::from_json(&value) {
        Ok(r) => r,
        Err(e) => return Response::error(400, &e),
    };
    if let Err(e) = req.resolve() {
        return Response::error(400, &e);
    }
    match start_job(app, req) {
        Ok(id) => {
            let mut r = Response::json(&Json::obj(vec![
                ("id", Json::str(id.clone())),
                ("worldId", Json::str(id)),
            ]));
            r.status = 202;
            r
        }
        Err(e) => Response::error(429, &e),
    }
}

fn job_endpoint(app: &Arc<App>, request: &HttpRequest) -> Response {
    let Some(id) = request.query.get("id") else {
        return Response::error(400, "missing job id");
    };
    let jobs = app.jobs.lock().unwrap_or_else(|e| e.into_inner());
    let Some(job) = jobs.map.get(id) else {
        return Response::error(404, "unknown job");
    };
    let mut value = job.status_json();
    if let Json::Object(pairs) = &mut value {
        for (key, v) in pairs.iter_mut() {
            if key == "id" {
                *v = Json::str(id.clone());
            }
        }
    }
    Response::json(&value)
}

fn start_job(app: &Arc<App>, req: RunRequest) -> Result<String, String> {
    let (world_cfg, history_cfg) = req.resolve()?;
    let mut jobs = app.jobs.lock().unwrap_or_else(|e| e.into_inner());
    if jobs.running >= 2 {
        return Err("two generations are already running; wait for one to finish".into());
    }
    // Evict the oldest finished world when the cache is full.
    while jobs.map.len() >= 10 {
        let victim = jobs
            .order
            .iter()
            .find(|id| {
                jobs.map
                    .get(*id)
                    .is_some_and(|j| j.status != JobStatus::Running)
            })
            .cloned();
        match victim {
            Some(id) => {
                jobs.map.remove(&id);
                jobs.order.retain(|x| x != &id);
            }
            None => break,
        }
    }
    let id = format!("w{}", app.next_id.fetch_add(1, Ordering::Relaxed));
    jobs.map.insert(
        id.clone(),
        Job {
            status: JobStatus::Running,
            stage: "queued".into(),
            progress: 0.0,
            error: None,
            started: Instant::now(),
            world: None,
            request: req.clone(),
        },
    );
    jobs.order.push(id.clone());
    jobs.running += 1;
    drop(jobs);

    let app2 = Arc::clone(app);
    let id2 = id.clone();
    std::thread::Builder::new()
        .name("terra-generate".into())
        .spawn(move || run_job(app2, id2, world_cfg, history_cfg))
        .map_err(|e| format!("could not start worker: {e}"))?;
    Ok(id)
}

fn set_progress(app: &Arc<App>, id: &str, stage: &str, progress: f64) {
    let mut jobs = app.jobs.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(job) = jobs.map.get_mut(id) {
        job.stage = stage.to_string();
        job.progress = progress.clamp(0.0, 1.0);
    }
}

fn run_job(app: Arc<App>, id: String, world_cfg: WorldConfig, history_cfg: HistoryConfig) {
    let started = Instant::now();
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let world = World::generate_with(world_cfg, |stage, progress| {
            set_progress(&app, &id, stage, 0.02 + progress * 0.68);
        });
        set_progress(&app, &id, "history", 0.74);
        let history = simulate(&world, history_cfg);
        if !history.chronicle.is_sound() {
            return Err(format!(
                "chronicle validation failed: {:?}",
                history.problems()
            ));
        }
        set_progress(&app, &id, "atlas", 0.96);
        Ok(Arc::new(Artifact {
            world,
            history,
            generation_ms: started.elapsed().as_millis() as u64,
        }))
    }));
    let mut jobs = app.jobs.lock().unwrap_or_else(|e| e.into_inner());
    jobs.running = jobs.running.saturating_sub(1);
    let Some(job) = jobs.map.get_mut(&id) else {
        return;
    };
    match outcome {
        Ok(Ok(artifact)) => {
            job.world = Some(artifact);
            job.status = JobStatus::Done;
            job.stage = "complete".into();
            job.progress = 1.0;
            job.error = None;
        }
        Ok(Err(message)) => {
            job.status = JobStatus::Failed;
            job.stage = "failed".into();
            job.progress = 1.0;
            job.error = Some(message);
        }
        Err(_) => {
            job.status = JobStatus::Failed;
            job.stage = "failed".into();
            job.progress = 1.0;
            job.error = Some("generation panicked".into());
        }
    }
}

fn artifact(app: &Arc<App>, id: &str) -> Result<Arc<Artifact>, Response> {
    let jobs = app.jobs.lock().unwrap_or_else(|e| e.into_inner());
    let Some(job) = jobs.map.get(id) else {
        return Err(Response::error(404, "unknown world"));
    };
    match job.status {
        JobStatus::Running => Err(Response::error(503, "world is still generating")),
        JobStatus::Failed => Err(Response::error(
            500,
            job.error.as_deref().unwrap_or("generation failed"),
        )),
        JobStatus::Done => job
            .world
            .clone()
            .ok_or_else(|| Response::error(500, "world missing")),
    }
}

fn request_id(request: &HttpRequest) -> Result<String, Response> {
    request
        .query
        .get("id")
        .cloned()
        .ok_or_else(|| Response::error(400, "missing world id"))
}

fn query_u32(request: &HttpRequest, key: &str) -> Result<Option<u32>, Response> {
    match request.query.get(key) {
        None => Ok(None),
        Some(v) => v
            .parse::<u32>()
            .map(Some)
            .map_err(|_| Response::error(400, &format!("{key} must be a non-negative integer"))),
    }
}

fn query_usize(request: &HttpRequest, key: &str, default: usize) -> Result<usize, Response> {
    match request.query.get(key) {
        None => Ok(default),
        Some(v) => v
            .parse::<usize>()
            .map_err(|_| Response::error(400, &format!("{key} must be a non-negative integer"))),
    }
}

fn world_endpoint(app: &Arc<App>, request: &HttpRequest) -> Response {
    let id = match request_id(request) {
        Ok(id) => id,
        Err(r) => return r,
    };
    let art = match artifact(app, &id) {
        Ok(a) => a,
        Err(r) => return r,
    };
    let world = &art.world;
    let history = &art.history;
    let stats = &world.stats;
    let mut layers = Vec::new();
    for layer in Layer::ALL {
        let entries: Vec<Json> = render::legend(layer)
            .into_iter()
            .map(|e| {
                Json::obj(vec![
                    ("label", Json::str(e.label)),
                    ("color", Json::str(format!("#{:06x}", e.rgb))),
                ])
            })
            .collect();
        layers.push(Json::obj(vec![
            ("code", Json::str(layer.code())),
            ("label", Json::str(layer.label())),
            ("legend", Json::Array(entries)),
        ]));
    }
    let civs: Vec<Json> = history
        .civs
        .iter()
        .map(|c| {
            Json::obj(vec![
                ("id", Json::num(f64::from(c.id))),
                ("name", Json::str(c.name.clone())),
                (
                    "color",
                    Json::str(format_hex(render::civ_color(c.color_index))),
                ),
                ("alive", Json::Bool(c.alive)),
                ("population", Json::num(c.population)),
                ("cells", Json::num(c.cells.len() as f64)),
                ("tech", Json::num(f64::from(c.tech))),
                ("founded", Json::num(f64::from(c.founded_year))),
                (
                    "collapsedYear",
                    match c.collapsed_year {
                        Some(y) => Json::num(f64::from(y)),
                        None => Json::Null,
                    },
                ),
            ])
        })
        .collect();
    let cs = history.chronicle.stats();
    let body = Json::obj(vec![
        ("id", Json::str(id)),
        ("planet", Json::str(world.name.clone())),
        ("seed", Json::num(world.config.seed as f64)),
        ("years", Json::num(f64::from(history.config.years))),
        ("subdiv", Json::num(f64::from(world.config.subdiv))),
        ("cells", Json::num(world.len() as f64)),
        ("hash", Json::str(world.hashes.combined.clone())),
        (
            "stats",
            Json::obj(vec![
                ("landFraction", Json::num(stats.land_fraction)),
                ("landCells", Json::num(f64::from(stats.land_cells))),
                ("oceanCells", Json::num(f64::from(stats.ocean_cells))),
                ("lakes", Json::num(f64::from(stats.lakes))),
                ("rivers", Json::num(f64::from(stats.river_cells))),
                ("deltas", Json::num(f64::from(stats.delta_cells))),
                ("plates", Json::num(f64::from(stats.plates))),
                ("convergent", Json::num(f64::from(stats.convergent))),
                ("divergent", Json::num(f64::from(stats.divergent))),
                ("transform", Json::num(f64::from(stats.transform))),
                ("continents", Json::num(f64::from(stats.continents))),
                (
                    "largestContinentKm2",
                    Json::num(stats.largest_continent_km2),
                ),
                ("meanTemperatureC", Json::num(stats.mean_temperature_c)),
                ("meanPrecipMm", Json::num(stats.mean_precip_mm)),
                ("maxElevationM", Json::num(stats.max_elevation)),
                ("minElevationM", Json::num(stats.min_elevation)),
                ("meanLandElevationM", Json::num(stats.mean_land_elevation)),
                ("meanSpecies", Json::num(stats.mean_species)),
                ("carryingCapacity", Json::num(stats.carrying_capacity)),
                ("npp", Json::num(stats.npp_t_c_yr)),
                (
                    "biomes",
                    Json::Object(
                        stats
                            .biome_counts
                            .iter()
                            .map(|(k, v)| ((*k).to_string(), Json::num(f64::from(*v))))
                            .collect(),
                    ),
                ),
            ]),
        ),
        (
            "hashes",
            Json::obj(vec![
                ("plates", Json::str(world.hashes.plates.clone())),
                ("terrain", Json::str(world.hashes.terrain.clone())),
                ("hydrology", Json::str(world.hashes.hydrology.clone())),
                ("climate", Json::str(world.hashes.climate.clone())),
                ("biosphere", Json::str(world.hashes.biosphere.clone())),
                ("chronicle", Json::str(history.hashes.chronicle.clone())),
                ("history", Json::str(history.hashes.civs.clone())),
            ]),
        ),
        (
            "history",
            Json::obj(vec![
                ("civs", Json::num(history.civs.len() as f64)),
                (
                    "surviving",
                    Json::num(f64::from(history.stats.surviving_civs)),
                ),
                ("collapses", Json::num(f64::from(history.stats.collapses))),
                ("wars", Json::num(f64::from(history.stats.wars))),
                ("battles", Json::num(f64::from(history.stats.battles))),
                (
                    "tradeRoutes",
                    Json::num(f64::from(history.stats.trade_routes)),
                ),
                ("cities", Json::num(f64::from(history.stats.cities))),
                ("migrations", Json::num(f64::from(history.stats.migrations))),
                ("famines", Json::num(f64::from(history.stats.famines))),
                ("totalPopulation", Json::num(history.stats.total_population)),
                (
                    "claimedCells",
                    Json::num(f64::from(history.stats.claimed_cells)),
                ),
                ("sound", Json::Bool(history.chronicle.is_sound())),
            ]),
        ),
        (
            "chronicle",
            Json::obj(vec![
                ("events", Json::num(f64::from(cs.events))),
                ("firstYear", Json::num(f64::from(cs.first_year))),
                ("lastYear", Json::num(f64::from(cs.last_year))),
                ("rootEvents", Json::num(f64::from(cs.root_events))),
                ("meanSeverity", Json::num(cs.mean_severity)),
                ("kinds", Json::num(f64::from(cs.kinds))),
                (
                    "counts",
                    Json::Object(
                        history
                            .chronicle
                            .counts_by_kind()
                            .iter()
                            .map(|(k, v)| (k.code().to_string(), Json::num(f64::from(*v))))
                            .collect(),
                    ),
                ),
            ]),
        ),
        ("layers", Json::Array(layers)),
        ("civs", Json::Array(civs)),
        (
            "snapshotYears",
            Json::Array(
                history
                    .snapshots
                    .iter()
                    .map(|s| Json::num(f64::from(s.year)))
                    .collect(),
            ),
        ),
    ]);
    Response::json(&body)
}

fn map_endpoint(app: &Arc<App>, request: &HttpRequest) -> Response {
    let id = match request_id(request) {
        Ok(id) => id,
        Err(r) => return r,
    };
    let art = match artifact(app, &id) {
        Ok(a) => a,
        Err(r) => return r,
    };
    let layer_code = request
        .query
        .get("layer")
        .map(String::as_str)
        .unwrap_or("atlas");
    let Some(layer) = Layer::from_code(layer_code) else {
        return Response::error(400, "unknown layer");
    };
    let Ok(width) = query_usize(request, "w", 1024) else {
        return Response::error(400, "w must be an integer");
    };
    let Ok(height) = query_usize(request, "h", 512) else {
        return Response::error(400, "h must be an integer");
    };
    let year = match query_u32(request, "year") {
        Ok(y) => y,
        Err(r) => return r,
    };
    let opts = RenderOptions {
        width: width.clamp(64, 2_048) as u32,
        height: height.clamp(32, 1_024) as u32,
        layer,
        year,
        rivers: request.query.get("rivers").map(String::as_str) != Some("0"),
        civ_overlay: layer == Layer::Political
            || request.query.get("civs").map(String::as_str) == Some("1"),
        hillshade: request.query.get("shade").map(String::as_str) != Some("0"),
    };
    let image = render::render(&art.world, Some(&art.history), opts);
    Response::new(200, "image/png", image.to_png())
}

fn event_summary_json(event: &Event) -> Json {
    Json::obj(vec![
        ("id", Json::num(f64::from(event.id))),
        ("year", Json::num(f64::from(event.year))),
        ("kind", Json::str(event.kind.code())),
        ("category", Json::str(event.kind.category())),
        ("label", Json::str(event.kind.label())),
        ("title", Json::str(event.title.clone())),
        ("severity", Json::num(event.severity)),
        (
            "civ",
            match event.civ {
                Some(c) => Json::num(f64::from(c)),
                None => Json::Null,
            },
        ),
        (
            "cell",
            match event.cell {
                Some(c) => Json::num(f64::from(c)),
                None => Json::Null,
            },
        ),
    ])
}

fn chronicle_endpoint(app: &Arc<App>, request: &HttpRequest) -> Response {
    let id = match request_id(request) {
        Ok(id) => id,
        Err(r) => return r,
    };
    let art = match artifact(app, &id) {
        Ok(a) => a,
        Err(r) => return r,
    };
    let from = match query_u32(request, "from") {
        Ok(v) => v,
        Err(r) => return r,
    };
    let to = match query_u32(request, "to") {
        Ok(v) => v,
        Err(r) => return r,
    };
    let civ = match request.query.get("civ") {
        None => None,
        Some(v) => match v.parse::<u16>() {
            Ok(c) => Some(c),
            Err(_) => return Response::error(400, "civ must be an integer"),
        },
    };
    let cell = match query_u32(request, "cell") {
        Ok(v) => v,
        Err(r) => return r,
    };
    let offset = match query_usize(request, "offset", 0) {
        Ok(v) => v,
        Err(r) => return r,
    };
    let limit = match query_usize(request, "limit", 50) {
        Ok(v) => v.clamp(1, 200),
        Err(r) => return r,
    };
    let mut query = art.history.chronicle.query();
    if let (Some(a), Some(b)) = (from, to) {
        query = query.years(a, b);
    } else if let Some(a) = from {
        query = query.years(a, u32::MAX);
    } else if let Some(b) = to {
        query = query.years(0, b);
    }
    if let Some(kind) = request.query.get("kind") {
        if kind != "all" {
            match EventKind::from_code(kind) {
                Some(k) => query = query.kind(k),
                None => return Response::error(400, "unknown event kind"),
            }
        }
    }
    if let Some(c) = civ {
        query = query.civ(c);
    }
    if let Some(c) = cell {
        query = query.cell(c);
    }
    if let Some(q) = request.query.get("q") {
        query = query.text(q.clone());
    }
    let total = query.count();
    let events: Vec<Json> = query
        .page(offset, limit)
        .into_iter()
        .map(event_summary_json)
        .collect();
    let body = Json::obj(vec![
        ("total", Json::num(total as f64)),
        ("offset", Json::num(offset as f64)),
        ("limit", Json::num(limit as f64)),
        ("events", Json::Array(events)),
    ]);
    Response::json(&body)
}

fn cause_json(cause: &Cause, history: &terra_history::civ::History) -> Json {
    match cause {
        Cause::Event(id) => {
            let target = history.chronicle.get(*id);
            Json::obj(vec![
                ("type", Json::str("event")),
                ("id", Json::num(f64::from(*id))),
                (
                    "title",
                    target
                        .map(|e| Json::str(e.title.clone()))
                        .unwrap_or(Json::Null),
                ),
                (
                    "year",
                    target
                        .map(|e| Json::num(f64::from(e.year)))
                        .unwrap_or(Json::Null),
                ),
                (
                    "kind",
                    target
                        .map(|e| Json::str(e.kind.code()))
                        .unwrap_or(Json::Null),
                ),
            ])
        }
        Cause::State(f) => Json::obj(vec![
            ("type", Json::str("state")),
            ("key", Json::str(f.key)),
            ("label", Json::str(f.label)),
            ("value", Json::num(f.value)),
            ("unit", Json::str(f.unit)),
            ("text", Json::str(f.describe())),
            (
                "cell",
                match f.cell {
                    Some(c) => Json::num(f64::from(c)),
                    None => Json::Null,
                },
            ),
            (
                "civ",
                match f.civ {
                    Some(c) => Json::num(f64::from(c)),
                    None => Json::Null,
                },
            ),
        ]),
    }
}

fn event_endpoint(app: &Arc<App>, request: &HttpRequest) -> Response {
    let id = match request_id(request) {
        Ok(id) => id,
        Err(r) => return r,
    };
    let art = match artifact(app, &id) {
        Ok(a) => a,
        Err(r) => return r,
    };
    let event_id = match query_u32(request, "event") {
        Ok(Some(v)) => v,
        Ok(None) => return Response::error(400, "missing event id"),
        Err(r) => return r,
    };
    let Some(event) = art.history.chronicle.get(event_id) else {
        return Response::error(404, "unknown event");
    };
    let causes: Vec<Json> = event
        .causes
        .iter()
        .map(|c| cause_json(c, &art.history))
        .collect();
    let chain = art.history.chronicle.cause_chain(event_id);
    let chain_json = match &chain {
        Some(chain) => {
            let nodes: Vec<Json> = chain
                .nodes
                .iter()
                .map(|n| {
                    Json::obj(vec![
                        ("id", Json::num(f64::from(n.event.id))),
                        ("year", Json::num(f64::from(n.event.year))),
                        ("kind", Json::str(n.event.kind)),
                        ("title", Json::str(n.event.title.clone())),
                        ("depth", Json::num(f64::from(n.depth))),
                        (
                            "parent",
                            match n.parent {
                                Some(p) => Json::num(f64::from(p)),
                                None => Json::Null,
                            },
                        ),
                    ])
                })
                .collect();
            let facts: Vec<Json> = chain
                .facts
                .iter()
                .map(|f| {
                    Json::obj(vec![
                        ("depth", Json::num(f64::from(f.depth))),
                        ("from", Json::num(f64::from(f.from))),
                        ("key", Json::str(f.fact.key)),
                        ("label", Json::str(f.fact.label)),
                        ("value", Json::num(f.fact.value)),
                        ("unit", Json::str(f.fact.unit)),
                        ("text", Json::str(f.fact.describe())),
                    ])
                })
                .collect();
            Json::obj(vec![
                ("nodes", Json::Array(nodes)),
                ("facts", Json::Array(facts)),
                ("maxDepth", Json::num(f64::from(chain.max_depth))),
                ("truncated", Json::Bool(chain.truncated)),
            ])
        }
        None => Json::Null,
    };
    let paths = art.history.chronicle.trace_paths(event_id, 8);
    let path_json: Vec<Json> = paths
        .iter()
        .map(|p| {
            let steps: Vec<Json> = p
                .steps
                .iter()
                .map(|step| match step {
                    terra_history::chronicle::query::TraceStep::Event(e) => Json::obj(vec![
                        ("type", Json::str("event")),
                        ("id", Json::num(f64::from(e.id))),
                        ("title", Json::str(e.title.clone())),
                        ("year", Json::num(f64::from(e.year))),
                    ]),
                    terra_history::chronicle::query::TraceStep::Fact(f) => Json::obj(vec![
                        ("type", Json::str("state")),
                        ("label", Json::str(f.label)),
                        ("text", Json::str(f.describe())),
                        ("value", Json::num(f.value)),
                        ("unit", Json::str(f.unit)),
                    ]),
                })
                .collect();
            Json::Array(steps)
        })
        .collect();
    let descendants: Vec<Json> = art
        .history
        .chronicle
        .descendants(event_id)
        .into_iter()
        .take(40)
        .filter_map(|d| art.history.chronicle.get(d))
        .map(event_summary_json)
        .collect();
    let civ = event
        .civ
        .and_then(|c| art.history.civs.get(c as usize))
        .map(|c| {
            Json::obj(vec![
                ("id", Json::num(f64::from(c.id))),
                ("name", Json::str(c.name.clone())),
                ("alive", Json::Bool(c.alive)),
            ])
        });
    let body = Json::obj(vec![
        ("event", event_summary_json(event)),
        ("detail", Json::str(event.detail.clone())),
        ("causes", Json::Array(causes)),
        (
            "explain",
            art.history
                .chronicle
                .explain(event_id)
                .map(Json::str)
                .unwrap_or(Json::Null),
        ),
        ("chain", chain_json),
        ("paths", Json::Array(path_json)),
        ("descendants", Json::Array(descendants)),
        ("civInfo", civ.unwrap_or(Json::Null)),
        ("sound", Json::Bool(art.history.chronicle.is_sound())),
    ]);
    Response::json(&body)
}

fn cell_endpoint(app: &Arc<App>, request: &HttpRequest) -> Response {
    let id = match request_id(request) {
        Ok(id) => id,
        Err(r) => return r,
    };
    let art = match artifact(app, &id) {
        Ok(a) => a,
        Err(r) => return r,
    };
    let cell = match query_u32(request, "cell") {
        Ok(Some(c)) => c,
        Ok(None) => {
            let lat = match request.query.get("lat").and_then(|v| v.parse::<f64>().ok()) {
                Some(v) if v.is_finite() && (-90.0..=90.0).contains(&v) => v,
                _ => return Response::error(400, "missing cell or valid lat/lon"),
            };
            let lon = match request.query.get("lon").and_then(|v| v.parse::<f64>().ok()) {
                Some(v) if v.is_finite() && (-180.0..=180.0).contains(&v) => v,
                _ => return Response::error(400, "missing cell or valid lat/lon"),
            };
            art.world.sphere.nearest_lat_lon(lat, lon)
        }
        Err(r) => return r,
    };
    let Some(info) = art.world.cell_info(cell) else {
        return Response::error(404, "cell out of range");
    };
    let year = match query_u32(request, "year") {
        Ok(y) => y.unwrap_or(art.history.config.years),
        Err(r) => return r,
    };
    let owner = art.history.snapshot_at(year).and_then(|s| s.owner_of(cell));
    let owner_json = match owner {
        Some(o) => Json::obj(vec![
            ("id", Json::num(f64::from(o))),
            (
                "name",
                Json::str(
                    art.history
                        .civs
                        .get(o as usize)
                        .map(|c| c.name.clone())
                        .unwrap_or_else(|| format!("Civilization {o}")),
                ),
            ),
        ]),
        None => Json::Null,
    };
    let events: Vec<Json> = art
        .history
        .chronicle
        .events_at_cell(cell)
        .into_iter()
        .take(20)
        .map(event_summary_json)
        .collect();
    let body = Json::obj(vec![
        ("cell", Json::num(f64::from(cell))),
        ("lat", Json::num(info.lat)),
        ("lon", Json::num(info.lon)),
        ("elevation", Json::num(info.elevation)),
        ("biome", Json::str(info.biome)),
        ("biomeLabel", Json::str(info.biome_label)),
        ("temperature", Json::num(info.temperature_c)),
        ("seasonality", Json::num(info.seasonality_c)),
        ("precip", Json::num(info.precip_mm)),
        ("aridity", Json::num(info.aridity)),
        ("humidity", Json::num(info.humidity)),
        ("plate", Json::num(f64::from(info.plate))),
        ("plateKind", Json::str(info.plate_kind)),
        ("stress", Json::str(info.stress)),
        ("uplift", Json::num(info.uplift_m)),
        ("discharge", Json::num(info.discharge_m3s)),
        ("river", Json::Bool(info.river)),
        ("lake", Json::Bool(info.lake)),
        ("delta", Json::Bool(info.delta)),
        ("npp", Json::num(info.npp_g_m2_yr)),
        ("species", Json::num(f64::from(info.species))),
        ("carryingCapacity", Json::num(info.carrying_capacity)),
        ("landmass", Json::num(f64::from(info.landmass))),
        ("crustAge", Json::num(info.crust_age_myr)),
        ("owner", owner_json),
        ("year", Json::num(f64::from(year))),
        ("events", Json::Array(events)),
    ]);
    Response::json(&body)
}

fn civs_endpoint(app: &Arc<App>, request: &HttpRequest) -> Response {
    let id = match request_id(request) {
        Ok(id) => id,
        Err(r) => return r,
    };
    let art = match artifact(app, &id) {
        Ok(a) => a,
        Err(r) => return r,
    };
    let civs: Vec<Json> = art
        .history
        .civs
        .iter()
        .map(|c| civ_json(c, &art.history))
        .collect();
    Response::json(&Json::obj(vec![
        ("civs", Json::Array(civs)),
        ("count", Json::num(art.history.civs.len() as f64)),
    ]))
}

fn civ_json(civ: &terra_history::civ::Civ, history: &terra_history::civ::History) -> Json {
    let cities: Vec<Json> = civ
        .cities
        .iter()
        .map(|city| {
            Json::obj(vec![
                ("cell", Json::num(f64::from(city.cell))),
                ("name", Json::str(city.name.clone())),
                ("population", Json::num(city.population)),
                ("since", Json::num(f64::from(city.since))),
                ("capital", Json::Bool(city.capital)),
            ])
        })
        .collect();
    let resources = Json::Object(
        civ.resources
            .iter()
            .map(|(r, n)| (r.code().to_string(), Json::num(f64::from(*n))))
            .collect(),
    );
    let wars: Vec<Json> = history
        .wars
        .iter()
        .filter(|w| w.a == civ.id || w.b == civ.id)
        .map(|w| {
            Json::obj(vec![
                ("a", Json::num(f64::from(w.a))),
                ("b", Json::num(f64::from(w.b))),
                ("start", Json::num(f64::from(w.start))),
                (
                    "end",
                    match w.end {
                        Some(e) => Json::num(f64::from(e)),
                        None => Json::Null,
                    },
                ),
                ("battles", Json::num(f64::from(w.battles))),
            ])
        })
        .collect();
    Json::obj(vec![
        ("id", Json::num(f64::from(civ.id))),
        ("name", Json::str(civ.name.clone())),
        (
            "color",
            Json::str(format_hex(render::civ_color(civ.color_index))),
        ),
        ("capital", Json::num(f64::from(civ.capital))),
        ("capitalName", Json::str(civ.capital_name.clone())),
        ("seat", Json::num(f64::from(civ.seat()))),
        ("cells", Json::num(civ.cells.len() as f64)),
        ("population", Json::num(civ.population)),
        ("tech", Json::num(f64::from(civ.tech))),
        ("cohesion", Json::num(civ.cohesion)),
        ("military", Json::num(civ.military)),
        ("treasury", Json::num(civ.treasury)),
        ("food", Json::num(civ.food)),
        ("alive", Json::Bool(civ.alive)),
        (
            "collapsedYear",
            match civ.collapsed_year {
                Some(y) => Json::num(f64::from(y)),
                None => Json::Null,
            },
        ),
        ("founded", Json::num(f64::from(civ.founded_year))),
        (
            "traits",
            Json::obj(vec![
                ("agrarian", Json::num(civ.traits.agrarian)),
                ("martial", Json::num(civ.traits.martial)),
                ("mercantile", Json::num(civ.traits.mercantile)),
                ("label", Json::str(civ.traits.describe())),
            ]),
        ),
        ("resources", resources),
        ("cities", Json::Array(cities)),
        ("wars", Json::Array(wars)),
        ("atWar", Json::Bool(civ.at_war())),
        (
            "techName",
            Json::str(
                terra_history::civ::TECHNOLOGY_NAMES
                    .get(civ.tech.saturating_sub(1) as usize)
                    .copied()
                    .unwrap_or("Stone age"),
            ),
        ),
    ])
}

fn civ_endpoint(app: &Arc<App>, request: &HttpRequest) -> Response {
    let id = match request_id(request) {
        Ok(id) => id,
        Err(r) => return r,
    };
    let art = match artifact(app, &id) {
        Ok(a) => a,
        Err(r) => return r,
    };
    let civ_id = match request.query.get("civ") {
        None => return Response::error(400, "missing civ id"),
        Some(v) => match v.parse::<u16>() {
            Ok(c) => c,
            Err(_) => return Response::error(400, "civ must be an integer"),
        },
    };
    let Some(civ) = art.history.civs.get(civ_id as usize) else {
        return Response::error(404, "unknown civilization");
    };
    let events: Vec<Json> = art
        .history
        .chronicle
        .events_for_civ(civ_id)
        .into_iter()
        .take(200)
        .map(event_summary_json)
        .collect();
    let history_json = civ_json(civ, &art.history);
    Response::json(&Json::obj(vec![
        ("civ", history_json),
        ("events", Json::Array(events)),
    ]))
}

fn timeline_endpoint(app: &Arc<App>, request: &HttpRequest) -> Response {
    let id = match request_id(request) {
        Ok(id) => id,
        Err(r) => return r,
    };
    let art = match artifact(app, &id) {
        Ok(a) => a,
        Err(r) => return r,
    };
    let density: Vec<Json> = art
        .history
        .chronicle
        .density_by_century()
        .iter()
        .enumerate()
        .map(|(i, n)| {
            Json::obj(vec![
                ("century", Json::num(i as f64)),
                ("count", Json::num(f64::from(*n))),
            ])
        })
        .collect();
    let series: Vec<Json> = art
        .history
        .snapshots
        .iter()
        .map(|s| {
            let civs: Vec<Json> = s
                .civs
                .iter()
                .map(|c| {
                    Json::obj(vec![
                        ("id", Json::num(f64::from(c.id))),
                        ("population", Json::num(c.population)),
                        ("cells", Json::num(f64::from(c.cells))),
                        ("tech", Json::num(f64::from(c.tech))),
                        ("cohesion", Json::num(c.cohesion)),
                        ("alive", Json::Bool(c.alive)),
                    ])
                })
                .collect();
            Json::obj(vec![
                ("year", Json::num(f64::from(s.year))),
                ("civs", Json::Array(civs)),
            ])
        })
        .collect();
    let wars: Vec<Json> = art
        .history
        .wars
        .iter()
        .map(|w| {
            Json::obj(vec![
                ("a", Json::num(f64::from(w.a))),
                ("b", Json::num(f64::from(w.b))),
                ("start", Json::num(f64::from(w.start))),
                (
                    "end",
                    match w.end {
                        Some(e) => Json::num(f64::from(e)),
                        None => Json::Null,
                    },
                ),
                ("battles", Json::num(f64::from(w.battles))),
                ("casualties", Json::num(w.casualties)),
                (
                    "winner",
                    match w.winner {
                        Some(v) => Json::num(f64::from(v)),
                        None => Json::Null,
                    },
                ),
            ])
        })
        .collect();
    Response::json(&Json::obj(vec![
        ("density", Json::Array(density)),
        ("series", Json::Array(series)),
        ("wars", Json::Array(wars)),
        (
            "civs",
            Json::Array(
                art.history
                    .civs
                    .iter()
                    .map(|c| {
                        Json::obj(vec![
                            ("id", Json::num(f64::from(c.id))),
                            ("name", Json::str(c.name.clone())),
                            (
                                "color",
                                Json::str(format_hex(render::civ_color(c.color_index))),
                            ),
                        ])
                    })
                    .collect(),
            ),
        ),
        ("years", Json::num(f64::from(art.history.config.years))),
    ]))
}

fn export_markdown(app: &Arc<App>, request: &HttpRequest) -> Response {
    let id = match request_id(request) {
        Ok(id) => id,
        Err(r) => return r,
    };
    let art = match artifact(app, &id) {
        Ok(a) => a,
        Err(r) => return r,
    };
    let text = chronicle_export::markdown(&art.world.name, &art.history.chronicle);
    let filename = format!("{}-chronicle.md", safe_filename(&art.world.name));
    Response::new(200, "text/markdown; charset=utf-8", text).with_header(
        "Content-Disposition",
        format!("attachment; filename=\"{filename}\""),
    )
}

fn export_json(app: &Arc<App>, request: &HttpRequest) -> Response {
    let id = match request_id(request) {
        Ok(id) => id,
        Err(r) => return r,
    };
    let art = match artifact(app, &id) {
        Ok(a) => a,
        Err(r) => return r,
    };
    let text = chronicle_export::json(&art.world.name, &art.history.chronicle);
    let filename = format!("{}-chronicle.json", safe_filename(&art.world.name));
    Response::new(200, "application/json; charset=utf-8", text).with_header(
        "Content-Disposition",
        format!("attachment; filename=\"{filename}\""),
    )
}

fn export_csv(app: &Arc<App>, request: &HttpRequest) -> Response {
    let id = match request_id(request) {
        Ok(id) => id,
        Err(r) => return r,
    };
    let art = match artifact(app, &id) {
        Ok(a) => a,
        Err(r) => return r,
    };
    let text = chronicle_export::csv(&art.world.name, &art.history.chronicle);
    let filename = format!("{}-chronicle.csv", safe_filename(&art.world.name));
    Response::new(200, "text/csv; charset=utf-8", text).with_header(
        "Content-Disposition",
        format!("attachment; filename=\"{filename}\""),
    )
}

fn export_world(app: &Arc<App>, request: &HttpRequest) -> Response {
    let id = match request_id(request) {
        Ok(id) => id,
        Err(r) => return r,
    };
    let name = match artifact(app, &id) {
        Ok(a) => a.world.name.clone(),
        Err(r) => return r,
    };
    let mut response = world_endpoint(app, request);
    response.headers.push((
        "Content-Disposition",
        format!(
            "attachment; filename=\"{}-world.json\"",
            safe_filename(&name)
        ),
    ));
    response
}

fn safe_filename(name: &str) -> String {
    let mut out: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    out = out.trim_matches('-').to_string();
    if out.is_empty() {
        out.push_str("terra");
    }
    out.chars().take(48).collect()
}

fn format_hex(color: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", color[0], color[1], color[2])
}

fn stream_sse(stream: &mut TcpStream, app: &Arc<App>, request: &HttpRequest) {
    let id = request.query.get("id").cloned().unwrap_or_default();
    let head = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream; charset=utf-8\r\nCache-Control: no-store\r\nConnection: close\r\nX-Accel-Buffering: no\r\n\r\n";
    if stream.write_all(head.as_bytes()).is_err() {
        return;
    }
    let _ = stream.write_all(b"retry: 1500\n\n");
    let mut last_progress = -1.0f64;
    let deadline = Instant::now() + Duration::from_secs(600);
    loop {
        if Instant::now() > deadline {
            break;
        }
        let snapshot = {
            let jobs = app.jobs.lock().unwrap_or_else(|e| e.into_inner());
            jobs.map.get(&id).map(|j| {
                (
                    j.status,
                    j.stage.clone(),
                    j.progress,
                    j.error.clone(),
                    j.world.is_some(),
                )
            })
        };
        let Some((status, stage, progress, error, done)) = snapshot else {
            let _ = stream.write_all(b"event: error\ndata: {\"error\":\"unknown job\"}\n\n");
            break;
        };
        if (progress - last_progress).abs() > 0.001 || status != JobStatus::Running {
            last_progress = progress;
            let payload = json::Json::obj(vec![
                ("id", json::Json::str(id.clone())),
                ("status", json::Json::str(status.code())),
                ("stage", json::Json::str(stage)),
                ("progress", json::Json::num(progress)),
                (
                    "error",
                    match error {
                        Some(e) => json::Json::str(e),
                        None => json::Json::Null,
                    },
                ),
            ]);
            let frame = format!("event: progress\ndata: {}\n\n", payload.to_string());
            if stream.write_all(frame.as_bytes()).is_err() {
                return;
            }
        }
        if done && status == JobStatus::Done {
            let payload = json::Json::obj(vec![
                ("id", json::Json::str(id.clone())),
                ("progress", json::Json::num(1.0)),
            ]);
            let _ = stream.write_all(
                format!("event: complete\ndata: {}\n\n", payload.to_string()).as_bytes(),
            );
            return;
        }
        if status == JobStatus::Failed {
            return;
        }
        std::thread::sleep(Duration::from_millis(90));
    }
}

/// Generates a random URL token and returns it. Used by the launcher.
pub fn startup_token() -> String {
    random_token()
}

/// Access to the listener address for tests and the launcher.
pub fn local_url(addr: SocketAddr, token: &str) -> String {
    format!("http://127.0.0.1:{}/?t={}", addr.port(), token)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn get(addr: SocketAddr, path: &str) -> (u16, String, Vec<u8>) {
        let mut stream = TcpStream::connect(addr).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let req = format!("GET {path} HTTP/1.1\r\nHost: localhost\r\n\r\n");
        stream.write_all(req.as_bytes()).unwrap();
        read_response(&mut stream)
    }

    fn read_response(stream: &mut TcpStream) -> (u16, String, Vec<u8>) {
        let mut raw = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            match stream.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    raw.extend_from_slice(&buf[..n]);
                    if let Some(pos) = find_header_end(&raw) {
                        let head = String::from_utf8_lossy(&raw[..pos]).to_string();
                        let len = head
                            .lines()
                            .find_map(|l| {
                                l.to_ascii_lowercase()
                                    .strip_prefix("content-length:")
                                    .map(|v| v.trim().parse::<usize>().unwrap_or(0))
                            })
                            .unwrap_or(0);
                        if raw.len() >= pos + 4 + len {
                            return (status_of(&head), head, raw[pos + 4..].to_vec());
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

    fn test_server() -> (Server, SocketAddr) {
        let server = start(ServerConfig {
            port: 0,
            token: "test-token".into(),
        })
        .unwrap();
        let addr = server.addr;
        (server, addr)
    }

    #[test]
    fn token_is_hex_and_unique() {
        let a = random_token();
        let b = random_token();
        assert_eq!(a.len(), 32);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
        assert_eq!(a, a.to_lowercase());
    }

    #[test]
    fn index_and_assets_are_served() {
        let (_server, addr) = test_server();
        let (status, head, body) = get(addr, "/");
        assert_eq!(status, 200);
        assert!(head.contains("text/html"));
        assert!(String::from_utf8_lossy(&body).contains("<title>Terra"));
        let (status, head, body) = get(addr, "/app.css");
        assert_eq!(status, 200);
        assert!(head.contains("text/css"));
        assert!(!body.is_empty());
        let (status, _, body) = get(addr, "/app.js");
        assert_eq!(status, 200);
        assert!(String::from_utf8_lossy(&body).contains("Terra"));
        let (status, head, _) = get(addr, "/favicon.svg");
        assert_eq!(status, 200);
        assert!(head.contains("svg"));
    }

    #[test]
    fn api_requires_a_token() {
        let (_server, addr) = test_server();
        let (status, _, body) = get(addr, "/api/health");
        assert_eq!(status, 401);
        assert!(String::from_utf8_lossy(&body).contains("token"));
        let (status, _, _body) = get(addr, "/api/health?t=wrong");
        assert_eq!(status, 401);
        let (status, _, body) = get(addr, "/api/health?t=test-token");
        assert_eq!(status, 200);
        let value = json::parse(&String::from_utf8_lossy(&body)).unwrap();
        assert_eq!(value.get("ok").unwrap().as_bool(), Some(true));
        assert_eq!(value.get("version").unwrap().as_str(), Some(crate::VERSION));
    }

    #[test]
    fn token_header_is_accepted() {
        let (_server, addr) = test_server();
        let mut stream = TcpStream::connect(addr).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let req = "GET /api/health HTTP/1.1\r\nHost: localhost\r\nX-Token: test-token\r\n\r\n";
        stream.write_all(req.as_bytes()).unwrap();
        let (status, _, _) = read_response(&mut stream);
        assert_eq!(status, 200);
        let mut stream = TcpStream::connect(addr).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let req = "GET /api/health HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer test-token\r\n\r\n";
        stream.write_all(req.as_bytes()).unwrap();
        let (status, _, _) = read_response(&mut stream);
        assert_eq!(status, 200);
    }

    #[test]
    fn unknown_routes_and_traversal_fail() {
        let (_server, addr) = test_server();
        assert_eq!(get(addr, "/nope?t=test-token").0, 404);
        assert_eq!(get(addr, "/../etc/passwd").0, 400);
        assert_eq!(get(addr, "/%2e%2e/etc/passwd").0, 400);
        assert_eq!(get(addr, "/api/nope?t=test-token").0, 404);
    }

    #[test]
    fn malformed_requests_fail_cleanly() {
        let (_server, addr) = test_server();
        let mut stream = TcpStream::connect(addr).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        stream.write_all(b"NOT A REQUEST\r\n\r\n").unwrap();
        let (status, _, _) = read_response(&mut stream);
        assert_eq!(status, 400);

        let mut stream = TcpStream::connect(addr).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        stream
            .write_all(b"GET / HTTP/1.1\r\nHost: x\r\nContent-Length: 999999\r\n\r\n")
            .unwrap();
        let (status, _, _) = read_response(&mut stream);
        assert_eq!(status, 413);
    }

    fn post_json(addr: SocketAddr, path: &str, body: &str) -> (u16, Vec<u8>) {
        let mut stream = TcpStream::connect(addr).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let req = format!(
            "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(req.as_bytes()).unwrap();
        let (status, _, body) = read_response(&mut stream);
        (status, body)
    }

    #[test]
    fn full_run_produces_a_world_and_streams_progress() {
        let (_server, addr) = test_server();
        let (status, body) = post_json(
            addr,
            "/api/run?t=test-token",
            "{\"seed\":3,\"years\":200,\"subdiv\":2,\"civs\":2}",
        );
        assert_eq!(status, 202);
        let value = json::parse(&String::from_utf8_lossy(&body)).unwrap();
        let id = value.get("id").unwrap().as_str().unwrap().to_string();
        assert!(id.starts_with('w'));

        let mut done = None;
        for _ in 0..200 {
            std::thread::sleep(Duration::from_millis(25));
            let (status, _, body) = get(addr, &format!("/api/job?id={id}&t=test-token"));
            assert_eq!(status, 200);
            let v = json::parse(&String::from_utf8_lossy(&body)).unwrap();
            let state = v.get("status").unwrap().as_str().unwrap();
            if state == "done" || state == "failed" {
                done = Some(v);
                break;
            }
        }
        let done = done.expect("job did not finish");
        assert_eq!(done.get("status").unwrap().as_str(), Some("done"));
        let world = done.get("world").unwrap();
        assert!(!world.get("planet").unwrap().as_str().unwrap().is_empty());
        assert!(world.get("events").unwrap().as_u64().unwrap() > 0);

        // SSE replays the final state and closes with a complete event.
        let mut stream = TcpStream::connect(addr).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let req = format!("GET /api/events?id={id}&t=test-token HTTP/1.1\r\nHost: x\r\n\r\n");
        stream.write_all(req.as_bytes()).unwrap();
        let mut raw = String::new();
        let mut buf = [0u8; 4096];
        for _ in 0..200 {
            match stream.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    raw.push_str(&String::from_utf8_lossy(&buf[..n]));
                    if raw.contains("event: complete") || raw.contains("event: error") {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
        assert!(raw.contains("text/event-stream"));
        assert!(raw.contains("event: complete"), "{raw}");
    }

    fn start_small(addr: SocketAddr) -> String {
        let (status, body) = post_json(
            addr,
            "/api/run?t=test-token",
            "{\"seed\":5,\"years\":300,\"subdiv\":2,\"civs\":3}",
        );
        assert_eq!(status, 202);
        let value = json::parse(&String::from_utf8_lossy(&body)).unwrap();
        let id = value.get("id").unwrap().as_str().unwrap().to_string();
        for _ in 0..200 {
            std::thread::sleep(Duration::from_millis(25));
            let (_, _, body) = get(addr, &format!("/api/job?id={id}&t=test-token"));
            let v = json::parse(&String::from_utf8_lossy(&body)).unwrap();
            match v.get("status").unwrap().as_str() {
                Some("done") => return id,
                Some("failed") => panic!("job failed: {body:?}"),
                _ => {}
            }
        }
        panic!("job did not finish");
    }

    #[test]
    fn world_and_map_endpoints_work() {
        let (_server, addr) = test_server();
        let id = start_small(addr);
        let (status, _, body) = get(addr, &format!("/api/world?id={id}&t=test-token"));
        assert_eq!(status, 200);
        let text = String::from_utf8_lossy(&body);
        let v = json::parse(&text).unwrap();
        assert!(v.get("planet").unwrap().as_str().unwrap().len() > 2);
        assert!(v.get("layers").unwrap().as_array().unwrap().len() >= 10);
        assert!(v.get("stats").unwrap().get("plates").is_some());
        assert!(v.get("history").unwrap().get("sound").unwrap().as_bool() == Some(true));

        for layer in [
            "atlas",
            "elevation",
            "plates",
            "biomes",
            "temperature",
            "precipitation",
            "rivers",
            "political",
            "biodiversity",
            "stress",
        ] {
            let (status, head, body) = get(
                addr,
                &format!("/api/map?id={id}&layer={layer}&w=200&h=100&t=test-token"),
            );
            assert_eq!(status, 200, "{layer}");
            assert!(head.contains("image/png"));
            assert_eq!(&body[..8], &crate::png::PNG_SIGNATURE);
            assert!(body.len() > 200);
        }
        assert_eq!(
            get(addr, &format!("/api/map?id={id}&layer=nope&t=test-token")).0,
            400
        );
        assert_eq!(
            get(
                addr,
                &format!("/api/map?id={id}&layer=atlas&w=999999&t=test-token")
            )
            .0,
            200
        );
    }

    #[test]
    fn chronicle_cell_civs_timeline_and_exports() {
        let (_server, addr) = test_server();
        let id = start_small(addr);
        let (status, _, body) = get(
            addr,
            &format!("/api/chronicle?id={id}&limit=10&t=test-token"),
        );
        assert_eq!(status, 200);
        let v = json::parse(&String::from_utf8_lossy(&body)).unwrap();
        assert!(v.get("total").unwrap().as_u64().unwrap() >= 4);
        let events = v.get("events").unwrap().as_array().unwrap();
        assert_eq!(events.len(), 10);
        let event_id = events[0].get("id").unwrap().as_u64().unwrap();
        let cell = events[0].get("cell");
        let (status, _, body) = get(
            addr,
            &format!("/api/event?id={id}&event={event_id}&t=test-token"),
        );
        assert_eq!(status, 200);
        let detail = json::parse(&String::from_utf8_lossy(&body)).unwrap();
        assert!(!detail.get("causes").unwrap().as_array().unwrap().is_empty());
        assert!(detail.get("chain").unwrap().get("nodes").is_some());
        assert!(detail.get("explain").unwrap().as_str().unwrap().len() > 20);

        if let Some(cell) = cell.and_then(|c| c.as_u64()) {
            let (status, _, body) =
                get(addr, &format!("/api/cell?id={id}&cell={cell}&t=test-token"));
            assert_eq!(status, 200);
            let v = json::parse(&String::from_utf8_lossy(&body)).unwrap();
            assert!(v.get("biome").unwrap().as_str().is_some());
            assert!(v.get("elevation").unwrap().as_f64().is_some());
        }

        let (status, _, body) = get(addr, &format!("/api/civs?id={id}&t=test-token"));
        assert_eq!(status, 200);
        let v = json::parse(&String::from_utf8_lossy(&body)).unwrap();
        assert!(v.get("count").unwrap().as_u64().unwrap() >= 2);
        let civ_id = v
            .get("civs")
            .unwrap()
            .index(0)
            .unwrap()
            .get("id")
            .unwrap()
            .as_u64()
            .unwrap();
        let (status, _, body) = get(addr, &format!("/api/civ?id={id}&civ={civ_id}&t=test-token"));
        assert_eq!(status, 200);
        let v = json::parse(&String::from_utf8_lossy(&body)).unwrap();
        assert!(v.get("civ").unwrap().get("name").is_some());

        let (status, _, body) = get(addr, &format!("/api/timeline?id={id}&t=test-token"));
        assert_eq!(status, 200);
        let v = json::parse(&String::from_utf8_lossy(&body)).unwrap();
        assert!(!v.get("series").unwrap().as_array().unwrap().is_empty());

        let (status, head, body) = get(
            addr,
            &format!("/api/export/chronicle.md?id={id}&t=test-token"),
        );
        assert_eq!(status, 200);
        assert!(head.contains("text/markdown"));
        assert!(String::from_utf8_lossy(&body).contains("# Chronicle of"));
        let (status, _, body) = get(
            addr,
            &format!("/api/export/chronicle.json?id={id}&t=test-token"),
        );
        assert_eq!(status, 200);
        let parsed = json::parse(&String::from_utf8_lossy(&body)).unwrap();
        assert_eq!(parsed.get("sound").unwrap().as_bool(), Some(true));

        let (status, head, body) = get(
            addr,
            &format!("/api/export/chronicle.csv?id={id}&t=test-token"),
        );
        assert_eq!(status, 200);
        assert!(head.contains("text/csv"));
        let csv = String::from_utf8_lossy(&body);
        assert!(csv.contains("id,year,kind"));
        assert!(csv.lines().count() >= 4);

        let (status, head, body) = get(
            addr,
            &format!("/api/export/world.json?id={id}&t=test-token"),
        );
        assert_eq!(status, 200);
        assert!(head.contains("application/json"));
        assert!(head.to_lowercase().contains("attachment"));
        let world = json::parse(&String::from_utf8_lossy(&body)).unwrap();
        assert_eq!(world.get("sound"), None);
        assert!(world.get("hashes").unwrap().get("chronicle").is_some());
    }

    #[test]
    fn bad_queries_fail_cleanly() {
        let (_server, addr) = test_server();
        let id = start_small(addr);
        for query in [
            "id=missing&t=test-token",
            "id=missing&layer=atlas&t=test-token",
        ] {
            let (status, _, _) = get(addr, &format!("/api/world?{query}"));
            assert_eq!(status, 404);
        }
        assert_eq!(
            get(addr, &format!("/api/chronicle?id={id}&from=x&t=test-token")).0,
            400
        );
        assert_eq!(
            get(
                addr,
                &format!("/api/event?id={id}&event=999999&t=test-token")
            )
            .0,
            404
        );
        assert_eq!(
            get(addr, &format!("/api/cell?id={id}&cell=999999&t=test-token")).0,
            404
        );
    }

    #[test]
    fn percent_decoding_is_bounded() {
        assert_eq!(percent_decode("a%20b+c").unwrap(), "a b c");
        assert!(percent_decode("%zz").is_none());
        assert!(percent_decode("%2").is_none());
        assert_eq!(percent_decode("plain").unwrap(), "plain");
    }

    #[test]
    fn constant_time_compare() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"ab"));
        assert!(!constant_time_eq(b"", b"a"));
    }

    #[test]
    fn run_request_is_sanitized() {
        let req = RunRequest {
            seed: Some(u64::MAX),
            years: Some(1),
            subdiv: Some(99),
            initial_civs: Some(0),
            aggression: Some(f64::NAN),
            expansion: Some(99.0),
            preset: None,
            scenario: None,
        };
        let (world, history) = req.resolve().unwrap();
        assert!(world.seed <= u64::from(u32::MAX));
        assert_eq!(world.years, 100);
        assert_eq!(world.subdiv, 5);
        assert_eq!(history.initial_civs, 2);
        assert_eq!(history.aggression, 1.0);
        assert_eq!(history.expansion, 3.0);
    }

    #[test]
    fn run_request_rejects_bad_fields() {
        for body in [
            "{\"seed\":-1}",
            "{\"seed\":1.5}",
            "{\"unknown\":1}",
            "{\"scenario\":5}",
            "not json",
        ] {
            let parsed = super::json::parse(body);
            if let Ok(v) = parsed {
                assert!(RunRequest::from_json(&v).is_err(), "{body}");
            }
        }
        let req = RunRequest {
            preset: Some("nope".into()),
            ..RunRequest::default()
        };
        assert!(req.resolve().is_err());
    }
}
