# Architecture

CamView is a single-process desktop application. It deliberately keeps the
runtime small: no web server, no database, no async runtime and no transcoding.

## 1. Goals and constraints

**Goals**

- Display several RTSP/RTSPS cameras in a grid on a low-power Linux desktop.
- Record the stream you are watching without re-encoding.
- Stay within the memory and connection budget of a 1.8 GB Intel Atom tablet.
- Coexist with another RTSP client (for example, a legacy IPTV player) that may
  use the same camera.

**Non-goals for v0.1**

- PTZ/ONVIF, motion detection, schedules, notifications, retention policies.
- Remote/mobile access, cloud sync, multi-user accounts.
- AI object detection or CPU transcoding.

## 2. Process and thread model

```
main thread (winit/eframe)
├── egui update()
├── GL paint callbacks (current OpenGL context)
│   ├── TileGl::draw()          FBO + shader per visible tile
│   └── libmpv render context   one per visible camera
├── player commands             mpv core calls (thread-safe)
└── tunnel supervision          every 2 s

libmpv internal threads
├── demux / RTSP
├── VA-API decode
└── stream-record writer
```

There is no Tokio or other async runtime. Blocking work that could stall the UI
(camera probing with `ffprobe`, tunnel restarts) is either short or delegated to
a worker thread.

## 3. UI and OpenGL compositing

The UI is built with `eframe`/`egui` 0.32 using the `glow` OpenGL backend. One
`libmpv` core exists per visible camera; each core has its own
`mpv_render_context`.

Rendering a tile:

1. `egui_glow` invokes a `PaintCallback` with the tile rectangle and the current
   GL context.
2. `TileGl::draw()` binds a tile-sized FBO and calls
   `mpv_render_context_render(fbo, w, h, flip_y = true)`.
3. The FBO is unbound and a small GLSL program draws a fullscreen triangle into
   the tile viewport (the viewport that egui set before the callback).
4. egui restores its own painting state and draws the overlays.

**Why an FBO instead of rendering mpv directly into the window?**
`mpv_render_context_render()` changes the OpenGL viewport. If called directly
for each tile, every stream is drawn into the same bottom-left corner. The FBO
isolates the viewport change and gives each tile an independent surface. It also
makes the compositing step trivial and keeps tile resize logic local.

OpenGL function pointers for mpv are loaded through the system `libEGL` /
`libGL` (`eglGetProcAddress`, `glXGetProcAddressARB`) rather than borrowing
eframe's loader closure. This lets the render context outlive the short
`CreationContext` borrow.

## 4. Player lifecycle

`Player` owns:

- a boxed `Mpv` core (initialized with `vo=libmpv`, `hwdec=auto-safe`, tiny
  demuxer cache, no OSD);
- a render context (OpenGL);
- state (`Idle`, `Connecting`, `Live`, `Reconnecting`, `Error`, `Stopped`);
- the current URL, stream kind and recording metadata.

State machine:

```
load(url) ──► Connecting ──► Live
                    │           │
                    │           └── no video / core idle > 3 s
                    └── 10 s timeout
                                │
                                ▼
                         Reconnecting (2, 4, 8, 15, 30 s backoff)
                                │
                                └── retry load(url)
```

`tick()` polls `video-params/w`, `video-params/h`, `core-idle` and
`hwdec-current` at roughly 3 Hz. It never blocks the UI.

Camera reloads always go through `Player::load()`, which stops an active
recording first. This prevents writing a new stream into an open container and
keeps the one-session invariant.

## 5. Stream selection and connection budget

Each camera exposes a main stream (1080p/1440p) and a substream (typically
640×360). The grid uses the substream; fullscreen switches to the main stream.

Many consumer cameras accept **two RTSP sessions total**. CamView therefore
uses exactly one session per camera:

- viewing uses that session;
- recording is `stream-record` on the same mpv core;
- a second client (another app) can use the remaining session.

When switching substream ↔ main stream, the player is reloaded. The old session
is closed before the new one is opened. If a recording is active, the quality
switch is blocked until the user stops recording.

