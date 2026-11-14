## Summary

<!-- What changed and why? Keep it focused. -->

## Type of change

- [ ] Bug fix
- [ ] New feature
- [ ] Refactor / internal cleanup
- [ ] Documentation
- [ ] CI / build

## Invariants

- [ ] One RTSP session per camera is preserved.
- [ ] Tunnels still bind to `127.0.0.1` only.
- [ ] No new web server, async runtime, database or webview.
- [ ] Recording remains a stream copy (no transcoding).
- [ ] Memory/CPU impact considered; numbers below if relevant.

## Testing

<!-- Commands run and manual tests performed. Do not include real camera data. -->

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo check --locked`
- [ ] `cargo test --release`
- [ ] Manual smoke test (describe)

## Security / privacy

- [ ] No credentials, RTSP URLs, IPs, recordings, snapshots or logs committed.
- [ ] Logs/UI redact credentials.
- [ ] Config remains `0600`.

## Docs

- [ ] README / ARCHITECTURE / ROADMAP / CHANGELOG updated if needed.
