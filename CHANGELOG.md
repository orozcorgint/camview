# Changelog

All notable changes to this project are documented here.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/)
and this project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- English documentation, AI-agent instructions, security policy and CI.
- Unit tests for URL building, credential redaction and TOML round-trips.

### Changed

- Updated `url` to 2.5.8.
- Dependabot now checks monthly with a two-PR limit.

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
- Spanish UI localization is planned for 0.2.0.
