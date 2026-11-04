# IPTV Slayer

A small Rust IPTV playlist client. Bring your own legal M3U playlist and streams; no channels are included. Rust and Cargo are needed to build, and `mpv` is required for CLI playback.

```sh
cargo build --release --locked
./target/release/iptv-slayer --playlist ./channels.m3u list
./target/release/iptv-slayer --playlist ./channels.m3u list --search news
./target/release/iptv-slayer --playlist ./channels.m3u play 1
./target/release/iptv-slayer --playlist https://example.org/playlist.m3u --web
```

`list` is the default action. Channel numbers are 1-based and remain the same when filtering. `play` invokes `mpv -- URL` without a shell and waits for it to exit. `--web` starts a detached child, waits until it has loaded the playlist and bound the port, prints its PID and returns. Stop it with `kill PID`; there is no PID file, restart policy or log file. Use `--web --foreground` to run under a service manager or diagnose startup failures. The UI is at http://127.0.0.1:8080; change the port with `--port 8081`. It binds IPv4 loopback only and offers no remote-bind option.

The browser UI has name search, a channel list and a native HTML video player. The **browser**, not the server, requests the selected stream. Browser codec/container and HLS support vary; streams requiring provider-specific headers, DRM, authentication, or unsupported formats may not play. Cross-origin/provider restrictions may also prevent playback. There is no HLS JavaScript library, transcoding, proxy, or mpv integration in the web UI; CLI mpv may play streams the browser cannot. A failed video reports an error. Playlist is loaded once at startup; restart to refresh.

## Playlist support and security

UTF-8 extended M3U: `#EXTINF` supplies the name after the first comma; a following HTTP(S) URL supplies the stream. Bare URL lines get generated names. Other comments and attributes are ignored. HTTP(S) relative stream URLs resolve against the originally supplied playlist URL; local relative stream paths and non-HTTP(S) streams are ignored. A maximum of 10 MiB is read; HTTP requests time out after 20 seconds and allow up to five redirects. No EPG, credentials management, `#EXTVLCOPT`, or advanced M3U parsing is provided. Empty or unsupported playlists result in an empty list.

Only use trusted playlist sources. The operator explicitly chooses the playlist URL: the initial HTTP fetch and redirects **can reach private network services**, so do not expose this CLI as an untrusted URL-fetching service. The web API has no URL-fetching or arbitrary command endpoint; it serves only the loaded channels. Loopback-only binding and a Host header check help restrict access and DNS rebinding; there is no authentication or TLS. Do not publish or reverse-proxy the UI to an untrusted network. Other local users can access it. Channel names are inserted as text, not HTML, and only HTTP(S) stream URLs are accepted. Selecting a channel contacts its server from the browser; a malicious playlist can therefore direct the browser to network destinations accessible from that browser.

Playlist and stream URLs can contain credentials. Playlist URL arguments (including background child arguments) and mpv stream arguments may be visible to local process inspection or shell history. Stream URLs are exposed in the local `/api/channels` response and browser developer tools, and providers receive them when played. Avoid sensitive URLs on shared systems. The list output does not display stream URLs. Error messages avoid printing HTTP request URLs.

## Docker

```sh
docker build -t iptv-slayer .
docker run --rm -v "$PWD/channels.m3u:/data/channels.m3u:ro" iptv-slayer --playlist /data/channels.m3u list
# Linux host networking is required for the loopback-only server to be reachable on the host:
docker run --rm --network host -v "$PWD/channels.m3u:/data/channels.m3u:ro" iptv-slayer --playlist /data/channels.m3u --web --foreground
```

Host networking shares the host network namespace; it is not appropriate for untrusted playlists. `-p` alone cannot reach a service listening on container loopback. Desktop Docker host networking behavior varies. The image has no mpv or graphical environment; use host CLI playback. Foreground mode keeps the container alive; stop with Ctrl-C. The runtime runs as an unprivileged user, so the mounted playlist must be readable by UID 10001.

## Development

Pull requests run formatting, all-target tests, and Clippy on stable Rust. The tests use temporary playlists and loopback HTTP servers; no external IPTV services or mpv are needed. Run the same checks locally:

```sh
cargo fmt --all -- --check
cargo test --locked --all-targets
cargo clippy --locked --all-targets -- -D warnings
```
