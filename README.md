<p align="center"><img src="docs/assets/logo.svg" alt="IPTV Slayer — your playlist, your screen" width="800"></p>

# IPTV Slayer

A small Rust IPTV playlist browser with a local web UI and CLI playback via mpv. Bring your own **legal** UTF-8 M3U playlist and streams; no channels are included.

## Get started: Docker web UI

On Linux, install Docker Engine and the Compose plugin, then clone the repository and start the service:

```sh
git clone https://github.com/MightyDjinn/iptv-slayer.git
cd iptv-slayer
docker compose up -d
docker compose logs -f
```

1. Open **http://127.0.0.1:8080** on the Docker host.
2. Select a local M3U file, or enter an HTTP(S) playlist URL. For a URL, optional username and password fields provide HTTP Basic authentication for the playlist request; use HTTPS.
3. Load the playlist, choose a channel, and play it in the browser. Search filters the channel list by name.

No playlist file or mount is needed for first run. Ctrl-C exits the log viewer; `docker compose down` stops and removes the container while retaining its data volume. `docker compose down -v` **deletes** the volume and saved data.

The included Compose file uses Linux host networking because the app listens only on IPv4 loopback; ordinary `ports:` publishing cannot reach container loopback. Host networking shares the host network namespace. Docker Desktop support varies by platform and settings; use the source build if host networking is unavailable. The image has no mpv or graphical environment.

Compose uses `pull_policy: build` to build from the checkout on each `up`, while still using Docker's build cache. This adds build-check time and requires a Compose version supporting that policy; `--no-build` bypasses it.

## Build and run from source

