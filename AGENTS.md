# AGENTS.md — instructions for AI coding agents

This file is the entry point for any AI agent (or human) modifying CamView.
Read it completely before touching the code. The project is a **native Linux
desktop application** with strict memory, connection and security constraints.

## 1. Project mission

CamView is a lightweight multi-camera RTSP viewer and recorder for low-power
Linux machines. It must remain usable on a 1.8 GB RAM Intel Atom tablet with
three 1080p/1440p cameras while another application may also be using the
cameras. Do not turn it into a web app, an Electron app, a cloud service or a
full NVR.

## 2. Non-negotiable invariants

These are product and safety invariants, not suggestions. If a task seems to
require breaking one, stop and explain the trade-off before changing it.

1. **One RTSP session per camera.** Viewing and recording share the same
   `libmpv` instance through mpv's `stream-record` property. Do not add a
   second connection per camera, a background ffmpeg recorder per camera, or a
   "preview" connection. Many consumer cameras accept only two sessions total.
2. **Local tunnels only on `127.0.0.1`.** `socat` listeners must include
   `bind=127.0.0.1`. Never bind `0.0.0.0`. Never expose a camera port on the LAN.
3. **Never modify external playlists or services.** The legacy M3U import
   reads `~/cameras.m3u` / `~/camaras.m3u` and systemd user units in read-only
   mode. Do not write to them, disable them or change another player's settings.
4. **No transcoding while recording.** Recording is a stream copy into MKV
   (H.264/AAC/PCM). Re-encoding would exceed the CPU budget.
5. **No secrets in Git.** Never commit `config.toml`, recordings, snapshots,
   logs, tokens, certificates or real camera/IP/credential data. The app writes
   credentials to a `0600` config; logs and UI must redact URL userinfo.
6. **Low-memory first.** Keep one decoder per visible camera, use substreams in
   the grid, keep the main stream for fullscreen, and preserve the live-tile
   limit and RAM watchdog. Avoid unbounded buffers and extra GL textures.
7. **Rust 1.85 / eframe 0.32.3 compatibility.** The project intentionally pins
   `eframe`/`egui` 0.32.3, the newest release that builds with Debian 13's
   Rust 1.85. It uses edition 2021, `rust-version = "1.85"` and
   `resolver = "3"`. `Cargo.lock` is committed. Newer egui releases require
   Rust 1.95 and a different `App` lifecycle; treat that as a separate,
   benchmark-gated upgrade. Other upgrades must be tested on the target
   machine and documented in `CHANGELOG.md`.
8. **All OpenGL work stays on the render thread.** Create/use GL resources only
   inside an `egui_glow` paint callback with the context current. Do not call GL
   from `update()` or worker threads.
9. **Clean shutdown matters.** Stop active recordings and save config in
   `on_exit`; keep the SIGTERM/SIGINT handler that closes the viewport. A hard
   kill may lose the current clip, but normal close must finalize MKV files.

## 3. Repository map

| Path | Responsibility |
|---|---|
| `src/main.rs` | CLI (`--selftest`, `--import-m3u`), signal handler, eframe boot |
| `src/app.rs` | UI, state, camera manager, settings, actions, sync/player lifecycle |
| `src/player.rs` | libmpv wrapper: load, reconnect/backoff, audio, snapshots, `stream-record` |
| `src/gltile.rs` | Aspect-correct FBO + GLSL shader with zoom/pan UVs |
| `src/storage.rs` | Recording scan, global storage accounting and oldest-clip pruning |
| `src/zoom.rs` | Fullscreen zoom/pan state and focal-point math |
| `src/tunnel.rs` | App-managed `socat` TLS listeners, port allocation, supervision |
| `src/config.rs` | TOML model, atomic `0600` saves, read-only M3U import, URL builder |
| `src/platform.rs` | Paths, logging, disk/RAM queries, autostart, single-instance, GL loader |
| `docs/ARCHITECTURE.md` | Technical design, data flow, trade-offs |
| `docs/DESIGN.md` | Product decisions and the original sanitized rationale |
| `docs/ROADMAP.md` | Planned features and explicit non-goals |
| `SECURITY.md` | Threat model and reporting policy |
| `THIRD_PARTY.md` | Dependency licenses and external processes |

## 4. Build, format and test

On the target tablet (Debian 13, rustc 1.85.1):

```bash
sudo apt install rustc cargo rustfmt rust-clippy pkg-config libmpv-dev libssl-dev \
  libx11-dev libxkbcommon-dev libxkbcommon-x11-dev libxcursor-dev \
  libxrandr-dev libxi-dev libgl1-mesa-dev libegl1-mesa-dev libwayland-dev

cargo fmt --all -- --check
cargo check --locked
cargo test --release
cargo build --release
```

- The release build takes roughly 28 minutes on the Atom. Keep
  `CARGO_BUILD_JOBS=2` (or `1`) to avoid OOM. Incremental app-only rebuilds are
  normally about a minute.
- `cargo test --release` may take several minutes to link on this machine; that
  is expected.
- `cargo clippy --all-targets` is recommended when the `rust-clippy` package is
  installed. Do not silence warnings globally; fix them or document why.
- CI (`.github/workflows/ci.yml`) runs fmt, check and tests on Ubuntu with the
  required system libraries.

Manual smoke tests (require real cameras, never commit the URLs):

```bash
cargo run --release -- --selftest
# Expect: tunnels created, 3 streams report width/height and hwdec=vaapi*, a
# short MKV is written and ffprobe can read video and audio.
```

