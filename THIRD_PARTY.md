# Third-party components

CamView is MIT-licensed. It depends on the following external components; their
licenses apply to those components.

## Linked at build/runtime

| Component | License | Notes |
|---|---|---|
| [libmpv](https://github.com/mpv-player/mpv) | LGPL-2.1-or-later | Dynamically linked via `libmpv-2`; provides playback, demuxing and the render API |
| [egui / eframe](https://github.com/emilk/egui) | MIT OR Apache-2.0 | Immediate-mode UI and OpenGL integration |
| [glow](https://github.com/grovesNL/glow) | MIT OR Apache-2.0 | OpenGL bindings |
| [serde](https://serde.rs/) | MIT OR Apache-2.0 | Configuration serialization |
| [toml](https://github.com/toml-rs/toml) | MIT OR Apache-2.0 | Configuration format |
| [url](https://github.com/servo/url) | MIT OR Apache-2.0 | URL parsing for the M3U importer |
| [percent-encoding](https://github.com/servo/percent-encoding) | MIT OR Apache-2.0 | RTSP userinfo encoding |
| [chrono](https://github.com/chronotope/chrono) | MIT OR Apache-2.0 | Timestamps |
| [libc](https://github.com/rust-lang/libc) | MIT OR Apache-2.0 | `prctl`, `flock`, `statvfs`, locale |
| [raw-window-handle](https://github.com/rust-windowing/raw-window-handle) | MIT OR Apache-2.0 | X11 display handle for VA-API interop |

## Executed as external processes

| Component | License | Notes |
|---|---|---|
| [socat](http://www.dest-unreach.org/socat/) | GPL-2.0 | Local TLS termination for RTSPS cameras |
| [FFmpeg / ffprobe](https://ffmpeg.org/) | LGPL-2.1+ or GPL (build-dependent) | Optional "Test camera" probe |
| OpenSSL (via socat) | Apache-2.0 | TLS implementation |
| Mesa / VA-API drivers | MIT and other | Hardware decoding |

The repository does not vendor or redistribute these components. Package
versions and licenses depend on the Linux distribution. Review the licenses of
the packages you install if you redistribute a bundled build.
