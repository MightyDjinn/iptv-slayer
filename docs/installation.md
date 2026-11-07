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

On Linux Docker Engine, the included Compose file builds the image and runs the web UI in the foreground. No playlist file or mount is required at startup:

```sh
docker compose build
docker compose up -d
docker compose logs -f
# Open http://127.0.0.1:8080
docker compose down
```

In the browser, select a local M3U file or enter an HTTP(S) playlist URL. Optional username and password fields use HTTP Basic authentication for fetching the playlist; prefer HTTPS. The source, credentials, and playlist are saved for this browser across restarts in the `iptv-slayer-data` Docker volume. Credentials are stored unencrypted with owner-only filesystem permissions. No channels are bundled. `docker compose down` retains the volume; `docker compose down -v` deletes its contents.

If an older Compose setup reports `Is a directory`, run `docker compose config` and check for an old `--playlist /data/channels.m3u` command or bind mount, including in override files. The current Compose file needs neither. Check any host `channels.m3u` directory's contents before moving it; remove it only if known to be empty. Rebuild and recreate with `docker compose up -d --build --force-recreate`. An explicitly supplied `--playlist` still reports an error if it is not a readable file.

Ctrl-C exits the log viewer; `down` stops and removes the container. Edit the command in `compose.yaml` to change the port. The container runs unprivileged, contains no mpv, and uses host networking: `ports:` mapping alone cannot expose its loopback listener. Host networking shares the host namespace and is not suitable for untrusted playlists. Docker Desktop support varies; use the source build if unavailable.

## Docker CLI

```sh
docker build -t iptv-slayer .
docker run --rm -v "$PWD/channels.m3u:/data/channels.m3u:ro" iptv-slayer --playlist /data/channels.m3u list
docker run --rm --network host -v "$PWD/channels.m3u:/data/channels.m3u:ro" iptv-slayer --playlist /data/channels.m3u --web --foreground
```

Stop the foreground container with Ctrl-C.