`sync_players()` is the only place that creates or destroys players. It keeps
players that are recording even if they are not on the current page, so a page
change or fullscreen transition cannot silently stop a recording.

## 6. TLS tunnel

Some cameras only expose RTSPS with a self-signed certificate, and the
ffmpeg/gnutls stack used by libmpv may fail the handshake. The app therefore
starts one `socat` per RTSPS camera:

```
socat TCP-LISTEN:<ephemeral>,reuseaddr,fork,bind=127.0.0.1 \
      OPENSSL:<camera-host>:<camera-port>,verify=0
```

Properties:

- listener bound to `127.0.0.1` only;
- one child per camera, `PR_SET_PDEATHSIG` set so it dies with the app;
- supervised and restarted with the same port when possible;
- ports are dynamic and never collide with another application's proxies;
- certificate verification is disabled for compatibility; see
  [SECURITY.md](../SECURITY.md).

The tunnel only relays bytes. RTSP Digest authentication remains end-to-end
between mpv and the camera.

## 7. Recording

Recording is a container-level copy:

```rust
mpv.set_property("stream-record", output_path);
// ...
mpv.set_property("stream-record", "");
```

- Output: Matroska (`.mkv`), one file per recording session.
- Path: `<storage>/<camera>/<YYYY-MM-DD>/<HH-MM-SS>.mkv`.
- No second connection, no ffmpeg child, no re-encoding.
- Audio is included if the camera stream has it.
- The file may remain 0 bytes for a while because libavformat buffers the
  container header. It is finalized when the property is cleared or the player
  is destroyed; do not use file size as a liveness signal.
- If the stream reloads (camera loss, quality change), the recording is stopped
  first. The user can start a new clip.
- A storage watchdog stops all recordings if free space drops below the
  configured threshold (default 500 MB).

Normal application exit and `SIGTERM` close the viewport, which triggers
`on_exit` and finalizes the active recording. A `SIGKILL` or power loss can lose
the clip that was being written; this is an accepted trade-off for a desktop
viewer.

## 8. Configuration and data

```
~/.config/camview/config.toml        camera list, storage, quality (0600)
~/.local/state/camview/camview.log   rotating log, credentials redacted
~/Videos/CamView/...                 recordings
~/Pictures/CamView/                  snapshots
```

Configuration is written atomically (`.tmp` + rename) and chmod `0600`. A
backup (`config.toml.bak`) is kept. Credentials are stored in this file because
the app must build RTSP URLs; they are never written to logs or shown in the UI.

The optional M3U import reads `~/cameras.m3u` (or the legacy `~/camaras.m3u`)
and systemd user units in a strictly read-only fashion, then resolves local
proxy ports to upstream camera hosts. It never modifies the playlist, another
player's settings or services.

## 9. Memory budget

| Component | Approximate |
|---|---|
| egui/eframe + fonts + UI state | 40–60 MB |
| 3 substream decoders (VA-API, 360p) | ~210–240 MB |
| FBOs and GL resources | < 20 MB |
| 3 `socat` listeners | ~5 MB |
| Recording (stream-record) | in-process, no extra decoder |
| Fullscreen main stream (1 camera) | replaces substream load, ~+30–40 MB |

Measured with 3 cameras on the target tablet: ~215 MB RSS and ~0.44 of one CPU
core in release mode. The app shows free RAM/disk in the status bar and switches
to the Eco profile if available memory drops below 100 MB.

## 10. Failure modes

| Symptom | Likely cause | Handling |
|---|---|---|
| All tiles black, UI visible | FBO/viewport bug | Render inside FBO; never call mpv into the default framebuffer |
| `Reconnecting` loop | wrong URL/password, camera blocked, 2-session limit | exponential backoff; check log and `ss` |
| Third connection fails | camera limit reached | CamView uses one session; stop the other client |
| Recording 0 bytes | mpv buffering | stop normally; do not delete |
| `Cannot load libcuda.so.1` | mpv probing CUDA | harmless; VA-API is used |
| Tunnel not starting | `socat` missing or port conflict | dynamic port allocation, restart with backoff |
| High RSS | too many main streams or extra apps | Eco profile, fullscreen, close other browsers |
