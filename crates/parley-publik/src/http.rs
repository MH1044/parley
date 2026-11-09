//! The HTTP transport (docs/PUBLIK-INTEGRATION.md §2, adaptation 3): reqwest with rustls on every
//! platform, in place of the reference module's `curl`, plus server-sent events for streamed chat.
//!
//! [`HttpNet`] is the desktop app's real [`Net`] (pack downloads), and the providers send every AI
//! request through [`send`] and [`send_events`].
//!
//! Every request runs on a plain thread of its own. reqwest's blocking client must not run inside
//! an async runtime, and the desktop app's async commands run on one; a plain thread has no
//! runtime, whoever calls. The key, when there is one, rides only in a request header, which no
//! debug form prints (invariant I1). No body of [`MAX_REQUEST_BYTES`] or more is ever sent (I10).
//! Requests to this computer (`localhost`, `127.0.0.1`, `[::1]`) never go through a proxy, so a
//! local model's traffic stays on the machine.

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::net::IpAddr;
use std::sync::mpsc;
use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::Duration;

use parley_core::ai::MAX_REQUEST_BYTES;
use parley_core::env::{HttpRequest, HttpResponse, Net, NetError};
use reqwest::blocking::{Client, Response};
use serde_json::Value;

/// How long the connection itself may take at most.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// How long the connection of a request with this time limit may take: three quarters of the
/// limit, at most [`CONNECT_TIMEOUT`]. A server that cannot be reached within it is offline, not
/// slow: the request fails as a connection error before its own limit runs out (on Windows a
/// refused connection takes about two seconds to say so).
fn connect_timeout(timeout: Duration) -> Duration {
    (timeout * 3 / 4).clamp(Duration::from_millis(1), CONNECT_TIMEOUT)
}

/// How long a pack download may take in all.
pub const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// The `User-Agent` of every request.
fn user_agent() -> String {
    format!("Parley/{}", parley_core::api::VERSION)
}

/// A client through the system's proxy settings, or one that never uses a proxy, for a connection
/// time limit. Each is built once, on first use, and never dropped (dropping a blocking client
/// inside an async runtime panics); a clone shares it.
fn client(direct: bool, connect: Duration) -> Result<Client, NetError> {
    static CLIENTS: OnceLock<Mutex<HashMap<(bool, u128), Client>>> = OnceLock::new();
    let mut clients = CLIENTS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    if let Some(client) = clients.get(&(direct, connect.as_millis())) {
        return Ok(client.clone());
    }
    // ring is the one crypto provider in the build; installing it twice is harmless.
    let _ = rustls::crypto::ring::default_provider().install_default();
    let mut builder = Client::builder()
        .connect_timeout(connect)
        .timeout(None)
        .user_agent(user_agent());
    if direct {
        builder = builder.no_proxy();
    }
    let client = builder
        .build()
        .map_err(|e| NetError::Transport(e.without_url().to_string()))?;
    clients.insert((direct, connect.as_millis()), client.clone());
    Ok(client)
}

/// The URL's host is this computer.
pub fn is_loopback(url: &str) -> bool {
    let Ok(parsed) = reqwest::Url::parse(url) else {
        return false;
    };
    match parsed.host_str() {
        Some("localhost") => true,
        Some(host) => host
            .trim_start_matches('[')
            .trim_end_matches(']')
            .parse::<IpAddr>()
            .is_ok_and(|ip| ip.is_loopback()),
        None => false,
    }
}

/// A connection that was never made (refused, unreachable, or not made within its time limit) is
/// offline; a request that got no answer in time after connecting timed out.
fn net_error(e: &reqwest::Error) -> NetError {
    if e.is_connect() {
        NetError::Offline
    } else if e.is_timeout() {
        NetError::Timeout
    } else {
        // Without the URL: an error message never needs it, and logs never carry it.
        NetError::Transport(e.to_string())
    }
}

/// Runs `f` on a plain thread and waits for it.
fn on_plain_thread<T: Send>(f: impl FnOnce() -> T + Send) -> T {
    std::thread::scope(|scope| match scope.spawn(f).join() {
        Ok(value) => value,
        Err(panic) => std::panic::resume_unwind(panic),
    })
}

