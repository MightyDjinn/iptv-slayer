mod storage;
use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use std::{
    io::{BufRead, BufReader, Read, Write},
    process::{Command, Stdio},
    time::Duration,
};
use url::Url;

const MAX_PLAYLIST: u64 = 10 * 1024 * 1024;
const MAX_CHANNELS: usize = 100_000;
const MAX_EXPANDED_PLAYLIST: usize = 10 * 1024 * 1024;
#[derive(Parser)]
#[command(version, about = "Local IPTV playlist browser")]
struct Args {
    /// Local path or HTTP(S) playlist URL. URLs may contain credentials.
    #[arg(long)]
    playlist: Option<String>,
    /// Start the local web UI as a background process.
    #[arg(long)]
    web: bool,
    /// Run the web UI in the foreground (for containers and services).
    #[arg(long, requires = "web")]
    foreground: bool,
    /// Loopback web UI port.
    #[arg(long, default_value_t = 8080)]
    port: u16,
    #[command(subcommand)]
    command: Option<Action>,
}
#[derive(Subcommand)]
enum Action {
    /// List channels, optionally filtering by name (case-insensitive).
    List {
        #[arg(long)]
        search: Option<String>,
    },
    /// Play a channel by its 1-based list number using mpv.
    Play { number: usize },
}
#[derive(Clone, Serialize, Deserialize)]
struct Channel {
    name: String,
    url: String,
}
fn parse_playlist(text: &str, base: Option<&Url>) -> Result<Vec<Channel>> {
    let mut channels = Vec::new();
    let mut expanded_bytes = 0usize;
    let mut name: Option<&str> = None;
    for line in text.lines() {
        let line = line.trim().trim_start_matches('\u{feff}').trim_start();
        if let Some(info) = line.strip_prefix("#EXTINF:") {
            let mut quoted = false;
            name = info.char_indices().find_map(|(i, c)| match c {
                '"' => {
                    quoted = !quoted;
                    None
                }
                ',' if !quoted => Some(info[i + 1..].trim()),
                _ => None,
            });
        } else if !line.is_empty() && !line.starts_with('#') {
            let parsed = Url::parse(line).or_else(|_| {
                base.ok_or(url::ParseError::RelativeUrlWithoutBase)?
                    .join(line)
            });
            if let Ok(url) = parsed {
                if matches!(url.scheme(), "http" | "https") {
                    if channels.len() >= MAX_CHANNELS {
                        bail!("playlist has too many channels");
                    }
                    let default_label;
                    let label = match name.take().filter(|s| !s.is_empty()) {
                        Some(label) => label,
                        None => {
                            default_label = format!("Channel {}", channels.len() + 1);
                            &default_label
                        }
                    };
                    expanded_bytes = expanded_bytes
                        .checked_add(label.len())
                        .and_then(|size| size.checked_add(url.as_str().len()))
                        .filter(|size| *size <= MAX_EXPANDED_PLAYLIST)
                        .ok_or_else(|| anyhow::anyhow!("expanded playlist exceeds 10 MiB"))?;
                    channels.push(Channel {
                        name: label.to_owned(),
                        url: url.to_string(),
                    });
                } else {
                    name = None;
                }
            } else {
                name = None;
            }
        }
    }
    Ok(channels)
}
fn load(source: &str) -> Result<Vec<Channel>> {
    let mut base = Url::parse(source).ok();
    // Existing opaque-looking names like `channels:2026.m3u` are local files,
    // not network URLs. Hierarchical URLs retain the HTTP(S)-only restriction.
    if base
        .as_ref()
        .is_some_and(|url| url.cannot_be_a_base() && std::path::Path::new(source).is_file())
    {
        base = None;
    }
    let mut reader: Box<dyn Read> = if let Some(url) = &base {
        if !matches!(url.scheme(), "http" | "https") {
            bail!("playlist URL must use HTTP(S)");
        }
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(20))
            .redirect(reqwest::redirect::Policy::limited(5))
            .build()?;
        let response = client
            .get(url.clone())
            .send()
            .map_err(|e| e.without_url())
            .context("playlist request failed")?
            .error_for_status()
            .map_err(|e| e.without_url())
            .context("playlist request failed")?;
        base = Some(response.url().clone());
        Box::new(response)
    } else {
        return load_local_playlist(std::path::Path::new(source));
    };
    read_playlist(&mut reader, base.as_ref())
}

