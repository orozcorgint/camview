# GitHub Copilot instructions

Follow [AGENTS.md](../AGENTS.md) as the canonical repository guide. The short
version:

- CamView is a native Rust/egui desktop app for low-memory Linux machines.
- Keep exactly one RTSP session per camera; recording uses the same libmpv
  session via `stream-record`.
- Keep `socat` tunnels bound to `127.0.0.1`; never expose camera ports.
- Do not modify external playlists, another player's settings or services.
- Never add transcoding, a web server, an async runtime, a database or a webview.
- Never commit credentials, `config.toml`, recordings, snapshots or logs.
- Rust 1.85 / eframe 0.32.3 are pinned; keep `Cargo.lock` updated deliberately.
- All OpenGL work belongs in `egui_glow` paint callbacks; use the FBO renderer.
- Run `cargo fmt --all -- --check`, `cargo check --locked` and
  `cargo test --release` before finishing.
