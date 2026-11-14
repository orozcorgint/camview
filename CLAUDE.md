# Claude instructions

The canonical agent guide for this repository is [AGENTS.md](AGENTS.md).
Read it before making changes.

Critical invariants (short version):

1. One RTSP session per camera. Viewer and recorder share the same libmpv
   session through `stream-record`.
2. TLS tunnels bind to `127.0.0.1` only.
3. Never modify external playlists, another player's settings or services.
4. Recording is a stream copy; do not add transcoding.
5. Never commit camera credentials, configs, recordings, snapshots or logs.
6. Keep the OpenGL work inside paint callbacks; use the FBO path.
7. Build with Rust 1.85 / eframe 0.32.3; do not bump dependencies casually.
8. Run `cargo fmt --all -- --check`, `cargo check --locked`,
   `cargo test --release` before proposing a change.
