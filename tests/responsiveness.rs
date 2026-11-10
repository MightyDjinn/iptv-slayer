use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

struct Guard(Child);
impl Drop for Guard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn saturated_playlist_uploads_are_rejected_without_blocking_reads() {
    let dir = tempfile::tempdir().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let mut child = Guard(
        Command::new(env!("CARGO_BIN_EXE_iptv-slayer"))
            .args(["--web", "--foreground", "--port", &port.to_string()])
            .env("XDG_CONFIG_HOME", dir.path())
            .env("XDG_DATA_HOME", dir.path())
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
    let origin = format!("http://127.0.0.1:{port}");
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .unwrap();
    let mut uploads = Vec::new();
    for _ in 0..4 {
        let cookie = client.get(&origin).send().unwrap().headers()["set-cookie"]
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_owned();
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(stream, "POST /api/playlist HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nOrigin: {origin}\r\nCookie: {cookie}\r\nContent-Type: application/json\r\nContent-Length: 10000\r\n\r\n{{").unwrap();
        uploads.push(stream);
    }
    std::thread::sleep(Duration::from_millis(300));
    let cookie = client.get(&origin).send().unwrap().headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let response = client
        .post(format!("{origin}/api/playlist"))
        .header("Origin", &origin)
        .header("Cookie", &cookie)
        .header("Content-Type", "application/json")
        .body("{}")
        .send()
        .unwrap();
    assert_eq!(response.status(), 503);
    assert_eq!(
        client
            .get(format!("{origin}/api/channels"))
            .header("Cookie", cookie)
            .send()
            .unwrap()
            .status(),
        200
    );
}

#[test]
fn incomplete_playlist_upload_does_not_block_get() {
    let dir = tempfile::tempdir().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let mut child = Guard(
        Command::new(env!("CARGO_BIN_EXE_iptv-slayer"))
            .args(["--web", "--foreground", "--port", &port.to_string()])
            .env("XDG_CONFIG_HOME", dir.path())
            .env("XDG_DATA_HOME", dir.path())
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
    let origin = format!("http://127.0.0.1:{port}");
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .unwrap();
    let cookie = client.get(&origin).send().unwrap().headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let mut upload = TcpStream::connect(("127.0.0.1", port)).unwrap();
    write!(upload, "POST /api/playlist HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nOrigin: {origin}\r\nCookie: {cookie}\r\nContent-Type: application/json\r\nContent-Length: 10000\r\n\r\n{{").unwrap();
    std::thread::sleep(Duration::from_millis(200));
    let result = client
        .get(format!("{origin}/api/channels"))
        .header("Cookie", cookie)
        .send();
    assert!(
        result.is_ok(),
        "GET blocked by incomplete upload: {result:?}"
    );
}

#[test]
fn later_successful_selection_is_not_overwritten_by_earlier_slow_selection() {
    let dir = tempfile::tempdir().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let mut child = Guard(
        Command::new(env!("CARGO_BIN_EXE_iptv-slayer"))
            .args(["--web", "--foreground", "--port", &port.to_string()])
            .env("XDG_CONFIG_HOME", dir.path())
            .env("XDG_DATA_HOME", dir.path())
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
    let origin = format!("http://127.0.0.1:{port}");
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
        .unwrap();
    let cookie = client.get(&origin).send().unwrap().headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let upstream = TcpListener::bind("127.0.0.1:0").unwrap();
    let upstream_port = upstream.local_addr().unwrap().port();
    let (seen_tx, seen_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let fixture = std::thread::spawn(move || {
        let (mut stream, _) = upstream.accept().unwrap();
        let mut bytes = [0; 4096];
        let _ = stream.read(&mut bytes).unwrap();
        seen_tx.send(()).unwrap();
        let _ = release_rx.recv_timeout(Duration::from_secs(10));
        let body = "#EXTM3U\n#EXTINF:-1,Old\nhttp://example.com/old\n";
        let _ = write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
    });
    let old_client = client.clone();
    let old_origin = origin.clone();
    let old_cookie = cookie.clone();
    let old = std::thread::spawn(move || {
        old_client
            .post(format!("{old_origin}/api/playlist"))
            .header("Origin", &old_origin)
            .header("Cookie", old_cookie)
            .header("Content-Type", "application/json")
            .body(format!(
                "{{\"url\":\"http://127.0.0.1:{upstream_port}/playlist\"}}"
            ))
            .send()
    });
    seen_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let newer = client
        .post(format!("{origin}/api/playlist"))
        .header("Origin", &origin)
        .header("Cookie", &cookie)
        .header("Content-Type", "application/json")
        .body(
            serde_json::json!({"content":"#EXTM3U\n#EXTINF:-1,New\nhttp://example.com/new\n"})
                .to_string(),
        )
        .send();
    let _ = release_tx.send(());
    fixture.join().unwrap();
    assert_eq!(old.join().unwrap().unwrap().status(), 409);
    assert_eq!(newer.unwrap().status(), 200);
    let channels: serde_json::Value = serde_json::from_str(
        &client
            .get(format!("{origin}/api/channels"))
            .header("Cookie", cookie)
            .send()
            .unwrap()
            .text()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(channels[0]["name"], "New");
}

#[test]
fn later_failed_selection_does_not_prevent_earlier_success() {
    let dir = tempfile::tempdir().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let mut child = Guard(
        Command::new(env!("CARGO_BIN_EXE_iptv-slayer"))
            .args(["--web", "--foreground", "--port", &port.to_string()])
            .env("XDG_CONFIG_HOME", dir.path())
            .env("XDG_DATA_HOME", dir.path())
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
    let origin = format!("http://127.0.0.1:{port}");
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
        .unwrap();
    let cookie = client.get(&origin).send().unwrap().headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let upstream = TcpListener::bind("127.0.0.1:0").unwrap();
    let upstream_port = upstream.local_addr().unwrap().port();
    let (seen_tx, seen_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let fixture = std::thread::spawn(move || {
        let (mut stream, _) = upstream.accept().unwrap();
        let mut bytes = [0; 4096];
        let _ = stream.read(&mut bytes).unwrap();
        seen_tx.send(()).unwrap();
        let _ = release_rx.recv_timeout(Duration::from_secs(10));
        let body = "#EXTM3U\n#EXTINF:-1,Old\nhttp://example.com/old\n";
        let _ = write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
    });
    let old_client = client.clone();
    let old_origin = origin.clone();
    let old_cookie = cookie.clone();
    let old = std::thread::spawn(move || {
        old_client
            .post(format!("{old_origin}/api/playlist"))
            .header("Origin", &old_origin)
            .header("Cookie", old_cookie)
            .header("Content-Type", "application/json")
            .body(format!(
                "{{\"url\":\"http://127.0.0.1:{upstream_port}/playlist\"}}"
            ))
            .send()
    });
    seen_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let newer = client
        .post(format!("{origin}/api/playlist"))
        .header("Origin", &origin)
        .header("Cookie", &cookie)
        .header("Content-Type", "application/json")
        .body(serde_json::json!({"content":"not a playlist"}).to_string())
        .send();
    let _ = release_tx.send(());
    fixture.join().unwrap();
    assert_eq!(old.join().unwrap().unwrap().status(), 200);
    assert_eq!(newer.unwrap().status(), 400);
    let channels: serde_json::Value = serde_json::from_str(
        &client
            .get(format!("{origin}/api/channels"))
            .header("Cookie", cookie)
            .send()
            .unwrap()
            .text()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(channels[0]["name"], "Old");
}

#[test]
fn slow_playlist_fetch_does_not_block_get() {
    let dir = tempfile::tempdir().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let mut child = Guard(
        Command::new(env!("CARGO_BIN_EXE_iptv-slayer"))
            .args(["--web", "--foreground", "--port", &port.to_string()])
            .env("XDG_CONFIG_HOME", dir.path())
            .env("XDG_DATA_HOME", dir.path())
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
    let origin = format!("http://127.0.0.1:{port}");
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .unwrap();
    let cookie = client.get(&origin).send().unwrap().headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let upstream = TcpListener::bind("127.0.0.1:0").unwrap();
    let upstream_port = upstream.local_addr().unwrap().port();
    let (seen_tx, seen_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let fixture = std::thread::spawn(move || {
        let (mut stream, _) = upstream.accept().unwrap();
        let mut bytes = [0; 4096];
        let _ = stream.read(&mut bytes).unwrap();
        seen_tx.send(()).unwrap();
        let _ = release_rx.recv_timeout(Duration::from_secs(10));
    });
    let body = format!("{{\"url\":\"http://127.0.0.1:{upstream_port}/playlist\"}}");
    let mut post = TcpStream::connect(("127.0.0.1", port)).unwrap();
    write!(post, "POST /api/playlist HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nOrigin: {origin}\r\nCookie: {cookie}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
    seen_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let result = client
        .get(format!("{origin}/api/channels"))
        .header("Cookie", cookie)
        .send();
    release_tx.send(()).unwrap();
    fixture.join().unwrap();
    assert!(result.is_ok(), "GET blocked by slow playlist: {result:?}");
}

#[test]
fn incomplete_request_deadlines_release_resources() {
    let dir = tempfile::tempdir().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let mut child = Guard(
        Command::new(env!("CARGO_BIN_EXE_iptv-slayer"))
            .args(["--web", "--foreground", "--port", &port.to_string()])
            .env("XDG_CONFIG_HOME", dir.path())
            .env("XDG_DATA_HOME", dir.path())
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
    let origin = format!("http://127.0.0.1:{port}");
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
        .unwrap();
    let cookie = client.get(&origin).send().unwrap().headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let mut header = TcpStream::connect(("127.0.0.1", port)).unwrap();
    header
        .set_read_timeout(Some(Duration::from_secs(8)))
        .unwrap();
    write!(header, "GET / HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n").unwrap();
    let mut body = TcpStream::connect(("127.0.0.1", port)).unwrap();
    body.set_read_timeout(Some(Duration::from_secs(14)))
        .unwrap();
    write!(body, "POST /api/playlist HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nOrigin: {origin}\r\nCookie: {cookie}\r\nContent-Type: application/json\r\nContent-Length: 10000\r\n\r\n{{").unwrap();
    let mut trickle = body.try_clone().unwrap();
    let sender = std::thread::spawn(move || {
        for _ in 0..20 {
            std::thread::sleep(Duration::from_millis(700));
            if trickle.write_all(b" ").is_err() {
                break;
            }
        }
    });
    let mut header_response = Vec::new();
    header.read_to_end(&mut header_response).unwrap();
    assert!(
        header_response.is_empty() || String::from_utf8_lossy(&header_response).contains("408")
    );
    let mut body_response = String::new();
    body.read_to_string(&mut body_response).unwrap();
    assert!(body_response.starts_with("HTTP/1.1 408"), "{body_response}");
    sender.join().unwrap();
    assert_eq!(
        client
            .get(format!("{origin}/api/channels"))
            .header("Cookie", cookie)
            .send()
            .unwrap()
            .status(),
        200
    );
}

#[test]
fn disconnected_fetches_keep_worker_permits_until_work_finishes() {
    let dir = tempfile::tempdir().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let mut child = Guard(
        Command::new(env!("CARGO_BIN_EXE_iptv-slayer"))
            .args(["--web", "--foreground", "--port", &port.to_string()])
            .env("XDG_CONFIG_HOME", dir.path())
            .env("XDG_DATA_HOME", dir.path())
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
    let origin = format!("http://127.0.0.1:{port}");
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .unwrap();
    let cookie = client.get(&origin).send().unwrap().headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let upstream = TcpListener::bind("127.0.0.1:0").unwrap();
    let upstream_port = upstream.local_addr().unwrap().port();
    let (seen_tx, seen_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let fixture = std::thread::spawn(move || {
        let mut streams = Vec::new();
        for _ in 0..4 {
            let (mut stream, _) = upstream.accept().unwrap();
            let mut bytes = [0; 4096];
            assert!(stream.read(&mut bytes).unwrap() > 0);
            streams.push(stream);
            seen_tx.send(()).unwrap();
        }
        release_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        drop(streams);
    });
    for _ in 0..4 {
        let body = format!("{{\"url\":\"http://127.0.0.1:{upstream_port}/playlist\"}}");
        let mut post = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(post, "POST /api/playlist HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nOrigin: {origin}\r\nCookie: {cookie}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
        seen_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        drop(post);
    }
    let response = client
        .post(format!("{origin}/api/playlist"))
        .header("Origin", &origin)
        .header("Cookie", &cookie)
        .header("Content-Type", "application/json")
        .body("{}")
        .send()
        .unwrap();
    assert_eq!(response.status(), 503);
    release_tx.send(()).unwrap();
    fixture.join().unwrap();
}

#[test]
fn slow_readers_saturate_response_capacity_and_disconnect_recovers_it() {
    let dir = tempfile::tempdir().unwrap();
    let playlist = dir.path().join("large.m3u");
    std::fs::write(
        &playlist,
        format!(
            "#EXTM3U\n#EXTINF:-1,{}\nhttp://example.com/stream\n",
            "x".repeat(8 * 1024 * 1024)
        ),
    )
    .unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let mut child = Guard(
        Command::new(env!("CARGO_BIN_EXE_iptv-slayer"))
            .args([
                "--web",
                "--foreground",
                "--port",
                &port.to_string(),
                "--playlist",
            ])
            .arg(&playlist)
            .env("XDG_CONFIG_HOME", dir.path())
            .env("XDG_DATA_HOME", dir.path())
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
    let origin = format!("http://127.0.0.1:{port}");
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap();
    let cookie = client.get(&origin).send().unwrap().headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let mut readers = Vec::new();
    for _ in 0..4 {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        write!(
            stream,
            "GET /api/channels HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nCookie: {cookie}\r\n\r\n"
        )
        .unwrap();
        let mut header = Vec::new();
        while !header.ends_with(b"\r\n\r\n") {
            let mut byte = [0];
            stream.read_exact(&mut byte).unwrap();
            header.push(byte[0]);
        }
        assert!(header.starts_with(b"HTTP/1.1 200"));
        readers.push(stream);
    }
    let status = client
        .get(format!("{origin}/api/channels"))
        .header("Cookie", &cookie)
        .send()
        .unwrap()
        .status();
    assert_eq!(status, 503, "slow responses must retain capacity");
    drop(readers.pop());
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        let response = client
            .get(format!("{origin}/api/channels"))
            .header("Cookie", &cookie)
            .send()
            .unwrap();
        if response.status() == 200 {
            break;
        }
        assert_eq!(response.status(), 503);
        assert!(
            std::time::Instant::now() < deadline,
            "capacity did not recover after disconnect"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    // The remaining readers never consume their bodies or disconnect. The
    // connection deadline must still reclaim their response capacity.
    let deadline = std::time::Instant::now() + Duration::from_secs(45);
    std::thread::sleep(
        deadline.saturating_duration_since(std::time::Instant::now()) - Duration::from_secs(4),
    );
    // The timed-out sockets may still contain kernel-buffered data, but the
    // server must have dropped its body allocations and released permits.
    let mut replacements = Vec::new();
    for _ in 0..4 {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        write!(
            stream,
            "GET /api/channels HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nCookie: {cookie}\r\n\r\n"
        )
        .unwrap();
        let mut header = Vec::new();
        while !header.ends_with(b"\r\n\r\n") {
            let mut byte = [0];
            stream.read_exact(&mut byte).unwrap();
            header.push(byte[0]);
        }
        assert!(
            header.starts_with(b"HTTP/1.1 200"),
            "timed-out readers retained capacity: {}",
            String::from_utf8_lossy(&header)
        );
        replacements.push(stream);
    }
}
