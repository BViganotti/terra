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
use terra_core::world::{World, WorldConfig};
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

/// Launches the Studio server and opens the browser. Implemented with the
/// launcher milestone; the scaffold returns a clear error if called early.
pub fn launch() -> std::io::Result<()> {
    let server = start(ServerConfig::default())?;
    println!("Terra Studio: {}", local_url(server.addr, &server.token));
    open_browser(&local_url(server.addr, &server.token));
    loop {
        std::thread::park();
    }
}

fn open_browser(url: &str) {
    let candidates: &[(&str, &[&str])] = if cfg!(target_os = "macos") {
        &[("open", &[url])]
    } else if cfg!(target_os = "windows") {
        &[("cmd", &["/C", "start", "", url])]
    } else {
        &[("xdg-open", &[url]), ("gio", &["open", url])]
    };
    for (program, args) in candidates {
        if std::process::Command::new(program)
            .args(*args)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .is_ok()
        {
            return;
        }
    }
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
