# Usage

These examples assume the source build. Replace the executable path or run inside the container as appropriate.

```sh
./target/release/iptv-slayer --playlist ./channels.m3u
./target/release/iptv-slayer --playlist ./channels.m3u list --search news
./target/release/iptv-slayer --playlist ./channels.m3u play 1
./target/release/iptv-slayer --playlist https://example.org/playlist.m3u --web --foreground --port 8081
```

`list` is the default. Search matches names case-insensitively; the displayed 1-based channel numbers remain unchanged after filtering. `play` invokes `mpv -- URL` without a shell and waits for it to exit. Install mpv and choose a valid channel number.

The foreground UI runs until Ctrl-C; open http://127.0.0.1:8081 for the example above. Omit `--foreground` to start a detached child: startup waits for loading and binding, prints the PID, and returns. Stop it with `kill PID`. There is no PID file, restart policy, or log file. Use foreground mode to diagnose startup errors. `--web` cannot be combined with a subcommand. The default port is 8080, the listener is IPv4 loopback only, and there is no remote-bind option. The optional startup playlist can be replaced from the browser without restarting.

Start with `--web --foreground` and no `--playlist` to choose a source after opening the browser. On startup, web mode first checks `$XDG_CONFIG_HOME/iptv-slayer/channels.m3u` (or `~/.config/iptv-slayer/channels.m3u` if `XDG_CONFIG_HOME` is unset or relative); an explicit `--playlist` takes precedence. If the default is absent or cannot be read, the site starts and prompts you to choose a source. In Docker this is the container's config directory, not the host's, unless explicitly mounted. Select a local M3U file, or enter an HTTP(S) URL and optionally an HTTP Basic username and password. Use HTTPS to avoid sending those credentials in clear text. The browser uploads the file or asks the local server to fetch the URL; a successful load replaces and saves that browser's playlist and source configuration, including credentials. A random browser cookie identifies the saved configuration; clearing cookies, switching browser or using a different host name creates a separate identity. Saved channels are a snapshot; press Load playlist to refresh a URL. The file picker cannot be repopulated by the browser, but its uploaded contents are saved. Source credentials are stored unencrypted with owner-only filesystem permissions. On Unix, storage is under `IPTV_SLAYER_DATA_DIR` if set, otherwise `$XDG_DATA_HOME/iptv-slayer` or `~/.local/share/iptv-slayer`. Docker Compose uses a named volume. The browser offers name search, a channel list and native video playback. The browser contacts the stream itself; codec/container, HLS, cross-origin, provider authentication and DRM restrictions can prevent playback. There is no HLS JavaScript library, proxy, transcoding or mpv integration in the UI. Try CLI mpv if a supported stream fails in the browser.

## Troubleshooting

- **No channels:** check UTF-8, supported HTTP(S) stream URLs and playlist formatting; see [playlist support](playlists-security.md).
- **Bind failure:** choose another nonzero port and use foreground mode to see the error.
- **Container cannot read a startup playlist:** check any explicitly mounted host file exists and is readable by UID 10001. The browser file picker does not require a mount.
- **Browser cannot reach container:** use supported host networking; published ports alone do not reach container loopback.
- **Browser playback fails:** check the provider, browser format and cross-origin restrictions; use mpv where appropriate.
