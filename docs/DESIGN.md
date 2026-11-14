# Design notes

This document records the product and engineering decisions behind CamView
v0.1. It is intentionally sanitized: no real camera addresses, credentials,
recordings or network details belong here.

## Problem

A low-power Linux tablet (Intel Atom, 1.8 GB RAM, 1280×800) needs to display and
record three consumer IP cameras. The cameras expose both a main stream
(1080p/1440p) and a low-resolution substream (typically 640×360), but the
firmware accepts only **two simultaneous RTSP sessions per camera**. A legacy
desktop player must keep working independently during the transition.

A generic media player can display one channel at a time but is not a security
viewer: it lacks multi-camera status, snapshots, recording controls, storage
guards and connection-budget awareness.

## Product decisions

| Topic | Decision | Rationale |
|---|---|---|
| Scope | Viewer + manual recorder | Keep v0.1 small, testable and light |
| Layout | Grid + fullscreen | Standard CCTV workflow; substreams keep RAM low |
| Cameras | RTSP and RTSPS | Cover consumer cameras with and without TLS |
| PTZ / ONVIF | Out of scope for v0.1 | Target cameras are fixed; avoids protocol surface |
| Motion detection | Out of scope for v0.1 | Extra decode or frame analysis is expensive; manual recording first |
| Notifications | Out of scope for v0.1 | Desktop-only app; no cloud/phone dependency |
| Recording | Stream copy to MKV | No transcode CPU, no quality loss, one connection |
| Audio | Optional monitoring, included in recording | Evidence value; muted by default |
| Storage | Selectable folder + free-space guard | Avoid filling the system disk; support USB/NAS mounts |
| Retention | Manual deletion | Keep behavior predictable; retention is a roadmap item |
| Remote access | None | No inbound web server; security and memory |
| UI language | English | Open-source/portfolio baseline; Spanish localization is a v0.2 item |

## Connection budget

The camera limit is the most important constraint. If CamView used a separate
viewer and recorder connection, it would consume both slots and no other client
could view the camera. Instead:

- one mpv core per camera;
- viewing and recording share that core;
- `stream-record` copies the demuxed stream to a file;
- quality changes stop recording first (or are blocked while recording).

This is documented as an invariant in [AGENTS.md](../AGENTS.md) and enforced in
`Player::load()` and the recording logic.

## Memory strategy

- Grid uses the substream, not the main stream.
- Fullscreen drops non-recording players and uses the main stream for one camera.
- `max_live_tiles` limits how many players exist at once.
- No extra decoder for motion detection, thumbnails or recording.
- `egui` immediate-mode UI plus a single OpenGL FBO per visible tile.
- Low-RAM guard switches to the Eco profile when available memory is low.

## TLS strategy

RTSPS with self-signed certificates is common in consumer cameras. The
ffmpeg/gnutls stack used by mpv may not complete the handshake, while OpenSSL
does. A local `socat` process terminates TLS and exposes plain RTSP on
`127.0.0.1`.

Trade-off: `verify=0`. This is acceptable only on a trusted LAN. The tunnel
never binds to a public interface. See [SECURITY.md](../SECURITY.md).

## Recording strategy

`stream-record` is preferred over a sidecar ffmpeg recorder because:

- it reuses the existing RTSP connection;
- it adds no process or decoder;
- it preserves the exact stream (video + audio);
- MKV is resilient and does not need a final moov atom like MP4.

The downside is less control over segmentation and buffering. The file can be
0 bytes while mpv buffers the container header. v0.1 therefore avoids file-size
watchdogs and relies on graceful shutdown to finalize the clip.

## Alternatives considered

| Alternative | Why not |
|---|---|
| GStreamer + GTK4 | More memory and build complexity; harder RTSP/TLS handling |
| Tauri/web UI | WebKit memory footprint, unnecessary attack surface |
| Electron | Too heavy for 1.8 GB |
| ffmpeg child per recording | Second camera connection, ~55 MB per recorder |
| Direct mpv rendering into the window | mpv changes the viewport; tiles overlap |
| Transcoding to a lower bitrate | CPU-bound on the Atom; stream copy is enough |
| Cloud/mobile dashboard | Out of scope and a privacy/security liability |

## Future work

See [ROADMAP.md](ROADMAP.md). Any new feature must preserve the connection and
memory invariants, or explicitly document why they are being changed.