fn start(req: &HttpRequest, timeout: Duration) -> Result<Response, NetError> {
    if req.body.len() >= MAX_REQUEST_BYTES {
        return Err(NetError::Transport(
            "a request body of 4,000,000 bytes or more is never sent".into(),
        ));
    }
    let method = reqwest::Method::from_bytes(req.method.as_bytes())
        .map_err(|_| NetError::Transport(format!("unknown method {}", req.method)))?;
    let client = client(is_loopback(&req.url), connect_timeout(timeout))?;
    let mut builder = client.request(method, &req.url).timeout(timeout);
    for (name, value) in &req.headers {
        builder = builder.header(name.as_str(), value.as_str());
    }
    if !req.body.is_empty() {
        builder = builder.body(req.body.clone());
    }
    builder.send().map_err(|e| net_error(&e.without_url()))
}

fn headers_of(response: &Response) -> Vec<(String, String)> {
    response
        .headers()
        .iter()
        .map(|(name, value)| {
            (
                name.as_str().to_ascii_lowercase(),
                String::from_utf8_lossy(value.as_bytes()).into_owned(),
            )
        })
        .collect()
}

fn blocking_send(req: &HttpRequest, timeout: Duration) -> Result<HttpResponse, NetError> {
    let response = start(req, timeout)?;
    let status = response.status().as_u16();
    let headers = headers_of(&response);
    let body = response
        .bytes()
        .map_err(|e| net_error(&e.without_url()))?
        .to_vec();
    Ok(HttpResponse {
        status,
        headers,
        body,
    })
}

/// Sends one request and reads the whole response, giving up after `timeout`.
pub fn send(req: &HttpRequest, timeout: Duration) -> Result<HttpResponse, NetError> {
    on_plain_thread(|| blocking_send(req, timeout))
}

/// The value of a header (names are lowercase).
pub fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

/// What the worker thread of a streamed request reports.
enum Streamed {
    Head(u16, Vec<(String, String)>),
    Data(String),
    Body(Vec<u8>),
    Failed(NetError),
}

/// Reads server-sent events: the `data:` lines of each event, joined by newlines, in order.
/// Comments and other fields are skipped; the stream ends with the body.
fn read_events(reader: impl BufRead, mut on_data: impl FnMut(String)) -> Result<(), NetError> {
    let mut data: Vec<String> = Vec::new();
    for line in reader.lines() {
        let line = line.map_err(|e| {
            if e.kind() == std::io::ErrorKind::TimedOut {
                NetError::Timeout
            } else {
                NetError::Transport(e.to_string())
            }
        })?;
        let line = line.strip_suffix('\r').unwrap_or(&line);
        if line.is_empty() {
            if !data.is_empty() {
                on_data(data.join("\n"));
                data.clear();
            }
            continue;
        }
        if let Some(value) = line.strip_prefix("data:") {
            data.push(value.strip_prefix(' ').unwrap_or(value).to_string());
        }
    }
    if !data.is_empty() {
        on_data(data.join("\n"));
    }
    Ok(())
}

fn stream_worker(req: &HttpRequest, timeout: Duration, tx: mpsc::Sender<Streamed>) {
    let response = match start(req, timeout) {
        Ok(response) => response,
        Err(e) => {
            let _ = tx.send(Streamed::Failed(e));
            return;
        }
    };
    let status = response.status().as_u16();
    let headers = headers_of(&response);
    let is_stream = header(&headers, "content-type")
        .is_some_and(|t| t.to_ascii_lowercase().starts_with("text/event-stream"));
    let _ = tx.send(Streamed::Head(status, headers));
    if status == 200 && is_stream {
        let result = read_events(BufReader::new(response), |data| {
            let _ = tx.send(Streamed::Data(data));
        });
        if let Err(e) = result {
            let _ = tx.send(Streamed::Failed(e));
        }
    } else {
        match response.bytes() {
            Ok(bytes) => {
                let _ = tx.send(Streamed::Body(bytes.to_vec()));
            }
            Err(e) => {
                let _ = tx.send(Streamed::Failed(net_error(&e.without_url())));
            }
        }
    }
}

