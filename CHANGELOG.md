# Changelog

All notable changes to this project are documented here.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/)
and this project adheres to [Semantic Versioning](https://semver.org/).

## [0.2.0] - 2026-11-16

### Added

- Continuous recording with a global, configurable storage limit (1 GB by
  default) and automatic removal of the oldest finished clips.
- Recording intent that resumes after a camera reconnect.
- Fullscreen digital zoom (pinch, mouse/touchpad, drag-to-pan and keyboard
  shortcuts) with an aspect-correct FBO and shader UV cropping.
- Unit tests for storage pruning, legacy config defaults and zoom math.
- English documentation, AI-agent instructions, security policy and CI.
- Unit tests for URL building, credential redaction and TOML round-trips.

### Changed

- Updated `url` to 2.5.8.
- Updated `toml` to 1.2 and refreshed the lockfile.
- Dependabot now checks monthly with a two-PR limit.

### Known limitations

- The storage cap has one segment of headroom; the clip currently being
  written can finish above the limit.
- Digital zoom is limited by the decoded stream resolution.
- Recording from the grid captures the substream; use fullscreen before
  starting a recording to capture the main stream.

## [0.1.0] - 2026-11-13

### Added

- Native Rust/egui viewer for RTSP and RTSPS cameras.
- Multi-camera grid (1/4/9 tiles, pages) and fullscreen main-stream view.
- One `libmpv` session per camera with VA-API hardware decoding.
- App-managed local `socat` TLS tunnels bound to `127.0.0.1`.
- Camera add/edit/test/remove UI with automatic substream detection.
- Stream-copy recording to Matroska via mpv `stream-record`.
- Record/stop per camera and record-all controls.
- Snapshots and per-camera audio monitoring.
- Eco/Balanced profiles, render FPS cap, live-tile limit and RAM watchdog.
- Selectable recording folder with a free-space guard.
- Autostart, single-instance lock, rotating logs and `0600` config.
- Clean shutdown/SIGTERM recording finalization.
- Read-only import from a legacy M3U playlist.

### Known limitations

- No PTZ/ONVIF, motion detection, schedules, notifications or retention.
- The application must be running to record.
- Spanish UI localization is planned for a future release.