Install stable [Rust and Cargo](https://rustup.rs/); install [mpv](https://mpv.io/) separately for CLI playback. The browser UI does not require mpv.

```sh
git clone https://github.com/MightyDjinn/iptv-slayer.git
cd iptv-slayer
cargo build --release --locked
./target/release/iptv-slayer --web --foreground
```

Open http://127.0.0.1:8080 and select a playlist as above. Stop the server with Ctrl-C. Use `--port 8081` for another nonzero port. Without `--foreground`, `--web` starts a detached process and prints its PID; stop it with `kill PID`. Use foreground mode to diagnose startup errors.

An explicit `--playlist` path or HTTP(S) URL takes precedence over the default startup file. Without it, web mode checks `$XDG_CONFIG_HOME/iptv-slayer/channels.m3u` (or `~/.config/iptv-slayer/channels.m3u` when unset or relative); if absent or unreadable, the UI prompts for a source. In Docker this is the container's config directory, not the host's. An explicitly supplied playlist that cannot be read causes startup to fail; a readable playlist with no supported channels yields an empty list. The browser can select a different playlist without restarting.

Playlists are limited to 10 MiB of input, 100,000 supported channels, and 10 MiB total for parsed channel names and resolved URLs. Relative URLs can make the parsed result larger than the input. Oversized playlists are rejected; see [playlist limits](docs/playlists-security.md).

## Saved sources and security

A successful browser selection saves the source configuration and a snapshot of its channels on the server. Uploaded file contents are saved too, although browsers cannot repopulate the file picker. Saved URLs are **not automatically refreshed**; use **Load playlist** to fetch a URL again.

A random HttpOnly cookie identifies each browser's saved configuration, not an authenticated account. Clearing cookies, switching browsers, or using a different hostname creates a separate identity; clearing a cookie does not delete the old data. Anyone with the cookie can access its settings. No automatic cleanup or aggregate storage quota is provided.

Compose persists data in a project-prefixed named volume using the `iptv-slayer-data` volume key. On Unix, source builds use `IPTV_SLAYER_DATA_DIR` if set, otherwise `$XDG_DATA_HOME/iptv-slayer` or `~/.local/share/iptv-slayer`. Secure persistent storage is unavailable on non-Unix systems, so browser selections cannot be saved there. **Passwords and other source data are stored unencrypted**, with owner-only filesystem permissions; protect the host and backups. Basic credentials are for fetching the playlist, not video playback.

The web UI has no authentication or TLS and is intended for local use; do not expose or reverse-proxy it to untrusted networks. Other local users can access it. Use trusted playlists only: playlist fetches can reach private network services, and selecting a stream directs your browser to its URL. Playlist and stream URLs can contain credentials visible in process arguments, shell history, browser tools, or the local API. See [playlist and security details](docs/playlists-security.md).

The browser fetches streams directly. Codec, HLS, cross-origin, provider authentication, or DRM restrictions can prevent playback; there is no proxy, transcoding, DRM support, or HLS JavaScript library. CLI mpv may play streams the browser cannot.

## Troubleshooting

- **No channels:** Check UTF-8, M3U formatting, and HTTP(S) stream URLs; browser uploads reject invalid UTF-8 without replacing the current playlist. See [playlist support](docs/playlists-security.md).
- **Playlist fails to load or server is busy:** Check the playlist limits; concurrent work is bounded, so retry a busy request after other loads finish.
- **Cannot open the UI or port already in use:** Check `docker compose logs` or run the source server in foreground; choose another nonzero port if needed. For Compose, add `"--port", "8081"` to the `command` list in `compose.yaml` and recreate the container. Published Docker ports alone do not reach the container's loopback listener.
- **Old container says `--playlist` is required, or reports `Is a directory`:** Check the checkout and `docker compose config` for old commands, bind mounts, or override files. The current Compose file needs no playlist mount. Run `docker compose up -d --build --force-recreate` to rebuild and recreate without deleting the data volume. Check a host `channels.m3u` directory's contents before moving it; remove it only if known to be empty.
- **Explicitly mounted playlist cannot be read:** Check that the file exists and is readable by container UID 10001. The browser file picker needs no mount.
- **Browser playback fails:** Check provider access, browser format and cross-origin restrictions; try CLI mpv where appropriate.

## CLI and Docker commands

For the source build, supply a local regular file or HTTP(S) playlist URL. Special files such as named pipes are rejected:

```sh
./target/release/iptv-slayer --playlist ./channels.m3u list
./target/release/iptv-slayer --playlist ./channels.m3u list --search news
./target/release/iptv-slayer --playlist ./channels.m3u play 1
./target/release/iptv-slayer --playlist ./channels.m3u --web --foreground
./target/release/iptv-slayer --help
```

`list` is the default command; search is case-insensitive and filtered results retain their original 1-based channel numbers. `play` launches mpv and waits for it to exit. `--web` cannot be combined with a subcommand.

To run without Compose:

```sh
docker build -t iptv-slayer .
docker run --rm --network host iptv-slayer
docker run --rm -v "$PWD/channels.m3u:/data/channels.m3u:ro" iptv-slayer --playlist /data/channels.m3u list
docker run --rm --network host -v "$PWD/channels.m3u:/data/channels.m3u:ro" iptv-slayer --playlist /data/channels.m3u --web --foreground
```

With no arguments the image runs the foreground web UI; Ctrl-C stops it. Supplying arguments replaces that default. Mount a data volume at `/var/lib/iptv-slayer` to retain browser data after container removal. CLI playback requires mpv on the host; it is not included in the image.

## Development and documentation

See the [full documentation](docs/index.md), including [installation](docs/installation.md), [usage](docs/usage.md), [security](docs/playlists-security.md), and [development](docs/development.md). When Pages is enabled, it is published at https://mightydjinn.github.io/iptv-slayer/.

CI runs the following Rust and Node checks, plus a Docker Compose integration test and a Trivy filesystem scan for HIGH/CRITICAL vulnerabilities and secrets:

```sh
cargo fmt --all -- --check
cargo test --locked --all-targets
node --test tests/source-selection.test.cjs
cargo clippy --locked --all-targets -- -D warnings
node --test tests/playback.test.cjs
node --test tests/docker-config.test.cjs
# Linux Docker Engine and Compose required; builds an image
./tests/docker-integration.py
```

Build the documentation with Python:

```sh
python3 -m venv .venv
. .venv/bin/activate
pip install -r docs/requirements.txt
mkdocs build --strict
```
