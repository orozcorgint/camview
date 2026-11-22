# Packaging

CamView ships a Debian binary package for quick installation on Debian and
Ubuntu-based desktops. The package is built from the release binary and
contains the application, desktop entry, icon, AppStream metadata, man page and
documentation.

## Build the package

Requirements: Debian/Ubuntu, Rust 1.85 or newer, `cargo`, `dpkg-deb`,
`dpkg-shlibdeps` (from `dpkg-dev`), and the native libraries listed in
[../README.md](../README.md).

```bash
make deb
# or
./packaging/build-deb.sh
```

The artifact is written to `dist/`:

```text
dist/camview_<version>-1_<architecture>.deb
```

Useful environment variables:

| Variable | Default | Purpose |
|---|---|---|
| `CARGO_BUILD_JOBS` | `2` | Limit compiler parallelism on low-memory machines |
| `DEB_REVISION` | `1` | Debian package revision |
| `DEB_ARCH` | `dpkg --print-architecture` | Target architecture |
| `OUT_DIR` | `dist/` | Output directory |

## Install

```bash
sudo apt install ./dist/camview_<version>-1_<architecture>.deb
```

`apt` resolves the runtime dependencies. The package installs:

```text
/usr/bin/camview
/usr/share/applications/camview.desktop
/usr/share/icons/hicolor/scalable/apps/camview.svg
/usr/share/metainfo/io.github.orozcorgint.camview.metainfo.xml
/usr/share/man/man1/camview.1.gz
/usr/share/doc/camview/
```

CamView writes its configuration and recordings to the current user's home
directory, not to the package tree:

```text
~/.config/camview/config.toml
~/Videos/CamView/
~/Pictures/CamView/
~/.local/state/camview/camview.log
```

## Verify the package

```bash
dpkg-deb --info dist/camview_*.deb
dpkg-deb --contents dist/camview_*.deb
rm -rf /tmp/camview-deb
dpkg-deb -x dist/camview_*.deb /tmp/camview-deb
desktop-file-validate /tmp/camview-deb/usr/share/applications/camview.desktop
```

The build script runs the available `desktop-file-validate`, `xmllint`,
`appstreamcli` and `groff` checks automatically. `lintian` can be used for a
full Debian policy check:

```bash
lintian --pedantic dist/camview_*.deb
```

A standalone package built outside the Debian archive may report
`initial-upload-closes-no-bugs`; this is expected for a project-local package
and is not a runtime issue.

## Remove

```bash
sudo apt remove camview
```

The user configuration and recordings are intentionally left untouched.

## Release automation

`.github/workflows/package.yml` builds the `.deb` on every pull request that
touches the packaging inputs and uploads it as an artifact. When a GitHub
release is published, the same workflow attaches the `.deb` to the release.