fn load_local_playlist(path: &std::path::Path) -> Result<Vec<Channel>> {
    #[cfg(unix)]
    let mut file = {
        use std::os::unix::fs::OpenOptionsExt;
        std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(path)
    }
    .context("could not open playlist")?;
    #[cfg(not(unix))]
    let mut file = std::fs::File::open(path).context("could not open playlist")?;
    let metadata = file.metadata().context("could not inspect playlist")?;
    if metadata.is_dir() {
        bail!("playlist path is a directory, not a file; replace it with your M3U playlist (Docker may have created the directory when the file was missing)");
    }
    if !metadata.is_file() {
        bail!("playlist path is not a regular file");
    }
    read_playlist(&mut file, None)
}

fn read_playlist(reader: &mut dyn Read, base: Option<&Url>) -> Result<Vec<Channel>> {
    let mut bytes = Vec::new();
    reader
        .take(MAX_PLAYLIST + 1)
        .read_to_end(&mut bytes)
        .context("could not read playlist")?;
    if bytes.len() as u64 > MAX_PLAYLIST {
        bail!("playlist exceeds 10 MiB");
    }
    let text = String::from_utf8(bytes).context("playlist is not UTF-8")?;
    parse_playlist(&text, base)
}
fn main() -> Result<()> {
    let args = Args::parse();
    if args.web && args.command.is_some() {
        bail!("--web cannot be combined with a command");
    }
    if args.web && !args.foreground {
        let mut command = Command::new(std::env::current_exe()?);
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            // setsid is async-signal-safe and detaches the child from the terminal.
            unsafe {
                command.pre_exec(|| {
                    if libc::setsid() == -1 {
                        Err(std::io::Error::last_os_error())
                    } else {
                        Ok(())
                    }
                });
            }
        }
        if let Some(playlist) = &args.playlist {
            command.args(["--playlist", playlist]);
        }
        let mut child = command
            .args(["--web", "--foreground", "--port", &args.port.to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .context("could not start web process")?;
        let mut ready = String::new();
        BufReader::new(child.stdout.take().context("missing startup pipe")?)
            .read_line(&mut ready)?;
        if ready.trim() != "READY" {
            let _ = child.wait();
            bail!("web process failed to start; run with --foreground for details");
        }
        println!(
            "Web UI at http://127.0.0.1:{} (PID {})",
            args.port,
            child.id()
        );
        return Ok(());
    }
    let channels = match &args.playlist {
        Some(playlist) => load(playlist)?,
        None if args.web => {
            let config = std::env::var_os("XDG_CONFIG_HOME")
                .filter(|p| std::path::Path::new(p).is_absolute())
                .map(std::path::PathBuf::from)
                .or_else(|| {
                    std::env::var_os("HOME")
                        .map(|home| std::path::PathBuf::from(home).join(".config"))
                });
            let playlist = config.map(|dir| dir.join("iptv-slayer/channels.m3u"));
            match playlist.filter(|path| path.exists()) {
                Some(path) => {
                    match load_local_playlist(&path) {
                        Ok(channels) => channels,
                        Err(_) => {
                            eprintln!("Could not load the default playlist; choose a playlist in the web UI.");
                            Vec::new()
                        }
                    }
                }
                None => Vec::new(),
            }
        }
        None => bail!("--playlist is required unless using --web"),
    };
    if args.web {
        return serve(channels, args.port);
    }
    match args.command.unwrap_or(Action::List { search: None }) {
        Action::List { search } => {
            let filter = search.unwrap_or_default().to_lowercase();
            for (i, c) in channels
                .iter()
                .enumerate()
                .filter(|(_, c)| c.name.to_lowercase().contains(&filter))
            {
                println!("{}\t{}", i + 1, c.name);
            }
        }
        Action::Play { number } => {
            let channel = number
                .checked_sub(1)
                .and_then(|n| channels.get(n))
                .context("channel number out of range")?;
            let status = Command::new("mpv")
                .arg("--")
                .arg(&channel.url)
                .status()
                .context("could not launch mpv")?;
            if !status.success() {
                bail!("mpv exited with {status}");
            }
        }
    }
    Ok(())
}
fn allowed_host(host: &str, port: u16) -> bool {
    host == format!("127.0.0.1:{port}")
        || host == format!("localhost:{port}")
        || (port == 80 && matches!(host, "127.0.0.1" | "localhost"))
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PlaylistRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    password: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct StoredPlaylist {
    source: PlaylistRequest,
    channels: Vec<Channel>,
}

fn select_playlist(input: PlaylistRequest) -> Result<Vec<Channel>> {
    let channels = match (input.content, input.url) {
        (Some(content), None) if input.username.is_none() && input.password.is_none() => {
            if content.len() as u64 > MAX_PLAYLIST {
                bail!("playlist too large");
            }
            parse_playlist(&content, None)?
        }
        (None, Some(source)) => {
            let mut url = Url::parse(&source).map_err(|_| anyhow::anyhow!("invalid URL"))?;
            if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
                bail!("invalid URL");
            }
            let embedded_user = percent_encoding::percent_decode_str(url.username())
                .decode_utf8()
                .context("invalid URL credentials")?
                .into_owned();
            let embedded_password = url
                .password()
                .map(|password| {
                    percent_encoding::percent_decode_str(password)
                        .decode_utf8()
                        .map(|s| s.into_owned())
                })
                .transpose()
                .context("invalid URL credentials")?;
            url.set_username("")
                .map_err(|_| anyhow::anyhow!("invalid URL"))?;
            url.set_password(None)
                .map_err(|_| anyhow::anyhow!("invalid URL"))?;
            if (input.username.is_some() || input.password.is_some())
                && (!embedded_user.is_empty() || embedded_password.is_some())
            {
                bail!("ambiguous credentials");
            }
            if input.password.is_some() && input.username.is_none() {
                bail!("missing username");
            }
            let username = input.username.or_else(|| {
                (!embedded_user.is_empty() || embedded_password.is_some()).then_some(embedded_user)
            });
            let password = input.password.or(embedded_password);
            let client = reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(20))
                .redirect(reqwest::redirect::Policy::custom(|attempt| {
                    if attempt.previous().len() > 5
                        || attempt
                            .previous()
                            .first()
                            .is_some_and(|first| first.origin() != attempt.url().origin())
                    {
                        attempt.stop()
                    } else {
                        attempt.follow()
                    }
                }))
                .build()?;
            let mut request = client.get(url);
            if let Some(username) = username {
                request = request.basic_auth(username, password);
            }
            let mut response = request.send().map_err(|e| e.without_url())?;
            if !response.status().is_success() {
                bail!("playlist request failed");
            }
            let base = response.url().clone();
            let mut bytes = Vec::new();
            response
                .by_ref()
                .take(MAX_PLAYLIST + 1)
                .read_to_end(&mut bytes)?;
            if bytes.len() as u64 > MAX_PLAYLIST {
                bail!("playlist too large");
            }
            parse_playlist(&String::from_utf8(bytes)?, Some(&base))?
        }
        _ => bail!("invalid selection"),
    };
    if channels.is_empty() {
        bail!("playlist has no playable channels");
    }
    Ok(channels)
}