/// Sends a request whose answer may be a stream of server-sent events. For a `200` with
/// `Content-Type: text/event-stream`, each event's data is passed to `on_data` as it arrives (on
/// the caller's thread) and the returned body is empty; any other answer is returned whole.
pub fn send_events(
    req: &HttpRequest,
    timeout: Duration,
    on_data: &mut dyn FnMut(&str),
) -> Result<HttpResponse, NetError> {
    std::thread::scope(|scope| {
        let (tx, rx) = mpsc::channel();
        let worker = scope.spawn(move || stream_worker(req, timeout, tx));
        let mut head: Option<(u16, Vec<(String, String)>)> = None;
        let mut body = Vec::new();
        let mut failure = None;
        for message in rx {
            match message {
                Streamed::Head(status, headers) => head = Some((status, headers)),
                Streamed::Data(data) => on_data(&data),
                Streamed::Body(bytes) => body = bytes,
                Streamed::Failed(e) => failure = Some(e),
            }
        }
        if let Err(panic) = worker.join() {
            std::panic::resume_unwind(panic);
        }
        if let Some(e) = failure {
            return Err(e);
        }
        let (status, headers) =
            head.ok_or_else(|| NetError::Transport("no response".to_string()))?;
        Ok(HttpResponse {
            status,
            headers,
            body,
        })
    })
}

/// POSTs a JSON body with no key (provisioning); the status and the raw body.
pub(crate) fn post_json(
    url: &str,
    body: &Value,
    timeout: Duration,
) -> Result<(u16, Vec<u8>), NetError> {
    let response = send(
        &HttpRequest {
            method: "POST".into(),
            url: url.to_string(),
            headers: vec![("content-type".into(), "application/json".into())],
            body: serde_json::to_vec(body).expect("a JSON value serialises"),
        },
        timeout,
    )?;
    Ok((response.status, response.body))
}

/// The real network of the desktop app: every request through [`send`], with a long limit for
/// pack downloads.
#[derive(Debug, Clone, Copy)]
pub struct HttpNet {
    timeout: Duration,
}

impl HttpNet {
    /// A network whose requests may take up to [`DOWNLOAD_TIMEOUT`].
    pub fn new() -> HttpNet {
        HttpNet {
            timeout: DOWNLOAD_TIMEOUT,
        }
    }

    pub fn with_timeout(timeout: Duration) -> HttpNet {
        HttpNet { timeout }
    }
}

impl Default for HttpNet {
    fn default() -> Self {
        HttpNet::new()
    }
}

impl Net for HttpNet {
    fn send(&self, req: HttpRequest) -> Result<HttpResponse, NetError> {
        send(&req, self.timeout)
    }
}

#[cfg(test)]
pub(crate) mod test_server {
    //! A tiny HTTP/1.1 server on 127.0.0.1 for the unit tests: it answers each connection with the
    //! next scripted response (after an optional delay) and records every request.

    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::{SocketAddr, TcpListener, TcpStream};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    #[derive(Debug, Clone)]
    pub struct Reply {
        pub status: u16,
        pub headers: Vec<(String, String)>,
        pub body: Vec<u8>,
        pub delay: Duration,
    }

    impl Reply {
        pub fn json(status: u16, body: &str) -> Reply {
            Reply {
                status,
                headers: vec![("content-type".into(), "application/json".into())],
                body: body.as_bytes().to_vec(),
                delay: Duration::ZERO,
            }
        }
        pub fn header(mut self, name: &str, value: &str) -> Reply {
            self.headers.push((name.into(), value.into()));
            self
        }
        pub fn after(mut self, delay: Duration) -> Reply {
            self.delay = delay;
            self
        }
    }

    #[derive(Debug, Clone)]
    pub struct Seen {
        pub method: String,
        pub path: String,
        pub headers: Vec<(String, String)>,
        pub body: Vec<u8>,
    }

    impl Seen {
        pub fn header(&self, name: &str) -> Option<&str> {
            super::header(&self.headers, name)
        }
    }

    pub struct Server {
        pub addr: SocketAddr,
        pub seen: Arc<Mutex<Vec<Seen>>>,
    }

