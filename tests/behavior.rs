use std::{fs, process::Command};

fn run(playlist: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_iptv-slayer"))
        .arg("--playlist")
        .arg(playlist)
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn list_preserves_channel_numbers_and_handles_playlist_edge_cases() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("playlist.m3u");
    fs::write(&path, "\u{feff}#EXTM3U\r\n#EXTINF:-1,First\r\nhttps://example.com/first\r\n#EXTINF:-1,Ignored\r\nfile:///tmp/secret\r\nhttps://example.com/bare\r\n#EXTINF:-1,NEWS, International\r\nhttps://example.com/news\r\n").unwrap();
    let all = run(&path, &[]);
    assert!(all.status.success());
    assert_eq!(
        String::from_utf8(all.stdout).unwrap(),
        "1\tFirst\n2\tChannel 2\n3\tNEWS, International\n"
    );
    let filtered = run(&path, &["list", "--search", "news"]);
    assert!(filtered.status.success());
    assert_eq!(
        String::from_utf8(filtered.stdout).unwrap(),
        "3\tNEWS, International\n"
    );
}

#[test]
fn invalid_play_numbers_fail_before_invoking_player() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("playlist.m3u");
    fs::write(&path, "#EXTINF:-1,One\nhttps://example.com/one\n").unwrap();
    for number in ["0", "2"] {
        let output = run(&path, &["play", number]);
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("channel number out of range"));
    }
}

#[test]
fn local_playlist_failures_are_reported() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("playlist.m3u");
    let missing = run(&path, &[]);
    assert!(!missing.status.success());
    assert!(String::from_utf8_lossy(&missing.stderr).contains("could not open playlist"));
    fs::write(&path, [0xff]).unwrap();
    let invalid = run(&path, &[]);
    assert!(!invalid.status.success());
    assert!(String::from_utf8_lossy(&invalid.stderr).contains("playlist is not UTF-8"));
    fs::write(&path, vec![b'x'; 10 * 1024 * 1024 + 1]).unwrap();
    let oversized = run(&path, &[]);
    assert!(!oversized.status.success());
    assert!(String::from_utf8_lossy(&oversized.stderr).contains("playlist exceeds 10 MiB"));
}

#[test]
fn rejects_foreground_without_web_and_non_http_playlist_url() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("playlist.m3u");
    fs::write(&path, "#EXTM3U\n").unwrap();
    let output = run(&path, &["--foreground"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("--web"));
    let output = Command::new(env!("CARGO_BIN_EXE_iptv-slayer"))
        .args(["--playlist", "file:///etc/passwd"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("playlist URL must use HTTP(S)"));
}
