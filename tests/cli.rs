use std::{fs, process::Command};

#[test]
fn lists_and_searches_playlist() {
    let dir = tempfile::tempdir().unwrap();
    let playlist = dir.path().join("list.m3u");
    fs::write(&playlist, "#EXTM3U\n#EXTINF:-1 group-title=\"News\",World News\nhttps://example.com/live.m3u8\n#EXTINF:-1,Music\nhttps://example.com/music\n").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_iptv-slayer"))
        .args([
            "--playlist",
            playlist.to_str().unwrap(),
            "list",
            "--search",
            "news",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("World News"));
    assert!(!stdout.contains("Music"));
}

#[test]
fn reports_directory_instead_of_playlist_file() {
    let dir = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_iptv-slayer"))
        .args(["--playlist", dir.path().to_str().unwrap(), "list"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("playlist path is a directory"), "{stderr}");
}

#[test]
fn rejects_web_combined_with_list() {
    let dir = tempfile::tempdir().unwrap();
    let playlist = dir.path().join("list.m3u");
    fs::write(&playlist, "#EXTM3U\n").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_iptv-slayer"))
        .args(["--playlist", playlist.to_str().unwrap(), "--web", "list"])
        .output()
        .unwrap();
    assert!(!output.status.success());
}

#[test]
fn web_serves_channels_and_rejects_untrusted_hosts() {
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::process::Stdio;
    struct Guard(std::process::Child);
    impl Drop for Guard {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let playlist = dir.path().join("list.m3u");
    fs::write(&playlist, "#EXTM3U\n#EXTINF:-1,<img src=x onerror=alert(1)>\nhttps://example.com/secret?token=abc\n#EXTINF:-1,Invalid\nfile:///etc/passwd\n").unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let mut child = Guard(
        Command::new(env!("CARGO_BIN_EXE_iptv-slayer"))
            .args([
                "--playlist",
                playlist.to_str().unwrap(),
                "--web",
                "--foreground",
                "--port",
                &port.to_string(),
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let mut ready = String::new();
    BufReader::new(child.0.stdout.take().unwrap())
        .read_line(&mut ready)
        .unwrap();
    assert_eq!(ready.trim(), "READY");
    fn request(port: u16, method: &str, path: &str, host: &str) -> String {
        let mut conn = TcpStream::connect(("127.0.0.1", port)).unwrap();
        conn.set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        write!(
            conn,
            "{method} {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n"
        )
        .unwrap();
        let mut response = String::new();
        conn.read_to_string(&mut response).unwrap();
        response
    }
    fn get(port: u16, path: &str, host: &str) -> String {
        request(port, "GET", path, host)
    }
    let response = get(port, "/api/channels", &format!("127.0.0.1:{port}"));
    assert!(response.starts_with("HTTP/1.1 200"));
    assert!(response.contains("onerror=alert(1)"));
    assert!(!response.contains("file:///etc/passwd"));
    assert!(get(port, "/api/channels", "evil.example").starts_with("HTTP/1.1 403"));
    assert!(get(port, "/api/channels", "localhost").starts_with("HTTP/1.1 403"));
    assert!(get(port, "/api/channels", &format!("localhost:{port}")).starts_with("HTTP/1.1 200"));
    let html = get(port, "/", &format!("127.0.0.1:{port}"));
    assert!(html.contains("button.textContent"));
    assert!(!html.contains("innerHTML"));
    let headers = response.to_ascii_lowercase();
    assert!(headers.contains("content-type: application/json"));
    assert!(headers.contains("cache-control: no-store"));
    assert!(headers.contains("x-content-type-options: nosniff"));
    assert!(headers.contains("content-security-policy:"));
    let host = format!("127.0.0.1:{port}");
    assert!(get(port, "/missing", &host).starts_with("HTTP/1.1 404"));
    assert!(request(port, "POST", "/api/channels", &host).starts_with("HTTP/1.1 405"));
    assert!(request(port, "POST", "/api/channels", "evil.example").starts_with("HTTP/1.1 403"));
    let body = response.split_once("\r\n\r\n").unwrap().1;
    let channels: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(channels.as_array().unwrap().len(), 1);
    assert_eq!(channels[0]["name"], "<img src=x onerror=alert(1)>");
    assert_eq!(channels[0]["url"], "https://example.com/secret?token=abc");
}

#[test]
fn http_playlist_resolves_relative_urls() {
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::process::Stdio;
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0; 4096];
        let _ = stream.read(&mut request).unwrap();
        let body = "#EXTM3U\n#EXTINF:-1,Remote\nstream.m3u8\n";
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        )
        .unwrap();
    });
    let web_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let web_port = web_listener.local_addr().unwrap().port();
    drop(web_listener);
    let mut child = Command::new(env!("CARGO_BIN_EXE_iptv-slayer"))
        .args([
            "--playlist",
            &format!("http://127.0.0.1:{port}/list.m3u"),
            "--web",
            "--foreground",
            "--port",
            &web_port.to_string(),
        ])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut ready = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut ready)
        .unwrap();
    server.join().unwrap();
    assert_eq!(ready.trim(), "READY");
    let mut stream = TcpStream::connect(("127.0.0.1", web_port)).unwrap();
    write!(
        stream,
        "GET /api/channels HTTP/1.1\r\nHost: 127.0.0.1:{web_port}\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    child.kill().unwrap();
    child.wait().unwrap();
    assert!(
        response.contains(&format!("http://127.0.0.1:{port}/stream.m3u8")),
        "{response}"
    );
}

#[cfg(unix)]
#[test]
fn detached_web_starts_and_serves_playlist() {
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    struct Process(u32);
    impl Drop for Process {
        fn drop(&mut self) {
            // The PID comes directly from the newly launched detached child.
            unsafe {
                libc::kill(self.0 as libc::pid_t, libc::SIGTERM);
            }
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let playlist = dir.path().join("list.m3u");
    fs::write(
        &playlist,
        "#EXTM3U\n#EXTINF:-1,Detached\nhttps://example.com/live\n",
    )
    .unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let output = Command::new(env!("CARGO_BIN_EXE_iptv-slayer"))
        .args([
            "--playlist",
            playlist.to_str().unwrap(),
            "--web",
            "--port",
            &port.to_string(),
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    let pid = stdout
        .split("(PID ")
        .nth(1)
        .unwrap()
        .trim()
        .trim_end_matches(')')
        .parse()
        .unwrap();
    let _guard = Process(pid);
    assert!(stdout.contains(&format!("http://127.0.0.1:{port}")));
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .unwrap();
    write!(
        stream,
        "GET /api/channels HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    assert!(response.starts_with("HTTP/1.1 200"));
    assert!(response.contains("Detached"));
}

#[test]
fn detached_web_reports_occupied_port() {
    use std::net::TcpListener;
    let dir = tempfile::tempdir().unwrap();
    let playlist = dir.path().join("list.m3u");
    fs::write(&playlist, "#EXTM3U\n").unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let output = Command::new(env!("CARGO_BIN_EXE_iptv-slayer"))
        .args([
            "--playlist",
            playlist.to_str().unwrap(),
            "--web",
            "--port",
            &port.to_string(),
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("web process failed to start"));
}

#[test]
fn detached_web_reports_missing_playlist() {
    let dir = tempfile::tempdir().unwrap();
    let playlist = dir.path().join("missing.m3u");
    let output = Command::new(env!("CARGO_BIN_EXE_iptv-slayer"))
        .args(["--playlist", playlist.to_str().unwrap(), "--web"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("web process failed to start"));
}

#[test]
fn web_starts_without_initial_playlist() {
    use std::io::{BufRead, BufReader};
    use std::net::TcpListener;
    use std::process::Stdio;
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let mut child = Command::new(env!("CARGO_BIN_EXE_iptv-slayer"))
        .args(["--web", "--foreground", "--port", &port.to_string()])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut line = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    if line.trim() == "READY" {
        child.kill().unwrap();
    }
    child.wait().unwrap();
    assert_eq!(line.trim(), "READY");
}

#[test]
fn web_replaces_playlist_from_content() {
    use std::io::{BufRead, BufReader};
    use std::net::TcpListener;
    use std::process::Stdio;
    struct Guard(std::process::Child);
    impl Drop for Guard {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let mut child = Guard(
        Command::new(env!("CARGO_BIN_EXE_iptv-slayer"))
            .args(["--web", "--foreground", "--port", &port.to_string()])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let mut line = String::new();
    BufReader::new(child.0.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    assert_eq!(line.trim(), "READY");
    let origin = format!("http://127.0.0.1:{port}");
    let client = reqwest::blocking::Client::new();
    let response = client
        .post(format!("{origin}/api/playlist"))
        .header("Origin", &origin)
        .header("Content-Type", "application/json")
        .body(r##"{"content":"#EXTM3U\n#EXTINF:-1,New\nhttps://example.com/live\n"}"##)
        .send()
        .unwrap();
    assert_eq!(response.status(), 200);
    let value: serde_json::Value = serde_json::from_str(&response.text().unwrap()).unwrap();
    assert_eq!(value[0]["name"], "New");
    assert!(client
        .get(format!("{origin}/api/channels"))
        .send()
        .unwrap()
        .text()
        .unwrap()
        .contains("New"));
    let endpoint = format!("{origin}/api/playlist");
    let oversized = format!("{{\"content\":\"{}\"}}", "x".repeat(16 * 1024 * 1024));
    assert_eq!(
        client
            .post(&endpoint)
            .header("Origin", &origin)
            .header("Content-Type", "application/json")
            .body(oversized)
            .send()
            .unwrap()
            .status(),
        413
    );
    let payload = r##"{"content":"#EXTM3U\nhttps://example.com/other"}"##;
    assert_eq!(
        client
            .post(&endpoint)
            .header("Origin", "http://evil.example")
            .header("Content-Type", "application/json")
            .body(payload)
            .send()
            .unwrap()
            .status(),
        403
    );
    assert_eq!(
        client
            .post(&endpoint)
            .header("Content-Type", "application/json")
            .body(payload)
            .send()
            .unwrap()
            .status(),
        403
    );
    assert_eq!(
        client
            .post(&endpoint)
            .header("Origin", &origin)
            .header("Content-Type", "text/plain")
            .body(payload)
            .send()
            .unwrap()
            .status(),
        415
    );
    let failure = client
        .post(&endpoint)
        .header("Origin", &origin)
        .header("Content-Type", "application/json")
        .body(r#"{"url":"file:///etc/passwd","password":"secret"}"#)
        .send()
        .unwrap();
    assert_eq!(failure.status(), 400);
    assert!(!failure.text().unwrap().contains("secret"));
    assert!(client
        .get(format!("{origin}/api/channels"))
        .send()
        .unwrap()
        .text()
        .unwrap()
        .contains("New"));
    let upstream = TcpListener::bind("127.0.0.1:0").unwrap();
    let upstream_port = upstream.local_addr().unwrap().port();
    let worker = std::thread::spawn(move || {
        use std::io::{Read, Write};
        let (mut stream, _) = upstream.accept().unwrap();
        let mut data = [0; 8192];
        let n = stream.read(&mut data).unwrap();
        let request = String::from_utf8_lossy(&data[..n]).to_lowercase();
        assert!(
            request.contains("authorization: basic dXNlcjpwYXNz".to_lowercase().as_str()),
            "{request}"
        );
        let body = "#EXTM3U\n#EXTINF:-1,Authenticated\nstream.m3u8\n";
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        )
        .unwrap();
    });
    let body = serde_json::json!({"url":format!("http://127.0.0.1:{upstream_port}/list"),"username":"user","password":"pass"}).to_string();
    let response = client
        .post(&endpoint)
        .header("Origin", &origin)
        .header("Content-Type", "application/json")
        .body(body)
        .send()
        .unwrap();
    assert_eq!(response.status(), 200);
    let text = response.text().unwrap();
    assert!(text.contains("Authenticated"));
    assert!(!text.contains("pass"));
    assert!(text.contains(&format!("http://127.0.0.1:{upstream_port}/stream.m3u8")));
    worker.join().unwrap();
    let destination = TcpListener::bind("127.0.0.1:0").unwrap();
    destination.set_nonblocking(true).unwrap();
    let dest_port = destination.local_addr().unwrap().port();
    let redirect = TcpListener::bind("127.0.0.1:0").unwrap();
    let redirect_port = redirect.local_addr().unwrap().port();
    let worker = std::thread::spawn(move || {
        use std::io::{Read, Write};
        let (mut stream, _) = redirect.accept().unwrap();
        let mut buffer = [0; 4096];
        assert!(stream.read(&mut buffer).unwrap() > 0);
        write!(stream, "HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:{dest_port}/steal\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
    });
    let body = serde_json::json!({"url":format!("http://127.0.0.1:{redirect_port}/list"),"username":"user","password":"pass"}).to_string();
    let response = client
        .post(&endpoint)
        .header("Origin", &origin)
        .header("Content-Type", "application/json")
        .body(body)
        .send()
        .unwrap();
    assert_eq!(response.status(), 400);
    worker.join().unwrap();
    assert_eq!(
        destination.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
    assert!(client
        .get(format!("{origin}/api/channels"))
        .send()
        .unwrap()
        .text()
        .unwrap()
        .contains("Authenticated"));
}

#[test]
fn cli_requires_a_playlist() {
    let output = Command::new(env!("CARGO_BIN_EXE_iptv-slayer"))
        .arg("list")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("--playlist is required"));
}

#[cfg(unix)]
#[test]
fn detached_web_starts_without_playlist() {
    use std::net::TcpListener;
    struct Process(u32);
    impl Drop for Process {
        fn drop(&mut self) {
            unsafe {
                libc::kill(self.0 as libc::pid_t, libc::SIGTERM);
            }
        }
    }
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let output = Command::new(env!("CARGO_BIN_EXE_iptv-slayer"))
        .args(["--web", "--port", &port.to_string()])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    let pid = stdout
        .split("(PID ")
        .nth(1)
        .unwrap()
        .trim()
        .trim_end_matches(')')
        .parse()
        .unwrap();
    let _guard = Process(pid);
    let text = reqwest::blocking::get(format!("http://127.0.0.1:{port}/api/channels"))
        .unwrap()
        .text()
        .unwrap();
    assert_eq!(text, "[]");
}