    impl Server {
        pub fn url(&self, path: &str) -> String {
            format!("http://{}{path}", self.addr)
        }
        pub fn requests(&self) -> Vec<Seen> {
            self.seen.lock().unwrap().clone()
        }
    }

    fn read_request(stream: &mut TcpStream) -> Option<Seen> {
        let mut reader = BufReader::new(stream.try_clone().ok()?);
        let mut line = String::new();
        reader.read_line(&mut line).ok()?;
        let mut parts = line.split_whitespace();
        let method = parts.next()?.to_string();
        let path = parts.next()?.to_string();
        let mut headers = Vec::new();
        loop {
            let mut h = String::new();
            reader.read_line(&mut h).ok()?;
            let h = h.trim_end();
            if h.is_empty() {
                break;
            }
            if let Some((name, value)) = h.split_once(':') {
                headers.push((name.trim().to_ascii_lowercase(), value.trim().to_string()));
            }
        }
        let length = super::header(&headers, "content-length")
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(0);
        let mut body = vec![0; length];
        reader.read_exact(&mut body).ok()?;
        Some(Seen {
            method,
            path,
            headers,
            body,
        })
    }

    /// Serves `replies` in order, one per connection; then stops listening. `{BASE}` in a reply
    /// body becomes the server's own `http://127.0.0.1:<port>`.
    pub fn serve(replies: Vec<Reply>) -> Server {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let own = format!("http://{addr}");
        let replies: Vec<Reply> = replies
            .into_iter()
            .map(|mut reply| {
                if let Ok(text) = String::from_utf8(reply.body.clone()) {
                    reply.body = text.replace("{BASE}", &own).into_bytes();
                }
                reply
            })
            .collect();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let record = seen.clone();
        std::thread::spawn(move || {
            for reply in replies {
                let Ok((mut stream, _)) = listener.accept() else {
                    return;
                };
                if let Some(request) = read_request(&mut stream) {
                    record.lock().unwrap().push(request);
                }
                std::thread::sleep(reply.delay);
                let mut head = format!(
                    "HTTP/1.1 {} X\r\ncontent-length: {}\r\nconnection: close\r\n",
                    reply.status,
                    reply.body.len()
                );
                for (name, value) in &reply.headers {
                    head.push_str(&format!("{name}: {value}\r\n"));
                }
                head.push_str("\r\n");
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(&reply.body);
                let _ = stream.flush();
            }
        });
        Server { addr, seen }
    }

    /// An address nothing listens on.
    pub fn closed_url(path: &str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);
        format!("http://{addr}{path}")
    }
}

#[cfg(test)]
mod tests {
    use super::test_server::{Reply, closed_url, serve};
    use super::*;

    fn get(url: &str) -> HttpRequest {
        HttpRequest {
            method: "GET".into(),
            url: url.into(),
            headers: vec![],
            body: vec![],
        }
    }

    #[test]
    fn the_real_net_downloads_a_body_with_its_status_and_headers() {
        let pack: Vec<u8> = (0..=255u8).cycle().take(70_000).collect();
        let server = serve(vec![Reply {
            status: 200,
            headers: vec![("x-publik-balance".into(), "120".into())],
            body: pack.clone(),
            delay: Duration::ZERO,
        }]);
        let response = HttpNet::new()
            .send(get(&server.url("/es.sqlite3")))
            .unwrap();
        assert_eq!(response.status, 200);
        assert_eq!(response.body, pack);
        assert_eq!(header(&response.headers, "X-Publik-Balance"), Some("120"));
        let seen = server.requests();
        assert_eq!(seen.len(), 1);
        assert_eq!(
            (seen[0].method.as_str(), seen[0].path.as_str()),
            ("GET", "/es.sqlite3")
        );
        assert!(
            seen[0]
                .header("user-agent")
                .is_some_and(|ua| ua.starts_with("Parley/"))
        );
    }

