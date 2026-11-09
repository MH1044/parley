#![allow(dead_code)]
//! A small HTTP/1.1 server for the module 06 tests (docs/modules/06-publik-and-providers.md,
//! Acceptance tests), standing in for publik API, the learner's own provider and a local model
//! server. No test reaches the network.
//!
//! It listens on 127.0.0.1 on a free port and serves every connection on its own thread, using
//! only `std::net`. It records every request (method, path, lower-cased headers and body bytes)
//! and answers from a script: per method and path, a queue of replies (status, headers, body and an
//! optional delay before answering), whose last reply repeats. A request nothing is scripted for
//! gets a 404. Every response carries a `Content-Length` and `Connection: close`, so each request
//! is one connection.
//!
//! The other test files reuse it with `#[path = ".../crates/parley-publik/tests/fake.rs"] mod
//! fake;`. Cargo also builds this file as a test target of its own, which holds no tests.

use std::collections::{BTreeMap, VecDeque};
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::Duration;

use serde_json::{Value, json};

/// One request as the server received it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub method: String,
    /// The request target as sent, query included (`/chat/completions`).
    pub path: String,
    /// Header names lower-cased, values trimmed, in the order received.
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Request {
    /// The first value of a header, by name in any case.
    pub fn header(&self, name: &str) -> Option<&str> {
        let name = name.to_ascii_lowercase();
        self.headers
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, v)| v.as_str())
    }

    /// The path without its query.
    pub fn route(&self) -> &str {
        self.path.split('?').next().unwrap_or(&self.path)
    }

    /// The body as JSON; panics with the request named when it is not.
    pub fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or_else(|e| {
            panic!("the body of {} {} is not JSON: {e}", self.method, self.path)
        })
    }

    /// The body as text (lossy UTF-8).
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    /// The request line, every header and the body as one text, for scans.
    pub fn everything(&self) -> String {
        let mut all = format!("{} {}\n", self.method, self.path);
        for (name, value) in &self.headers {
            all.push_str(name);
            all.push_str(": ");
            all.push_str(value);
            all.push('\n');
        }
        all.push_str(&self.text());
        all
    }
}

/// One scripted answer.
#[derive(Debug, Clone, PartialEq)]
pub struct Reply {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    /// How long the server waits after reading the request and before answering.
    pub delay: Duration,
}

impl Reply {
    /// A reply with this status and an empty body.
    pub fn status(status: u16) -> Reply {
        Reply {
            status,
            headers: Vec::new(),
            body: Vec::new(),
            delay: Duration::ZERO,
        }
    }

    /// A JSON reply.
    pub fn json(status: u16, body: &Value) -> Reply {
        Reply::status(status).with_json(body)
    }

    /// A reply of raw bytes with a content type.
    pub fn bytes(status: u16, content_type: &str, body: &[u8]) -> Reply {
        let mut reply = Reply::status(status).header("content-type", content_type);
        reply.body = body.to_vec();
        reply
    }

    /// A 200 OpenAI chat completion whose one choice says `content`.
    pub fn chat(content: &str) -> Reply {
        Reply::status(200).with_chat(content)
    }

    /// A 200 server-sent event stream of OpenAI chat completion chunks, one per delta, then a
    /// final chunk with `finish_reason` and `data: [DONE]`.
    pub fn sse(deltas: &[&str]) -> Reply {
        let mut text = String::new();
        for delta in deltas {
            let chunk = json!({
                "id": "chatcmpl-stream",
                "object": "chat.completion.chunk",
                "created": 1_762_387_200,
                "model": "publik-balanced",
                "choices": [{"index": 0, "delta": {"content": delta}, "finish_reason": null}],
            });
            text.push_str(&format!("data: {chunk}\n\n"));
        }
        let last = json!({
            "id": "chatcmpl-stream",
            "object": "chat.completion.chunk",
            "created": 1_762_387_200,
            "model": "publik-balanced",
            "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}],
        });
        text.push_str(&format!("data: {last}\n\ndata: [DONE]\n\n"));
        Reply::bytes(200, "text/event-stream", text.as_bytes())
    }