type ClientSelections =
    std::collections::HashMap<String, (std::sync::Arc<std::sync::Mutex<u64>>, usize)>;

struct WebState {
    default_json: String,
    jobs: std::sync::Arc<tokio::sync::Semaphore>,
    reads: std::sync::Arc<tokio::sync::Semaphore>,
    read_responses: std::sync::Arc<tokio::sync::Semaphore>,
    job_responses: std::sync::Arc<tokio::sync::Semaphore>,
    next: std::sync::atomic::AtomicU64,
    latest: std::sync::Mutex<ClientSelections>,
}

// Hyper can retain these bytes after polling the body, until socket writes finish.
struct ResponseBytes {
    body: String,
    _permit: tokio::sync::OwnedSemaphorePermit,
}
impl AsRef<[u8]> for ResponseBytes {
    fn as_ref(&self) -> &[u8] {
        self.body.as_bytes()
    }
}

// Entries live only while requests for that client are in flight. The guard moves
// into blocking work so a disconnected client cannot release it prematurely.
struct SelectionGuard {
    state: std::sync::Arc<WebState>,
    id: String,
}
impl Drop for SelectionGuard {
    fn drop(&mut self) {
        let mut latest = self.state.latest.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(entry) = latest.get_mut(&self.id) {
            entry.1 -= 1;
            if entry.1 == 0 {
                latest.remove(&self.id);
            }
        }
    }
}

