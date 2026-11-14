# Security policy

## Supported versions

| Version | Supported |
|---|---|
| 0.1.x | Yes |

## Reporting a vulnerability

Please do **not** open a public issue for a security problem. Use GitHub's
private vulnerability reporting for this repository, or contact the maintainer
through the GitHub profile linked in the repository metadata. Include:

- affected version/commit;
- steps to reproduce;
- impact (credential exposure, camera access, local code execution, etc.);
- any suggested mitigation.

You will get an acknowledgement as soon as possible. Please allow time for a fix
before public disclosure.

## Threat model

CamView is a **single-user desktop application**. It is not a hardened NVR,
not a multi-tenant service and not designed to be exposed to the internet.

### Trusted

- The local user account and the desktop session.
- The local network, to a reasonable degree.
- The camera firmware and its credentials.
- The system OpenGL, libmpv, ffmpeg and socat packages.

### Untrusted / considered

- Other devices on the LAN.
- RTSP responses from the camera (untrusted input parsed by libmpv/ffmpeg).
- A malicious or compromised camera.
- Anyone with read access to the user's home directory or backups.

### Explicit non-goals

- Protecting against a compromised local account or root.
- Encrypting recordings at rest.
- Hiding camera credentials from the local user.
- Providing remote access, authentication or authorization.
- Auditing camera firmware.

## Security properties and trade-offs

### Local-only tunnels

RTSPS cameras are reached through a `socat` listener bound to
`127.0.0.1`. No camera port is exposed on the LAN by CamView.

### TLS certificate verification is disabled

Many consumer cameras use self-signed certificates that the local
ffmpeg/gnutls stack cannot validate (or even handshake with). The tunnel uses:

```
OPENSSL:<camera>:<port>,verify=0
```

This means the tunnel does **not** authenticate the camera certificate. An
attacker who can intercept local network traffic and present a different TLS
endpoint could impersonate the camera. This is a deliberate compatibility
trade-off for a trusted LAN.

If your camera supports a trusted certificate or you can pin its fingerprint,
prefer a validating configuration. A future version may add certificate pinning.

### Credentials

- Camera credentials are stored in `~/.config/camview/config.toml` with mode
  `0600`.
- The config is not encrypted; anyone who can read the file can read the
  credentials.
- Logs and UI redact the userinfo portion of RTSP URLs.
- Recording, snapshot and log directories are created with user-only
  permissions where possible.
- Do not commit `config.toml`, recordings, snapshots or logs to Git.

### Recording

- Files are unencrypted Matroska.
- The current clip may be incomplete after a hard kill or power loss.
- Disk space is monitored; recordings stop below the configured threshold.

### Network exposure

CamView does **not** open an HTTP server, a web UI, a remote-control port or a
cloud connection. The only listening sockets are the localhost `socat`
listeners. The app does not collect telemetry.

### Dependencies

CamView links dynamically to libmpv (LGPL-2.1+) and executes `socat` and
`ffprobe` at runtime. Keep these packages updated through the distribution.
Cargo dependencies are pinned by `Cargo.lock`; review changes before updating.

## Hardening recommendations

1. Keep the system, libmpv, ffmpeg and socat updated.
2. Use unique, strong camera passwords; do not reuse the desktop password.
3. Restrict the camera to a dedicated VLAN or firewall rule if possible.
4. Keep `~/.config/camview/config.toml` at `0600` and exclude it from backups
   you do not control.
5. Do not expose the machine's SSH or remote desktop to the internet without a
   VPN/Tailscale and strong authentication.
6. If you enable autostart, ensure the desktop session is locked when unattended.
7. Review recordings and snapshots before sharing them; they contain sensitive
   images and metadata.

## Out of scope

- Vulnerabilities in camera firmware or vendor cloud services.
- Vulnerabilities in the Linux kernel, Mesa, libmpv, ffmpeg, socat or OpenSSL
  (report them upstream).
- Physical access attacks.
- Denial of service by a local user who can already kill the process.
