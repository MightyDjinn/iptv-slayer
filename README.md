<p align="center"><img src="docs/assets/logo.svg" alt="IPTV Slayer — your playlist, your screen" width="800"></p>

# IPTV Slayer

A small Rust IPTV playlist browser with CLI playback via mpv and a local browser UI. Bring your own **legal** M3U playlist and streams; no channels are included.

## Quick start: build from source

Install [Rust and Cargo](https://rustup.rs/) and, for CLI playback, [mpv](https://mpv.io/). Clone and build:

```sh
git clone https://github.com/MightyDjinn/iptv-slayer.git
cd iptv-slayer
cargo build --release --locked
./target/release/iptv-slayer --playlist ./channels.m3u list
./target/release/iptv-slayer --playlist ./channels.m3u list --search news
./target/release/iptv-slayer --playlist ./channels.m3u play 1
./target/release/iptv-slayer --playlist ./channels.m3u --web --foreground
```

Open http://127.0.0.1:8080; stop the foreground server with Ctrl-C. Replace the file with an HTTP(S) playlist URL if desired. Channel numbers are 1-based and retain their original values after filtering; `list` is the default. `play` launches mpv. Use `--port 8081` to change the web port. Without `--foreground`, `--web` starts a detached process and prints its PID; stop it with `kill PID`. The optional startup playlist can be replaced in the browser without restarting.

## Docker Compose (Linux)

Install Docker Engine and the Compose plugin, then:

```sh
docker compose build
docker compose up -d
docker compose logs -f
# Open http://127.0.0.1:8080; Ctrl-C exits the log viewer.
docker compose down
```

In the browser, choose a local M3U file or enter an HTTP(S) playlist URL. If no `--playlist` is provided, web mode also checks `$XDG_CONFIG_HOME/iptv-slayer/channels.m3u` (default `~/.config/iptv-slayer/channels.m3u`); otherwise it prompts for a source. Docker uses the container's config path unless a host directory is mounted. Optional username and password fields use HTTP Basic authentication for the playlist request; use HTTPS to protect credentials. A successful selection is saved separately for this browser and restored after restart. Compose stores this data in the `iptv-slayer-data` named volume. No playlist is bundled. Source credentials are stored unencrypted with owner-only filesystem permissions; protect the host and backups. See [security details](docs/playlists-security.md).

The included `compose.yaml` uses Linux host networking because the app binds only to loopback; ordinary `ports:` mapping cannot reach container loopback. Docker Desktop host-network support varies by platform and settings. Host networking shares the host network namespace; do not use untrusted playlists. The foreground command keeps the container running. The image has no mpv or graphical environment, so use the host binary for CLI playback.

To run a one-shot list or use Docker without Compose:

```sh
docker build -t iptv-slayer .
docker run --rm -v "$PWD/channels.m3u:/data/channels.m3u:ro" iptv-slayer --playlist /data/channels.m3u list
docker run --rm --network host -v "$PWD/channels.m3u:/data/channels.m3u:ro" iptv-slayer --playlist /data/channels.m3u --web --foreground
```

## Browser and security

The **browser**, not the server, fetches streams. Codec, HLS, cross-origin and provider restrictions may prevent playback; there is no proxy, transcoding, DRM support or HLS JavaScript library. CLI mpv may play streams the browser cannot.

Only use trusted playlists: HTTP fetches and redirects can reach private network services, and clicking a stream directs your browser to its URL. The local UI has no authentication or TLS; do not expose or reverse-proxy it to untrusted networks. Playlist and stream URLs may contain credentials visible to other local users via process arguments, browser tools or the local API. See [playlist and security details](docs/playlists-security.md).

## Documentation and development

See the [full documentation](docs/index.md), including [installation](docs/installation.md), [usage](docs/usage.md), [security](docs/playlists-security.md), and [development](docs/development.md). When Pages is enabled, it is published at https://mightydjinn.github.io/iptv-slayer/.

```sh
cargo fmt --all -- --check
cargo test --locked --all-targets
cargo clippy --locked --all-targets -- -D warnings
python3 -m venv .venv
. .venv/bin/activate
pip install -r docs/requirements.txt
mkdocs build --strict
```

Pull requests run Rust checks, a strict MkDocs build and a Trivy filesystem scan for HIGH/CRITICAL vulnerabilities and secrets. Configure branch protection to require the checks; the workflow alone does not enforce that requirement.