// Preserve the existing strict header validation while using Hyper's HTTP parser.
struct Field(String);
impl Field {
    fn equiv(&self, name: &str) -> bool {
        self.0.eq_ignore_ascii_case(name)
    }
}
struct Value(String);
impl Value {
    fn as_str(&self) -> &str {
        &self.0
    }
}
struct WebHeader {
    field: Field,
    value: Value,
}
struct WebRequest {
    method: String,
    url: String,
    headers: Vec<WebHeader>,
    body: hyper::body::Incoming,
}
impl WebRequest {
    fn method(&self) -> &str {
        &self.method
    }
    fn url(&self) -> &str {
        &self.url
    }
    fn headers(&self) -> &[WebHeader] {
        &self.headers
    }
}

fn serve(channels: Vec<Channel>, port: u16) -> Result<()> {
    if port == 0 {
        bail!("port must be nonzero");
    }
    let listener =
        std::net::TcpListener::bind(("127.0.0.1", port)).context("cannot bind web server")?;
    listener.set_nonblocking(true)?;
    let state = std::sync::Arc::new(WebState {
        default_json: serde_json::to_string(&channels)?,
        jobs: std::sync::Arc::new(tokio::sync::Semaphore::new(4)),
        reads: std::sync::Arc::new(tokio::sync::Semaphore::new(4)),
        read_responses: std::sync::Arc::new(tokio::sync::Semaphore::new(4)),
        job_responses: std::sync::Arc::new(tokio::sync::Semaphore::new(4)),
        next: std::sync::atomic::AtomicU64::new(0),
        latest: std::sync::Mutex::new(std::collections::HashMap::new()),
    });
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    eprintln!("Web UI: http://127.0.0.1:{port}");
    println!("READY");
    std::io::stdout().flush()?;
    runtime.block_on(async move {
        let listener = tokio::net::TcpListener::from_std(listener)?;
        let connections = std::sync::Arc::new(tokio::sync::Semaphore::new(128));
        loop {
            let (stream, _) = listener.accept().await?;
            let Ok(permit) = connections.clone().try_acquire_owned() else {
                continue;
            };
            let state = state.clone();
            tokio::spawn(async move {
                let _permit = permit;
                let service =
                    hyper::service::service_fn(move |req| handle(req, state.clone(), port));
                let mut builder = hyper::server::conn::http1::Builder::new();
                builder
                    .timer(hyper_util::rt::TokioTimer::new())
                    .header_read_timeout(Duration::from_secs(5))
                    .keep_alive(false);
                let _ = tokio::time::timeout(
                    Duration::from_secs(40),
                    builder.serve_connection(hyper_util::rt::TokioIo::new(stream), service),
                )
                .await;
            });
        }
        #[allow(unreachable_code)]
        Ok::<(), anyhow::Error>(())
    })
}

async fn handle(
    req: hyper::Request<hyper::body::Incoming>,
    state: std::sync::Arc<WebState>,
    port: u16,
) -> std::result::Result<
    hyper::Response<http_body_util::Full<bytes::Bytes>>,
    std::convert::Infallible,