    /// A reply read from `tests/fixtures/<relative>`, a JSON file `{"status", "headers", "body"}`
    /// in which every `{base_url}` becomes `base_url`. A string body is sent as it is, a null body
    /// as nothing, any other body as JSON.
    pub fn fixture(relative: &str, base_url: &str) -> Reply {
        let path = repo_root().join("tests").join("fixtures").join(relative);
        let text = fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("{} could not be read: {e}", path.display()));
        let text = text.replace("{base_url}", base_url);
        let value: Value = serde_json::from_str(&text)
            .unwrap_or_else(|e| panic!("{} is not JSON: {e}", path.display()));
        let status = value["status"]
            .as_u64()
            .and_then(|s| u16::try_from(s).ok())
            .unwrap_or_else(|| panic!("{} has no status", path.display()));
        let mut reply = Reply::status(status);
        if let Some(headers) = value["headers"].as_object() {
            for (name, header) in headers {
                let header = header
                    .as_str()
                    .unwrap_or_else(|| panic!("{}: header {name} is not text", path.display()));
                reply = reply.header(name, header);
            }
        }
        reply.body = match &value["body"] {
            Value::Null => Vec::new(),
            Value::String(text) => text.clone().into_bytes(),
            body => serde_json::to_vec(body).expect("a JSON body serialises"),
        };
        reply
    }

    /// The same reply with a header set (replacing one of the same name).
    pub fn header(mut self, name: &str, value: &str) -> Reply {
        let name = name.to_ascii_lowercase();
        self.headers.retain(|(n, _)| *n != name);
        self.headers.push((name, value.to_string()));
        self
    }

    /// The same reply with a JSON body.
    pub fn with_json(self, body: &Value) -> Reply {
        let mut reply = self.header("content-type", "application/json");
        reply.body = serde_json::to_vec(body).expect("a JSON body serialises");
        reply
    }

    /// The same reply (status and headers kept) with a chat completion body saying `content`.
    pub fn with_chat(self, content: &str) -> Reply {
        self.with_json(&chat_completion(content))
    }

    /// The same reply, sent only after `delay`.
    pub fn after(mut self, delay: Duration) -> Reply {
        self.delay = delay;
        self
    }
}

/// An OpenAI chat completion body with one choice saying `content`.
pub fn chat_completion(content: &str) -> Value {
    json!({
        "id": "chatcmpl-test",
        "object": "chat.completion",
        "created": 1_762_387_200,
        "model": "publik-fast",
        "choices": [{
            "index": 0,
            "message": {"role": "assistant", "content": content},
            "finish_reason": "stop",
        }],
        "usage": {"prompt_tokens": 30, "completion_tokens": 5, "total_tokens": 35},
    })
}

#[derive(Default)]
struct Script {
    routes: BTreeMap<(String, String), VecDeque<Reply>>,
    requests: Vec<Request>,
    connections: usize,
}

fn lock(script: &Mutex<Script>) -> MutexGuard<'_, Script> {
    script
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The fake server. It runs until the test process ends.
pub struct Fake {
    addr: SocketAddr,
    script: Arc<Mutex<Script>>,
}

impl Fake {
    /// Starts a server on 127.0.0.1 on a free port.
    pub fn start() -> Fake {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a free port on 127.0.0.1");
        let addr = listener.local_addr().expect("the listener's address");
        let script = Arc::new(Mutex::new(Script::default()));
        let shared = Arc::clone(&script);
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                let shared = Arc::clone(&shared);
                thread::spawn(move || serve(stream, &shared));
            }
        });
        Fake { addr, script }
    }

    /// `http://127.0.0.1:<port>`, with no trailing slash.
    pub fn url(&self) -> String {
        format!("http://{}", self.addr)
    }

    pub fn port(&self) -> u16 {
        self.addr.port()
    }

    /// Adds a reply to the queue of `method path` (path without query). Replies are used in
    /// order; the last one repeats.
    pub fn on(&self, method: &str, path: &str, reply: Reply) -> &Fake {
        lock(&self.script)
            .routes
            .entry((method.to_string(), path.to_string()))
            .or_default()
            .push_back(reply);
        self
    }

    /// Every request received so far, in order.
    pub fn requests(&self) -> Vec<Request> {
        lock(&self.script).requests.clone()
    }

    /// The requests to `method path` (path without query).
    pub fn requests_to(&self, method: &str, path: &str) -> Vec<Request> {
        self.requests()
            .into_iter()
            .filter(|r| r.method == method && r.route() == path)
            .collect()
    }

    /// How many requests were received.
    pub fn count(&self) -> usize {
        lock(&self.script).requests.len()
    }

    /// How many requests to `method path` were received.
    pub fn count_of(&self, method: &str, path: &str) -> usize {
        self.requests_to(method, path).len()
    }

    /// How many connections were accepted, including any that sent no complete request.
    pub fn connections(&self) -> usize {
        lock(&self.script).connections
    }

    /// Forgets the requests and connections recorded so far; the script stays.
    pub fn forget(&self) {
        let mut script = lock(&self.script);
        script.requests.clear();
        script.connections = 0;
    }
}

