mod storage;
use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    io::{BufRead, BufReader, Read, Write},
    process::{Command, Stdio},
    time::Duration,
};
use url::Url;

const MAX_PLAYLIST: u64 = 10 * 1024 * 1024;
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
fn parse_playlist(text: &str, base: Option<&Url>) -> Vec<Channel> {
    let mut channels = Vec::new();
    let mut name: Option<String> = None;
    for line in text.lines() {
        let line = line.trim().trim_start_matches('\u{feff}').trim_start();
        if let Some(info) = line.strip_prefix("#EXTINF:") {
            let mut quoted = false;
            name = info.char_indices().find_map(|(i, c)| match c {
                '"' => {
                    quoted = !quoted;
                    None
                }
                ',' if !quoted => Some(info[i + 1..].trim().to_owned()),
                _ => None,
            });
        } else if !line.is_empty() && !line.starts_with('#') {
            let parsed = Url::parse(line).or_else(|_| {
                base.ok_or(url::ParseError::RelativeUrlWithoutBase)?
                    .join(line)
            });
            if let Ok(url) = parsed {
                if matches!(url.scheme(), "http" | "https") {
                    let label = name
                        .take()
                        .filter(|s| !s.is_empty())
                        .unwrap_or_else(|| format!("Channel {}", channels.len() + 1));
                    channels.push(Channel {
                        name: label,
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
    channels
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
    let mut file = File::open(path).context("could not open playlist")?;
    if file
        .metadata()
        .context("could not inspect playlist")?
        .is_dir()
    {
        bail!("playlist path is a directory, not a file; replace it with your M3U playlist (Docker may have created the directory when the file was missing)");
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
    Ok(parse_playlist(&text, base))
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
            parse_playlist(&content, None)
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
            let username = input
                .username
                .or_else(|| (!embedded_user.is_empty()).then_some(embedded_user));
            let password = input.password.or(embedded_password);
            let client = reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(20))
                .redirect(reqwest::redirect::Policy::custom(|attempt| {
                    if attempt.previous().len() >= 5
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
            parse_playlist(&String::from_utf8(bytes)?, Some(&base))
        }
        _ => bail!("invalid selection"),
    };
    if channels.is_empty() {
        bail!("playlist has no playable channels");
    }
    Ok(channels)
}

fn serve(channels: Vec<Channel>, port: u16) -> Result<()> {
    if port == 0 {
        bail!("port must be nonzero");
    }
    let server = tiny_http::Server::http(("127.0.0.1", port))
        .map_err(|e| anyhow::anyhow!("cannot bind web server: {e}"))?;
    eprintln!("Web UI: http://127.0.0.1:{port}");
    println!("READY");
    std::io::stdout().flush()?;
    let default_json = serde_json::to_string(&channels)?;
    for mut request in server.incoming_requests() {
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
        if host_ok && request.method() == &tiny_http::Method::Get && request.url() == "/" {
            set_cookie = Some(match &client {
                Some(id) => id.clone(),
                None => storage::id()?,
            });
        }
        let (code, body, content_type) = if !host_ok {
            (403, "Forbidden".to_owned(), "text/plain; charset=utf-8")
        } else if request.method() == &tiny_http::Method::Post && request.url() == "/api/playlist" {
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
                // A 10 MiB UTF-8 playlist may expand sixfold when JSON-escaped.
                const MAX_REQUEST: u64 = 64 * 1024 * 1024;
                let mut body = Vec::new();
                let read = request
                    .as_reader()
                    .take(MAX_REQUEST + 1)
                    .read_to_end(&mut body);
                if read.is_err() || body.len() as u64 > MAX_REQUEST {
                    (
                        413,
                        "Request too large".to_owned(),
                        "text/plain; charset=utf-8",
                    )
                } else {
                    match serde_json::from_slice::<PlaylistRequest>(&body)
                        .ok()
                        .and_then(|input| {
                            select_playlist(input.clone())
                                .ok()
                                .map(|channels| (input, channels))
                        }) {
                        Some((source, channels)) => {
                            let json = serde_json::to_string(&channels)?;
                            let state =
                                serde_json::to_string(&StoredPlaylist { source, channels })?;
                            if storage::write(client.as_deref().unwrap(), &state).is_ok() {
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
                }
            }
        } else if request.method() != &tiny_http::Method::Get {
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
                        match storage::read(id) {
                            Ok(json) => match serde_json::from_str::<StoredPlaylist>(&json) {
                                Ok(state) => (
                                    200,
                                    serde_json::to_string(&state.source)?,
                                    "application/json",
                                ),
                                Err(_) if serde_json::from_str::<Vec<Channel>>(&json).is_ok() => {
                                    (200, "null".to_owned(), "application/json")
                                }
                                Err(_) => (
                                    500,
                                    "{\"error\":\"Unable to read playlist\"}".to_owned(),
                                    "application/json",
                                ),
                            },
                            Err(e)
                                if e.downcast_ref::<std::io::Error>()
                                    .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
                            {
                                (200, "null".to_owned(), "application/json")
                            }
                            Err(_) => (
                                500,
                                "{\"error\":\"Unable to read playlist\"}".to_owned(),
                                "application/json",
                            ),
                        }
                    } else {
                        (
                            401,
                            "Open / to initialize a browser session".to_owned(),
                            "text/plain; charset=utf-8",
                        )
                    }
                }
                "/api/channels" => match client.as_deref() {
                    Some(id) => match storage::read(id) {
                        Ok(json) if serde_json::from_str::<StoredPlaylist>(&json).is_ok() => {
                            let state: StoredPlaylist = serde_json::from_str(&json)?;
                            (
                                200,
                                serde_json::to_string(&state.channels)?,
                                "application/json",
                            )
                        }
                        Ok(json) if serde_json::from_str::<Vec<Channel>>(&json).is_ok() => {
                            (200, json, "application/json")
                        }
                        Err(e)
                            if e.downcast_ref::<std::io::Error>()
                                .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
                        {
                            (200, default_json.clone(), "application/json")
                        }
                        _ => (
                            500,
                            "{\"error\":\"Unable to read playlist\"}".to_owned(),
                            "application/json",
                        ),
                    },
                    None => (
                        401,
                        "Open / to initialize a browser session".to_owned(),
                        "text/plain; charset=utf-8",
                    ),
                },
                _ => (404, "Not found".to_owned(), "text/plain; charset=utf-8"),
            }
        };
        let mut response = tiny_http::Response::from_string(body).with_status_code(code)
            .with_header(tiny_http::Header::from_bytes("Content-Type", content_type).unwrap())
            .with_header(tiny_http::Header::from_bytes("Cache-Control", "no-store").unwrap())
            .with_header(tiny_http::Header::from_bytes("X-Content-Type-Options", "nosniff").unwrap())
            .with_header(tiny_http::Header::from_bytes("Content-Security-Policy", "default-src 'none'; script-src 'self'; style-src 'unsafe-inline'; connect-src 'self'; media-src http: https:; base-uri 'none'; frame-ancestors 'none'").unwrap());
        if let Some(id) = set_cookie {
            response = response.with_header(
                tiny_http::Header::from_bytes(
                    "Set-Cookie",
                    format!("iptv_client={id}; HttpOnly; SameSite=Lax; Path=/; Max-Age=31536000"),
                )
                .unwrap(),
            );
        }
        let _ = request.respond(response);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn playlist_name_ignores_commas_in_quoted_attributes() {
        let channels = super::parse_playlist(
            "#EXTINF:-1 group-title=\"News, World\",Actual channel\nhttps://example.com/live\n",
            None,
        );
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
    fn default_http_port_accepts_loopback_host_without_port() {
        assert!(super::allowed_host("127.0.0.1", 80));
        assert!(super::allowed_host("localhost", 80));
        assert!(!super::allowed_host("localhost", 8080));
        assert!(!super::allowed_host("evil.example", 80));
    }
}