> {
    let (parts, body) = req.into_parts();
    let headers = parts
        .headers
        .iter()
        .map(|(name, value)| WebHeader {
            field: Field(name.as_str().to_owned()),
            value: Value(value.to_str().unwrap_or("").to_owned()),
        })
        .collect();
    let request = WebRequest {
        method: parts.method.to_string(),
        url: parts
            .uri
            .path_and_query()
            .map(|v| v.as_str())
            .unwrap_or("")
            .to_owned(),
        headers,
        body,
    };
    let response_limit = match (request.method(), request.url()) {
        ("GET", "/api/channels" | "/api/source") => Some(state.read_responses.clone()),
        ("POST", "/api/playlist") => Some(state.job_responses.clone()),
        _ => None,
    };
    let permit = response_limit.map(|limit| limit.try_acquire_owned());
    let busy = matches!(permit, Some(Err(_)));
    let permit = permit.and_then(Result::ok);
    let result = if busy {
        (
            503,
            "Server busy".to_owned(),
            "text/plain; charset=utf-8",
            None,
        )
    } else {
        handle_inner(request, state, port)
            .await
            .unwrap_or_else(|_| {
                (
                    500,
                    "Internal error".to_owned(),
                    "text/plain; charset=utf-8",
                    None,
                )
            })
    };
    let (code, body, content_type, set_cookie) = result;
    let mut response = hyper::Response::builder().status(code)
        .header("Content-Type", content_type).header("Cache-Control", "no-store")
        .header("X-Content-Type-Options", "nosniff")
        .header("Content-Security-Policy", "default-src 'none'; script-src 'self'; style-src 'unsafe-inline'; connect-src 'self'; media-src http: https:; base-uri 'none'; frame-ancestors 'none'");
    if let Some(id) = set_cookie {
        response = response.header(
            "Set-Cookie",
            format!("iptv_client={id}; HttpOnly; SameSite=Lax; Path=/; Max-Age=31536000"),
        );
    }
    let body = match permit {
        Some(permit) => bytes::Bytes::from_owner(ResponseBytes {
            body,
            _permit: permit,
        }),
        None => bytes::Bytes::from(body),
    };
    Ok(response.body(http_body_util::Full::new(body)).unwrap())
}

