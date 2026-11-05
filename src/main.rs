use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use serde::Serialize;
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
    playlist: String,
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
#[derive(Clone, Serialize)]
struct Channel {
    name: String,
    url: String,
}
fn parse_playlist(text: &str, base: Option<&Url>) -> Vec<Channel> {
    let mut channels = Vec::new();
    let mut name: Option<String> = None;
    for line in text.lines() {
        let line = line.trim().trim_start_matches('\u{feff}');
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
    let base = Url::parse(source).ok();
    let mut reader: Box<dyn Read> = if let Some(url) = &base {
        if !matches!(url.scheme(), "http" | "https") {
            bail!("playlist URL must use HTTP(S)");
        }
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(20))
            .redirect(reqwest::redirect::Policy::limited(5))
            .build()?;
        Box::new(
            client
                .get(url.clone())
                .send()
                .map_err(|e| e.without_url())
                .context("playlist request failed")?
                .error_for_status()
                .map_err(|e| e.without_url())
                .context("playlist request failed")?,
        )
    } else {
        Box::new(File::open(source).context("could not open playlist")?)
    };
    let mut bytes = Vec::new();
    reader
        .by_ref()
        .take(MAX_PLAYLIST + 1)
        .read_to_end(&mut bytes)
        .context("could not read playlist")?;
    if bytes.len() as u64 > MAX_PLAYLIST {
        bail!("playlist exceeds 10 MiB");
    }
    let text = String::from_utf8(bytes).context("playlist is not UTF-8")?;
    Ok(parse_playlist(&text, base.as_ref()))
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
        let mut child = command
            .args([
                "--playlist",
                &args.playlist,
                "--web",
                "--foreground",
                "--port",
                &args.port.to_string(),
            ])
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
    let channels = load(&args.playlist)?;
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

fn serve(channels: Vec<Channel>, port: u16) -> Result<()> {
    if port == 0 {
        bail!("port must be nonzero");
    }
    let server = tiny_http::Server::http(("127.0.0.1", port))
        .map_err(|e| anyhow::anyhow!("cannot bind web server: {e}"))?;
    eprintln!("Web UI: http://127.0.0.1:{port}");
    println!("READY");
    std::io::stdout().flush()?;
    let json = serde_json::to_string(&channels)?;
    for request in server.incoming_requests() {
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
        let (code, body, content_type) = if !host_ok {
            (403, "Forbidden".to_owned(), "text/plain; charset=utf-8")
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
                "/api/channels" => (200, json.clone(), "application/json"),
                _ => (404, "Not found".to_owned(), "text/plain; charset=utf-8"),
            }
        };
        let response = tiny_http::Response::from_string(body).with_status_code(code)
            .with_header(tiny_http::Header::from_bytes("Content-Type", content_type).unwrap())
            .with_header(tiny_http::Header::from_bytes("Cache-Control", "no-store").unwrap())
            .with_header(tiny_http::Header::from_bytes("X-Content-Type-Options", "nosniff").unwrap())
            .with_header(tiny_http::Header::from_bytes("Content-Security-Policy", "default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; connect-src 'self'; media-src http: https:; base-uri 'none'; frame-ancestors 'none'").unwrap());
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
    fn default_http_port_accepts_loopback_host_without_port() {
        assert!(super::allowed_host("127.0.0.1", 80));
        assert!(super::allowed_host("localhost", 80));
        assert!(!super::allowed_host("localhost", 8080));
        assert!(!super::allowed_host("evil.example", 80));
    }
}
