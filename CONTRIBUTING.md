# Contributing to CamView

Thanks for taking the time to improve CamView. This project is intentionally
small and conservative: a native, low-memory Linux viewer/recorder. Please read
[AGENTS.md](AGENTS.md) for the invariants that must not be broken.

## Development setup

Tested on Debian 13 with Rust 1.85.1. Install:

```bash
sudo apt install rustc cargo rustfmt rust-clippy pkg-config libmpv-dev libssl-dev \
  libx11-dev libxkbcommon-dev libxkbcommon-x11-dev libxcursor-dev \
  libxrandr-dev libxi-dev libgl1-mesa-dev libegl1-mesa-dev libwayland-dev
```

Build and test:

```bash
cargo fmt --all -- --check
cargo check --locked
cargo test --release
cargo build --release
```

The release build is slow on low-power machines. Use `CARGO_BUILD_JOBS=2` (or
`1`) if memory is tight. Do not raise the job count on a 1–2 GB machine.

## What to work on

Good first issues:

- UI polish and accessibility (keyboard navigation, labels, larger controls).
- Spanish localization and a language setting.
- Documentation, screenshots (without real camera feeds), and examples.
- Better error messages and diagnostics.
- Unit tests for configuration, URL building, retention/storage logic.

Please open an issue before starting a large feature. The roadmap in
[docs/ROADMAP.md](docs/ROADMAP.md) lists priorities and non-goals.

## Coding conventions

- Keep the one-RTSP-session-per-camera invariant.
- Keep tunnels bound to `127.0.0.1`.
- Do not add a web server, async runtime, database or webview.
- Prefer the standard library; justify new dependencies in the PR.
- Keep UI strings and user-facing logs in English.
- Keep code comments clear and in English.
- Run `cargo fmt` before committing.
- Avoid `unwrap()` in new runtime paths; return a `Result` or show a UI error.
- All OpenGL calls belong in paint callbacks with the context current.
- Do not commit `config.toml`, recordings, snapshots, logs or credentials.

## Commit messages

Use imperative, scoped messages:

```
feat(player): add audio mute toggle
fix(tunnel): restart socat when the child exits
docs(agents): document the FBO rendering invariant
test(config): cover credential redaction
```

Keep commits focused. Do not force-push a shared branch.

## Pull request checklist

- [ ] `./scripts/check-secrets.sh` passes.
- [ ] `cargo fmt --all -- --check` passes.
- [ ] `cargo check --locked` passes.
- [ ] `cargo test --release` passes.
- [ ] No secrets, camera data, recordings or personal paths are staged.
- [ ] Tunnels still bind to localhost only.
- [ ] One RTSP connection per camera is preserved.
- [ ] Memory/CPU impact is considered and, if relevant, measured.
- [ ] `README.md`, `docs/ARCHITECTURE.md`, `docs/ROADMAP.md` and
      `CHANGELOG.md` are updated when behavior or usage changes.
- [ ] New user-facing text is in English.

## Testing without real cameras

CI does not use cameras. For local integration testing you can run a synthetic
RTSP source. For example, with MediaMTX (formerly rtsp-simple-server):

```bash
# Terminal 1: local RTSP server
mediamtx

# Terminal 2: synthetic H.264 test stream
ffmpeg -re -f lavfi -i testsrc2=size=640x360:rate=20 \
  -c:v libx264 -preset veryfast -tune zerolatency -f rtsp \
  rtsp://127.0.0.1:8554/demo
```

Then add a camera in CamView with host `127.0.0.1`, port `8554`, path `/demo`,
TLS off. This is enough to exercise the render path, snapshots and recording.

If you have a real camera, you can use `cargo run --release -- --selftest`;
never commit its URL, IP address, hostname or credentials.

## Security

See [SECURITY.md](SECURITY.md). Do not report vulnerabilities in public issues.
