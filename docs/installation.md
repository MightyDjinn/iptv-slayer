# Installation

## Build from source

Install stable [Rust and Cargo](https://rustup.rs/) and [mpv](https://mpv.io/) for CLI playback. The browser UI does not require mpv.

```sh
git clone https://github.com/MightyDjinn/iptv-slayer.git
cd iptv-slayer
cargo build --release --locked
./target/release/iptv-slayer --help
```

The executable is `target/release/iptv-slayer`. Supply your own legal UTF-8 M3U file or HTTP(S) URL.

## Docker Compose

On Linux Docker Engine, place your playlist at `channels.m3u`, readable by container UID 10001. The included Compose file builds the image, mounts the playlist read-only and runs the web UI in the foreground:

```sh
docker compose build
docker compose up -d
docker compose logs -f
# Open http://127.0.0.1:8080
docker compose down
```

Ctrl-C exits the log viewer; `down` stops and removes the container. Edit the volume and command in `compose.yaml` to change the playlist or port. The container runs unprivileged, contains no mpv, and uses host networking: `ports:` mapping alone cannot expose its loopback listener. Host networking shares the host namespace and is not suitable for untrusted playlists. Docker Desktop support varies; use the source build if unavailable.

## Docker CLI

```sh
docker build -t iptv-slayer .
docker run --rm -v "$PWD/channels.m3u:/data/channels.m3u:ro" iptv-slayer --playlist /data/channels.m3u list
docker run --rm --network host -v "$PWD/channels.m3u:/data/channels.m3u:ro" iptv-slayer --playlist /data/channels.m3u --web --foreground
```

Stop the foreground container with Ctrl-C.