Do not add real camera URLs to tests, fixtures, docs or CI. Use RFC 5737
addresses (`192.0.2.0/24`) or synthetic streams.

## 5. How to implement common changes

### Add or change a camera field

1. Add the field to `Camera` in `src/config.rs` with `#[serde(default)]` and a
   default function if needed.
2. Update the camera form in `src/app.rs` (`CameraForm`, fields, commit).
3. Update URL building only if the field affects it.
4. If it affects connections, verify the one-session invariant and update
   `docs/ARCHITECTURE.md`.
5. Add a unit test for defaults or URL construction.

### Change the UI

- Keep the dark, high-contrast style and the 1280×800 target.
- Use `UiAction` + `apply_action` for interactions; avoid mutating the player
  map from inside paint closures.
- Video must be drawn through `TileGl::draw` inside a paint callback. Never
  render mpv directly into the default framebuffer: mpv changes the GL viewport
  and all tiles would overlap in the bottom-left corner.
- Respect `max_live_tiles` and `fullscreen`: `sync_players()` is the only place
  that should create/destroy players.
- Fullscreen zoom belongs in `src/zoom.rs` and must only change the shader UVs
  and the fitted video viewport. Never create another decoder or RTSP session
  for zoom. Grid tiles stay at 1×.

### Change recording

- Recording is `mpv.set_property("stream-record", path)`; stopping sets an empty
  string. The file is often 0 bytes while buffering and only gets its final size
  when the property changes — do not use file size as a liveness signal.
- Rotating a segment is allowed: set `stream-record` to the next path. mpv
  closes the old file and opens the new one on the same RTSP session. Never
  reload the stream just to rotate a file.
- The global storage limit is deliberately soft by one segment: prune to
  `cap - 64 MiB`, never delete the active clip, files younger than 30 s,
  symlinks or files outside the CamView camera/date layout.
- A recording intent survives a reconnect: `src/app.rs` resumes it when the
  player is live again. Stopping the user recording clears that intent.
- Keep the free-space guard. Update `README.md` and
  `docs/ARCHITECTURE.md` if behavior changes.

### Change the tunnel

- `socat` arguments live in `src/tunnel.rs`. Always keep
  `TCP-LISTEN:...,reuseaddr,fork,bind=127.0.0.1`.
- The child must receive `PR_SET_PDEATHSIG` and be killed on app shutdown.
- Ports are allocated dynamically; never reuse the legacy 8554–8556 ports of
  another application.

### Add a dependency

- Prefer the standard library and existing crates. Justify any new dependency
  in the PR description, check its MSRV/edition (must compile with Rust 1.85),
  license, and memory impact.
- Do not add Tokio, a web framework, a database, an AI runtime or a GUI toolkit.
  The app deliberately has no async runtime.

## 6. Debugging checklist

- Log: `~/.local/state/camview/camview.log` (credentials redacted).
- Tunnel check: `ss -ltnp | grep 127.0.0.1` and count established camera
  connections with `ss -tnp`.
- Decoder check: the tile label shows `hwdec`; `vaapi` or `vaapi-copy` is good,
  empty means software decode.
- A camera in `Reconnecting` usually means bad credentials/path, the camera's
  two-session limit reached, or a stale `socat` listener.
- `Cannot load libcuda.so.1` is harmless: mpv probes CUDA and falls back to VA-API.
- If all tiles are black, check the FBO/GLSL path, not the RTSP URL: a viewport
  bug makes every tile render into the bottom-left corner.
- If a recording is 0 bytes, stop it normally; mpv buffers the container header.

## 7. Security review before a PR

- [ ] No real IPs, hostnames, camera names, credentials, tokens or personal data.
- [ ] No `config.toml`, `*.mkv`, `*.log`, snapshots or secrets staged.
- [ ] Tunnels bind `127.0.0.1`; no new listening socket.
- [ ] Logs/UI redact credentials; errors never print a full RTSP URL.
- [ ] Config remains `0600`; new files have reasonable permissions.
- [ ] One RTSP connection per camera still holds.
- [ ] Release memory/CPU did not regress (record numbers in the PR).
- [ ] `./scripts/check-secrets.sh` passes.
- [ ] `cargo fmt --check`, `cargo check --locked`, `cargo test --release` pass.
- [ ] Docs (`README`, `docs/ARCHITECTURE.md`, `CHANGELOG.md`) updated.

## 8. AI agent workflow

1. Read this file, `docs/ARCHITECTURE.md` and the relevant module.
2. Make a short plan; identify which invariant or connection limit the change
   touches.
3. Prefer small, reviewable commits. Never rewrite unrelated code.
4. Run fmt/check/tests. If real cameras are unavailable, state which manual
   tests could not be run.
5. Update docs and `CHANGELOG.md`.
6. Never push, force-push, rotate credentials or alter GitHub settings unless
   the human explicitly asks.
7. If something is ambiguous (camera model, TLS, storage policy, UI language,
   connection budget), ask before implementing.

## 9. Environment notes

- Target: Debian 13, X11/XFCE, Intel Atom Z3735F, 1.8 GB RAM, Mesa i965, VA-API
  H.264, 1280×800 display.
- Runtime dependencies: `libmpv2`, `socat`, `ffprobe`.
- The app stores data under the user's home; never hardcode a username. Use
  `HOME` and XDG-style paths.
- The UI and documentation are in English. Spanish localization is a v0.2
  roadmap item; do not add partial translations without a language setting.