async fn handle_inner(
    request: WebRequest,
    state: std::sync::Arc<WebState>,
    port: u16,
) -> Result<(u16, String, &'static str, Option<String>)> {
    let host_ok = request
        .headers()
        .iter()
        .filter(|h| h.field.equiv("Host"))
        .count()
        == 1
        && request
            .headers()
            .iter()
            .any(|h| h.field.equiv("Host") && allowed_host(h.value.as_str(), port));
    let mut ids = request
        .headers()
        .iter()
        .filter(|h| h.field.equiv("Cookie"))
        .flat_map(|h| h.value.as_str().split(';'))
        .filter_map(|part| part.trim().strip_prefix("iptv_client="));
    let first = ids.next();
    let client = if ids.next().is_none() {
        first.filter(|id| storage::valid(id)).map(str::to_owned)
    } else {
        None
    };
    let mut set_cookie = None;
    if host_ok && request.method() == "GET" && request.url() == "/" {
        set_cookie = Some(match &client {
            Some(id) => id.clone(),
            None => storage::id()?,
        });
    }
    let (code, body, content_type) = if !host_ok {
        (403, "Forbidden".to_owned(), "text/plain; charset=utf-8")
    } else if request.method() == "POST" && request.url() == "/api/playlist" {
        let headers = request.headers();
        let host = headers
            .iter()
            .find(|h| h.field.equiv("Host"))
            .unwrap()
            .value
            .as_str();
        let origins: Vec<_> = headers.iter().filter(|h| h.field.equiv("Origin")).collect();
        let types: Vec<_> = headers
            .iter()
            .filter(|h| h.field.equiv("Content-Type"))
            .collect();
        if origins.len() != 1 || origins[0].value.as_str() != format!("http://{host}") {
            (403, "Forbidden".to_owned(), "text/plain; charset=utf-8")
        } else if types.len() != 1
            || !types[0]
                .value
                .as_str()
                .split(';')
                .next()
                .unwrap_or("")
                .trim()
                .eq_ignore_ascii_case("application/json")
        {
            (
                415,
                "Expected application/json".to_owned(),
                "text/plain; charset=utf-8",
            )
        } else if client.is_none() {
            (
                401,
                "Open / to initialize a browser session".to_owned(),
                "text/plain; charset=utf-8",
            )
        } else {
            match state.jobs.clone().try_acquire_owned() {
                Err(_) => (503, "Server busy".to_owned(), "text/plain; charset=utf-8"),
                Ok(permit) => {
                    let client_id = client.clone().unwrap();
                    let (sequence, client_lock) = {
                        let mut latest = state.latest.lock().unwrap_or_else(|e| e.into_inner());
                        let sequence =
                            state.next.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                        let entry = latest
                            .entry(client_id.clone())
                            .or_insert_with(|| (std::sync::Arc::new(std::sync::Mutex::new(0)), 0));
                        entry.1 += 1;
                        (sequence, entry.0.clone())
                    };
                    let selection_guard = SelectionGuard {
                        state: state.clone(),
                        id: client_id.clone(),
                    };
                    use http_body_util::BodyExt;
                    let body = tokio::time::timeout(Duration::from_secs(10), async {
                        let mut body = request.body;
                        let mut bytes = Vec::new();
                        while let Some(frame) = body.frame().await {
                            let frame = frame.map_err(|_| ())?;
                            if let Ok(data) = frame.into_data() {
                                if bytes.len().saturating_add(data.len()) > 64 * 1024 * 1024 {
                                    return Err(());
                                }
                                bytes.extend_from_slice(&data);
                            }
                        }
                        Ok(bytes)
                    })
                    .await;
                    match body {
                        Err(_) => (
                            408,
                            "Request timeout".to_owned(),
                            "text/plain; charset=utf-8",
                        ),
                        Ok(Err(())) => (
                            413,
                            "Request too large".to_owned(),
                            "text/plain; charset=utf-8",
                        ),
                        Ok(Ok(body)) => tokio::task::spawn_blocking(move || {
                            let _permit = permit;
                            let _selection_guard = selection_guard;
                            let result = serde_json::from_slice::<PlaylistRequest>(&body)
                                .ok()
                                .and_then(|input| {
                                    select_playlist(input.clone())
                                        .ok()
                                        .map(|channels| (input, channels))
                                });
                            match result {
                                Some((source, channels)) => {
                                    let json = serde_json::to_string(&channels).unwrap();
                                    let stored =
                                        serde_json::to_string(&StoredPlaylist { source, channels })
                                            .unwrap();
                                    let mut latest =
                                        client_lock.lock().unwrap_or_else(|e| e.into_inner());
                                    if *latest > sequence {
                                        return (
                                            409,
                                            "{\"error\":\"Selection superseded\"}".to_owned(),
                                            "application/json",
                                        );
                                    }
                                    if storage::write(&client_id, &stored, || *latest = sequence)
                                        .is_ok()
                                    {
                                        (200, json, "application/json")
                                    } else {
                                        (
                                            500,
                                            "{\"error\":\"Unable to save playlist\"}".to_owned(),
                                            "application/json",
                                        )
                                    }
                                }
                                None => (
                                    400,
                                    "{\"error\":\"Unable to load playlist\"}".to_owned(),
                                    "application/json",
                                ),
                            }
                        })
                        .await
                        .unwrap_or((
                            500,
                            "Internal error".to_owned(),
                            "text/plain; charset=utf-8",
                        )),
                    }
                }
            }
        }
    } else if request.method() != "GET" {
        (
            405,
            "Method not allowed".to_owned(),
            "text/plain; charset=utf-8",
        )
    } else {
        match request.url() {
            "/" => (
                200,
                include_str!("index.html").to_owned(),
                "text/html; charset=utf-8",
            ),
            "/app.js" => (
                200,
                include_str!("app.js").to_owned(),
                "application/javascript; charset=utf-8",
            ),
            "/api/source" => {
                let origins: Vec<_> = request
                    .headers()
                    .iter()
                    .filter(|h| h.field.equiv("Origin"))
                    .collect();
                let host = request
                    .headers()
                    .iter()
                    .find(|h| h.field.equiv("Host"))
                    .unwrap()
                    .value
                    .as_str();
                let fetch_ok = request
                    .headers()
                    .iter()
                    .filter(|h| h.field.equiv("Sec-Fetch-Site"))
                    .all(|h| matches!(h.value.as_str(), "same-origin" | "none"));
                if origins.len() > 1
                    || origins
                        .first()
                        .is_some_and(|h| h.value.as_str() != format!("http://{host}"))
                    || !fetch_ok
                {
                    (403, "Forbidden".to_owned(), "text/plain; charset=utf-8")
                } else if let Some(id) = client.as_deref() {
                    read_state(state.clone(), id.to_owned(), true).await
                } else {
                    (
                        401,
                        "Open / to initialize a browser session".to_owned(),
                        "text/plain; charset=utf-8",
                    )
                }
            }
            "/api/channels" => match client.as_deref() {
                Some(id) => read_state(state.clone(), id.to_owned(), false).await,
                None => (
                    401,
                    "Open / to initialize a browser session".to_owned(),
                    "text/plain; charset=utf-8",
                ),
            },
            _ => (404, "Not found".to_owned(), "text/plain; charset=utf-8"),
        }
    };
    Ok((code, body, content_type, set_cookie))
}