    #[test]
    fn statuses_come_back_as_they_are_and_bodies_and_headers_go_out() {
        let server = serve(vec![Reply::json(404, "{\"error\":{}}")]);
        let response = send(
            &HttpRequest {
                method: "POST".into(),
                url: server.url("/x"),
                headers: vec![("authorization".into(), "Bearer pk_live_a".into())],
                body: b"{\"a\":1}".to_vec(),
            },
            Duration::from_secs(5),
        )
        .unwrap();
        assert_eq!(response.status, 404);
        let seen = &server.requests()[0];
        assert_eq!(seen.body, b"{\"a\":1}");
        assert_eq!(seen.header("authorization"), Some("Bearer pk_live_a"));
    }

    #[test]
    fn no_server_is_offline_and_a_slow_one_times_out() {
        assert_eq!(
            HttpNet::new().send(get(&closed_url("/x"))),
            Err(NetError::Offline)
        );
        // Even when the request's own limit is shorter than a refusal takes to arrive.
        assert_eq!(
            HttpNet::with_timeout(Duration::from_millis(400)).send(get(&closed_url("/x"))),
            Err(NetError::Offline)
        );
        assert_eq!(
            connect_timeout(Duration::from_secs(1)),
            Duration::from_millis(750)
        );
        assert_eq!(connect_timeout(DOWNLOAD_TIMEOUT), CONNECT_TIMEOUT);
        let server = serve(vec![
            Reply::json(200, "{}").after(Duration::from_millis(1500)),
        ]);
        let result = HttpNet::with_timeout(Duration::from_millis(300)).send(get(&server.url("/")));
        assert_eq!(result, Err(NetError::Timeout));
    }

    #[test]
    fn a_body_of_four_million_bytes_is_never_sent() {
        let server = serve(vec![Reply::json(200, "{}")]);
        let mut req = get(&server.url("/"));
        req.method = "POST".into();
        req.body = vec![b'a'; MAX_REQUEST_BYTES];
        assert!(matches!(
            send(&req, Duration::from_secs(5)),
            Err(NetError::Transport(_))
        ));
        req.body.truncate(MAX_REQUEST_BYTES - 1);
        assert_eq!(send(&req, Duration::from_secs(5)).unwrap().status, 200);
        assert_eq!(server.requests().len(), 1);
    }

    #[test]
    fn it_works_from_inside_an_async_runtime() {
        // As in a Tauri async command: a blocking reqwest call made straight from here would panic.
        let server = serve(vec![Reply::json(200, "{\"ok\":true}")]);
        let url = server.url("/");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let response = runtime.block_on(async move { HttpNet::new().send(get(&url)) });
        assert_eq!(response.unwrap().status, 200);
    }

    #[test]
    fn server_sent_events_arrive_in_order_on_the_callers_thread() {
        let events =
            "data: {\"a\":1}\n\n: comment\ndata: one\ndata: two\n\nevent: x\ndata: [DONE]\n\n";
        let server = serve(vec![
            Reply {
                status: 200,
                headers: vec![("content-type".into(), "text/event-stream".into())],
                body: events.as_bytes().to_vec(),
                delay: Duration::ZERO,
            },
            Reply::json(500, "{\"error\":{\"message\":\"boom\"}}"),
        ]);
        let mut seen = Vec::new();
        let caller = std::thread::current().id();
        let response = send_events(&get(&server.url("/")), Duration::from_secs(5), &mut |d| {
            assert_eq!(std::thread::current().id(), caller);
            seen.push(d.to_string());
        })
        .unwrap();
        assert_eq!(response.status, 200);
        assert!(response.body.is_empty());
        assert_eq!(seen, ["{\"a\":1}", "one\ntwo", "[DONE]"]);
        let failed = send_events(&get(&server.url("/")), Duration::from_secs(5), &mut |_| {
            panic!("no events in an error")
        })
        .unwrap();
        assert_eq!(failed.status, 500);
        assert_eq!(failed.body, b"{\"error\":{\"message\":\"boom\"}}");
    }

    #[test]
    fn loopback_hosts_are_recognised() {
        for url in [
            "http://localhost:11434/v1",
            "http://127.0.0.1:8080",
            "http://[::1]:1/v1",
            "https://127.0.0.2/",
        ] {
            assert!(is_loopback(url), "{url}");
        }
        for url in [
            "https://publikhq.com/api/v1",
            "http://192.168.1.2/v1",
            "not a url",
        ] {
            assert!(!is_loopback(url), "{url}");
        }
    }
}
