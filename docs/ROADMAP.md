# Roadmap

CamView v0.1 is a focused viewer/recorder. The following items are intentionally
deferred. Priorities may change based on user feedback, but the connection and
memory invariants in [AGENTS.md](../AGENTS.md) remain the default.

## v0.2 — quality of life

- [ ] Spanish UI localization with a language setting (English is current).
- [ ] Keyboard navigation and accessibility pass (focus, shortcuts, labels).
- [ ] Better camera form validation (duplicate IDs, port/path checks).
- [ ] Per-camera "preferred stream" setting for the grid.
- [ ] Config migration/version field and import/export of camera lists without
      credentials.
- [ ] Optional tray icon / minimize-to-tray.
- [ ] Native Wayland support (current build targets X11/XWayland).
- [ ] More unit tests for storage guards and reconnect backoff.

## v0.3 — storage and playback

- [ ] Retention policy (days and/or maximum GB), with dry-run preview.
- [ ] Recording timeline/playback UI with camera/date filters.
- [ ] Export a clip without re-encoding.
- [ ] Detect and repair interrupted MKV clips.
- [ ] Optional disk usage chart.

## v0.4 — detection and alerts (only if it fits the budget)

- [ ] Low-rate motion detection from the existing rendered frames (no extra
      decoder), with zones and sensitivity.
- [ ] Post-roll recording (no pre-roll in the first iteration).
- [ ] Desktop notifications and optional webhook/ntfy.
- [ ] Pre-roll ring buffer, only if a memory-safe design is possible.

## v0.5 — protocols

- [ ] ONVIF discovery and PTZ for cameras that support it.
- [ ] Optional certificate pinning for RTSPS tunnels.
- [ ] HTTPS/MJPEG snapshot endpoint over Tailscale (token-protected, no cloud).

## Explicit non-goals

- Cloud recording, vendor accounts, telemetry.
- Multi-user accounts, roles or a public web dashboard.
- CPU transcoding or AI object detection on the target Atom hardware.
- Mobile apps in this repository.
- Recording to a remote SMB/NFS share without a local mount.

## How to propose a feature

Open an issue describing:

1. the user problem;
2. the expected memory and CPU impact;
3. whether it needs an extra RTSP connection or decoder;
4. how it behaves on a 1.8 GB machine;
5. the security/privacy implications.

Features that require a second camera session or a second decode per camera
need an explicit design review before implementation.