async fn read_state(
    state: std::sync::Arc<WebState>,
    id: String,
    source: bool,
) -> (u16, String, &'static str) {
    let Ok(permit) = state.reads.clone().try_acquire_owned() else {
        return (503, "Server busy".to_owned(), "text/plain; charset=utf-8");
    };
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let result = match storage::read(&id) {
            Ok(json) => match serde_json::from_str::<StoredPlaylist>(&json) {
                Ok(stored) => {
                    if source {
                        serde_json::to_string(&stored.source).ok()
                    } else {
                        serde_json::to_string(&stored.channels).ok()
                    }
                }
                Err(_) => serde_json::from_str::<Vec<Channel>>(&json).ok().map(|_| {
                    if source {
                        "null".to_owned()
                    } else {
                        json
                    }
                }),
            },
            Err(e)
                if e.downcast_ref::<std::io::Error>()
                    .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
            {
                Some(if source {
                    "null".to_owned()
                } else {
                    state.default_json.clone()
                })
            }
            Err(_) => None,
        };
        match result {
            Some(json) => (200, json, "application/json"),
            None => (
                500,
                "{\"error\":\"Unable to read playlist\"}".to_owned(),
                "application/json",
            ),
        }
    })
    .await
    .unwrap_or((
        500,
        "Internal error".to_owned(),
        "text/plain; charset=utf-8",
    ))
}

#[cfg(test)]
mod tests {
    #[test]
    fn rejects_amplified_relative_urls() {
        let base = url::Url::parse(&format!(
            "https://example.com/{}/list.m3u",
            "a".repeat(32_000)
        ))
        .unwrap();
        let input = "stream\n".repeat(600);
        let result = super::read_playlist(&mut input.as_bytes(), Some(&base));
        assert!(result.is_err(), "expanded playlist must be rejected");
    }

    #[test]
    fn channel_count_limit_accepts_boundary_and_rejects_next() {
        let base = url::Url::parse("https://example.com/list").unwrap();
        let input = "x\n".repeat(super::MAX_CHANNELS);
        assert_eq!(
            super::parse_playlist(&input, Some(&base)).unwrap().len(),
            super::MAX_CHANNELS
        );
        let over = format!("{input}x\n");
        assert!(super::parse_playlist(&over, Some(&base)).is_err());
    }

    #[test]
    fn playlist_name_ignores_commas_in_quoted_attributes() {
        let channels = super::parse_playlist(
            "#EXTINF:-1 group-title=\"News, World\",Actual channel\nhttps://example.com/live\n",
            None,
        )
        .unwrap();
        assert_eq!(channels.len(), 1);
        assert_eq!(channels[0].name, "Actual channel");
    }