/// An address where nothing listens: a port that was free a moment ago.
pub fn closed_url() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a free port on 127.0.0.1");
    let addr = listener.local_addr().expect("the listener's address");
    drop(listener);
    format!("http://{addr}")
}

/// The repository root: the first ancestor of the crate under test holding the workspace
/// Cargo.toml.
pub fn repo_root() -> PathBuf {
    let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    loop {
        if let Ok(text) = fs::read_to_string(dir.join("Cargo.toml"))
            && text.contains("[workspace]")
        {
            return dir;
        }
        assert!(dir.pop(), "no workspace Cargo.toml above the crate");
    }
}

fn serve(stream: TcpStream, script: &Mutex<Script>) {
    lock(script).connections += 1;
    let _ = stream.set_read_timeout(Some(Duration::from_secs(20)));
    let Ok(reading) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(reading);
    let Some(request) = read_request(&mut reader) else {
        return;
    };
    let reply = {
        let mut script = lock(script);
        script.requests.push(request.clone());
        let key = (request.method.clone(), request.route().to_string());
        match script.routes.get_mut(&key) {
            Some(queue) if queue.len() > 1 => queue.pop_front().expect("a queued reply"),
            Some(queue) if !queue.is_empty() => queue[0].clone(),
            _ => Reply::json(
                404,
                &json!({"error": {
                    "type": "not_found",
                    "message": format!("no reply scripted for {} {}", request.method, request.path),
                }}),
            ),
        }
    };
    if !reply.delay.is_zero() {
        thread::sleep(reply.delay);
    }
    let mut stream = stream;
    if write_reply(&mut stream, &reply).is_ok() {
        let _ = stream.flush();
        let _ = stream.shutdown(Shutdown::Write);
        // Wait for the client to close its side, so the close is clean on every platform.
        let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
        let mut rest = Vec::new();
        let _ = reader.read_to_end(&mut rest);
    }
}

fn read_request(reader: &mut BufReader<TcpStream>) -> Option<Request> {
    let mut line = String::new();
    if reader.read_line(&mut line).ok()? == 0 {
        return None;
    }
    let mut parts = line.trim_end().splitn(3, ' ');
    let method = parts.next()?.to_string();
    let path = parts.next()?.to_string();
    let mut headers = Vec::new();
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header).ok()? == 0 {
            return None;
        }
        let header = header.trim_end_matches(['\r', '\n']);
        if header.is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':') {
            headers.push((name.trim().to_ascii_lowercase(), value.trim().to_string()));
        }
    }
    let value_of = |name: &str| {
        headers
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.to_ascii_lowercase())
    };
    let chunked = value_of("transfer-encoding").is_some_and(|v| v.contains("chunked"));
    let length: usize = value_of("content-length")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let body = if chunked {
        read_chunked(reader)?
    } else {
        let mut body = vec![0; length];
        reader.read_exact(&mut body).ok()?;
        body
    };
    Some(Request {
        method,
        path,
        headers,
        body,
    })
}

fn read_chunked(reader: &mut BufReader<TcpStream>) -> Option<Vec<u8>> {
    let mut body = Vec::new();
    loop {
        let mut size_line = String::new();
        if reader.read_line(&mut size_line).ok()? == 0 {
            return None;
        }
        let size_text = size_line.trim().split(';').next()?.trim();
        let size = usize::from_str_radix(size_text, 16).ok()?;
        if size == 0 {
            loop {
                let mut trailer = String::new();
                if reader.read_line(&mut trailer).ok()? == 0 || trailer.trim().is_empty() {
                    return Some(body);
                }
            }
        }
        let mut chunk = vec![0; size];
        reader.read_exact(&mut chunk).ok()?;
        body.extend_from_slice(&chunk);
        let mut line_end = [0u8; 2];
        reader.read_exact(&mut line_end).ok()?;
    }
}

fn write_reply(stream: &mut TcpStream, reply: &Reply) -> std::io::Result<()> {
    let mut head = format!("HTTP/1.1 {} {}\r\n", reply.status, reason(reply.status));
    for (name, value) in &reply.headers {
        head.push_str(name);
        head.push_str(": ");
        head.push_str(value);
        head.push_str("\r\n");
    }
    head.push_str(&format!(
        "content-length: {}\r\nconnection: close\r\n\r\n",
        reply.body.len()
    ));
    stream.write_all(head.as_bytes())?;
    stream.write_all(&reply.body)
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        402 => "Payment Required",
        403 => "Forbidden",
        404 => "Not Found",
        408 => "Request Timeout",
        413 => "Payload Too Large",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        _ => "Unknown",
    }
}
