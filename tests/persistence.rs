use std::{
    io::{Read, Write},
    net::TcpListener,
    process::{Child, Command, Stdio},
    thread,
    time::Duration,
};
struct Server(Child);
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn start(port: u16, dir: &std::path::Path) -> Server {
    let child = Server(
        Command::new(env!("CARGO_BIN_EXE_iptv-slayer"))
            .args(["--web", "--foreground", "--port", &port.to_string()])
            .env("IPTV_SLAYER_DATA_DIR", dir)
            .env("XDG_CONFIG_HOME", dir)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    for _ in 0..100 {
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return child;
        }
        thread::sleep(Duration::from_millis(30));
    }
    panic!("server did not start")
}
#[test]
fn browser_selection_is_isolated_and_survives_restart() {
    let dir = tempfile::tempdir().unwrap();
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let base = format!("http://127.0.0.1:{port}");
    let client = reqwest::blocking::Client::new();
    let server = start(port, dir.path());
    let get_cookie = || {
        let r = client.get(format!("{base}/")).send().unwrap();
        let h = r
            .headers()
            .get("set-cookie")
            .expect("browser cookie")
            .to_str()
            .unwrap();
        assert!(h.contains("HttpOnly") && h.contains("SameSite=Lax") && h.contains("Path=/"));
        h.split(';').next().unwrap().to_owned()
    };
    let a = get_cookie();
    let b = get_cookie();
    assert_ne!(a, b);
    let renewed = client
        .get(format!("{base}/"))
        .header("Cookie", &a)
        .send()
        .unwrap();
    let renewed = renewed.headers()["set-cookie"].to_str().unwrap();
    assert_eq!(renewed.split(';').next().unwrap(), a);
    assert!(renewed.contains("Max-Age=31536000"));
    let post = |cookie: &str, body: serde_json::Value| {
        client
            .post(format!("{base}/api/playlist"))
            .header("Origin", &base)
            .header("Cookie", cookie)
            .header("Content-Type", "application/json")
            .body(body.to_string())
            .send()
            .unwrap()
    };
    assert_eq!(
        post(
            &a,
            serde_json::json!({"content":"#EXTINF:-1,One\nhttps://example.com/one\n"})
        )
        .status(),
        200
    );
    let channels = |cookie: &str| {
        client
            .get(format!("{base}/api/channels"))
            .header("Cookie", cookie)
            .send()
            .unwrap()
            .text()
            .unwrap()
    };
    assert!(channels(&a).contains("One"));
    assert!(!channels(&b).contains("One"));
    assert_eq!(
        client
            .get(format!("{base}/api/channels"))
            .header("Cookie", "iptv_client=../../etc")
            .send()
            .unwrap()
            .status(),
        401
    );
    assert_eq!(
        client
            .get(format!("{base}/api/channels"))
            .send()
            .unwrap()
            .status(),
        401
    );
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let up = listener.local_addr().unwrap().port();
    let worker = thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        let mut buf = [0; 4096];
        let n = s.read(&mut buf).unwrap();
        assert!(String::from_utf8_lossy(&buf[..n])
            .to_lowercase()
            .contains("authorization: basic"));
        let body = "#EXTINF:-1,Remote\nhttps://example.org/stream?token=secret\n";
        write!(
            s,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        )
        .unwrap();
    });
    assert_eq!(post(&b, serde_json::json!({"url": format!("http://127.0.0.1:{up}/private"), "username":"myuser", "password":"mypassword"})).status(),200);
    worker.join().unwrap();
    let denied = TcpListener::bind("127.0.0.1:0").unwrap();
    let denied_port = denied.local_addr().unwrap().port();
    let worker = thread::spawn(move || {
        let (mut stream, _) = denied.accept().unwrap();
        let mut buf = [0; 4096];
        let _ = stream.read(&mut buf).unwrap();
        stream
            .write_all(
                b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            )
            .unwrap();
    });
    assert_eq!(post(&b, serde_json::json!({"url": format!("http://127.0.0.1:{denied_port}/"), "username":"bad", "password":"wrong"})).status(), 400);
    worker.join().unwrap();
    assert_eq!(
        client
            .get(format!("{base}/api/source"))
            .header("Cookie", &b)
            .header("Origin", "http://evil.example")
            .send()
            .unwrap()
            .status(),
        403
    );
    assert_eq!(
        client
            .get(format!("{base}/api/source"))
            .send()
            .unwrap()
            .status(),
        401
    );
    drop(server);
    let _server = start(port, dir.path());
    assert!(channels(&a).contains("One"));
    assert!(channels(&b).contains("Remote"));
    let aid = a.split('=').nth(1).unwrap();
    let bid = b.split('=').nth(1).unwrap();
    let bytes = std::fs::read_to_string(dir.path().join(bid).join("channels.json")).unwrap();
    assert!(bytes.contains("mypassword") && bytes.contains("myuser") && bytes.contains("/private"));
    let source: serde_json::Value = serde_json::from_str(
        &client
            .get(format!("{base}/api/source"))
            .header("Cookie", &b)
            .send()
            .unwrap()
            .text()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(source["username"], "myuser");
    assert_eq!(source["password"], "mypassword");
    assert!(source["url"].as_str().unwrap().ends_with("/private"));
    let local: serde_json::Value = serde_json::from_str(
        &client
            .get(format!("{base}/api/source"))
            .header("Cookie", &a)
            .send()
            .unwrap()
            .text()
            .unwrap(),
    )
    .unwrap();
    assert!(local["content"].as_str().unwrap().contains("One"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(dir.path().join(aid))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(
            std::fs::metadata(dir.path().join(aid).join("channels.json"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}
#[cfg(unix)]
#[test]
fn symlink_client_directory_is_not_followed() {
    use std::os::unix::fs::symlink;
    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let base = format!("http://127.0.0.1:{port}");
    let _server = start(port, dir.path());
    let c = reqwest::blocking::Client::new();
    let response = c.get(format!("{base}/")).send().unwrap();
    let cookie = response.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap();
    let id = cookie.split('=').nth(1).unwrap();
    symlink(outside.path(), dir.path().join(id)).unwrap();
    assert_eq!(
        c.get(format!("{base}/api/channels"))
            .header("Cookie", cookie)
            .send()
            .unwrap()
            .status(),
        500
    );
    let response = c
        .post(format!("{base}/api/playlist"))
        .header("Origin", &base)
        .header("Cookie", cookie)
        .header("Content-Type", "application/json")
        .body(r#"{"content":"https://example.com/stream"}"#)
        .send()
        .unwrap();
    assert_eq!(response.status(), 500);
    assert!(!outside.path().join("channels.json").exists());
}