    #[test]
    fn redirected_playlist_resolves_stream_against_final_url() {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            for response in [
                "HTTP/1.1 302 Found\r\nLocation: /new/list.m3u\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_owned(),
                "HTTP/1.1 200 OK\r\nContent-Length: 12\r\nConnection: close\r\n\r\nstream.m3u8\n".to_owned(),
            ] {
                let (mut stream, _) = listener.accept().unwrap();
                stream.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
                let mut request = [0; 4096];
                assert!(stream.read(&mut request).unwrap() > 0);
                stream.write_all(response.as_bytes()).unwrap();
            }
        });
        let channels = super::load(&format!("http://127.0.0.1:{port}/old/list.m3u")).unwrap();
        server.join().unwrap();
        assert_eq!(channels.len(), 1);
        assert_eq!(
            channels[0].url,
            format!("http://127.0.0.1:{port}/new/stream.m3u8")
        );
    }

    #[test]
    fn browser_playlist_follows_up_to_five_same_origin_redirects() {
        for (redirects, expected_success, expected_requests) in [(5, true, 6), (6, false, 6)] {
            let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
            let address = server.server_addr().to_ip().unwrap();
            let worker = std::thread::spawn(move || {
                let mut count = 0;
                while let Some(request) = server
                    .recv_timeout(std::time::Duration::from_secs(2))
                    .unwrap()
                {
                    count += 1;
                    let response = if count <= redirects {
                        tiny_http::Response::from_string("")
                            .with_status_code(302)
                            .with_header(
                                tiny_http::Header::from_bytes("Location", format!("/{count}"))
                                    .unwrap(),
                            )
                    } else {
                        tiny_http::Response::from_string(
                            "#EXTM3U\n#EXTINF:-1,Test\nhttps://example.com/live\n",
                        )
                    };
                    request.respond(response).unwrap();
                }
                count
            });
            let result = super::select_playlist(super::PlaylistRequest {
                content: None,
                url: Some(format!("http://{address}/start")),
                username: None,
                password: None,
            });
            let count = worker.join().unwrap();
            assert_eq!(result.is_ok(), expected_success, "redirects: {redirects}");
            assert_eq!(count, expected_requests, "redirects: {redirects}");
        }
    }

    #[test]
    fn url_credentials_are_percent_decoded_for_basic_auth() {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let worker = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .unwrap();
            let mut bytes = [0; 4096];
            let n = stream.read(&mut bytes).unwrap();
            let request = String::from_utf8_lossy(&bytes[..n]).to_lowercase();
            let body = "https://example.org/stream\n";
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
            request
        });
        let input = super::PlaylistRequest {
            content: None,
            url: Some(format!("http://user:p%40ss@127.0.0.1:{port}/playlist")),
            username: None,
            password: None,
        };
        assert!(super::select_playlist(input).is_ok());
        assert!(worker
            .join()
            .unwrap()
            .contains("authorization: basic dxnlcjpwqhnz"));
    }

    #[test]
    fn url_password_with_empty_username_uses_basic_auth() {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let address = server.server_addr().to_ip().unwrap();
        let worker = std::thread::spawn(move || {
            let request = server
                .recv_timeout(std::time::Duration::from_secs(5))
                .unwrap()
                .unwrap();
            let authorization = request
                .headers()
                .iter()
                .find(|header| header.field.equiv("Authorization"))
                .map(|header| header.value.as_str().to_owned());
            request
                .respond(tiny_http::Response::from_string(
                    "https://example.com/live\n",
                ))
                .unwrap();
            authorization
        });
        let result = super::select_playlist(super::PlaylistRequest {
            content: None,
            url: Some(format!("http://:pass@{address}/playlist")),
            username: None,
            password: None,
        });
        assert!(result.is_ok());
        assert_eq!(worker.join().unwrap().as_deref(), Some("Basic OnBhc3M="));
    }

    #[test]
    fn default_http_port_accepts_loopback_host_without_port() {
        assert!(super::allowed_host("127.0.0.1", 80));
        assert!(super::allowed_host("localhost", 80));
        assert!(!super::allowed_host("localhost", 8080));
        assert!(!super::allowed_host("evil.example", 80));
    }
}
