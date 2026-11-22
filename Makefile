PREFIX ?= $(HOME)/.local
BINDIR := $(PREFIX)/bin
APPDIR := $(PREFIX)/share/applications
ICONDIR := $(PREFIX)/share/icons/hicolor/scalable/apps

.PHONY: all build release run install uninstall deb fmt check secrets test selftest clean

all: build

build:
	CARGO_BUILD_JOBS=2 cargo build

release:
	CARGO_BUILD_JOBS=2 cargo build --release

run: release
	./target/release/camview

fmt:
	cargo fmt --all

check:
	cargo fmt --all -- --check
	cargo check --locked

secrets:
	./scripts/check-secrets.sh

test:
	cargo test --release

selftest: release
	./target/release/camview --selftest

install: release
	install -d "$(BINDIR)" "$(APPDIR)" "$(ICONDIR)"
	install -m 755 target/release/camview "$(BINDIR)/camview"
	sed 's|@BINDIR@|$(BINDIR)|g' data/camview.desktop.in > "$(APPDIR)/camview.desktop"
	chmod 644 "$(APPDIR)/camview.desktop"
	install -m 644 data/camview.svg "$(ICONDIR)/camview.svg"
	-update-desktop-database "$(APPDIR)" 2>/dev/null || true
	@echo "Installed CamView in $(PREFIX)"

uninstall:
	rm -f "$(BINDIR)/camview" "$(APPDIR)/camview.desktop" "$(ICONDIR)/camview.svg"

deb:
	./packaging/build-deb.sh

clean:
	cargo clean
