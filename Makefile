# Build and install tasks for Sagiri. GNU Make.
# The quality gate is scripts/check.sh; `make check` only delegates to it.

CARGO       ?= cargo
CARGO_FLAGS ?= --locked
PREFIX      ?= /usr/local
DESTDIR     ?=
USERUNITDIR ?= $(PREFIX)/lib/systemd/user
BINS        := sagiri sagiri-cli sagiri-session

ifneq ($(CARGO_TARGET_DIR),)
RELEASE_DIR := $(CARGO_TARGET_DIR)/release
else
RELEASE_DIR := target/release
endif

# A leading `~` in PREFIX is expanded here, because shells such as zsh pass
# `PREFIX=~/.local` through unexpanded and a non-empty DESTDIR would hide it from the
# recipe shell.
PREFIX_DIR := $(patsubst ~/%,$(HOME)/%,$(patsubst ~,$(HOME),$(PREFIX)))
BINDIR     := $(DESTDIR)$(PREFIX_DIR)/bin

# The systemd user unit is installed on Linux only. Its ExecStart= names the binary at run
# time, so @BINDIR@ is replaced by the path without DESTDIR. The replacement is escaped for
# the sed expression of `install` (backslash, `&` and the delimiter `|`).
UNAME_S         := $(shell uname -s)
USERUNITDIR_DIR := $(patsubst ~/%,$(HOME)/%,$(patsubst ~,$(HOME),$(USERUNITDIR)))
UNIT_SRC        := packaging/systemd/sagiri-session.service.in
UNIT_FILE       := $(DESTDIR)$(USERUNITDIR_DIR)/sagiri-session.service
UNIT_BINDIR     := $(subst |,\|,$(subst &,\&,$(subst \,\\,$(PREFIX_DIR)/bin)))

.DEFAULT_GOAL := all

.PHONY: all build release $(BINS) install uninstall pkg check audit clean help

all: release

build:
	$(CARGO) build $(CARGO_FLAGS) --workspace

release:
	$(CARGO) build $(CARGO_FLAGS) --release -p sagiri $(addprefix --bin ,$(BINS))

$(BINS):
	$(CARGO) build $(CARGO_FLAGS) --release -p sagiri --bin $@

# Never builds: `sudo make install` must not leave root-owned files in the target directory.
# The directory is created with `install -d`: `install -D` creates it with GNU coreutils
# only, the `install` of macOS takes `-D` as an option with an argument and creates nothing.
install:
	@for bin in $(BINS); do \
		if [ ! -x '$(RELEASE_DIR)'/"$$bin" ]; then \
			echo "missing $(RELEASE_DIR)/$$bin: run 'make release' first" >&2; \
			exit 1; \
		fi; \
	done
	install -d '$(BINDIR)'
	$(foreach b,$(BINS),install -m 755 '$(RELEASE_DIR)/$(b)' '$(BINDIR)/$(b)' &&) true
ifeq ($(UNAME_S),Linux)
	install -d '$(DESTDIR)$(USERUNITDIR_DIR)'
	sed 's|@BINDIR@|$(UNIT_BINDIR)|g' '$(UNIT_SRC)' > '$(UNIT_FILE).tmp' || \
		{ rm -f '$(UNIT_FILE).tmp'; exit 1; }
	chmod 644 '$(UNIT_FILE).tmp'
	mv -f '$(UNIT_FILE).tmp' '$(UNIT_FILE)'
endif

uninstall:
	rm -f $(foreach b,$(BINS),'$(BINDIR)/$(b)')
ifeq ($(UNAME_S),Linux)
	rm -f '$(UNIT_FILE)'
endif

# PKGEXT and PKGDEST are fixed so that the package name and location do not depend on the
# local makepkg.conf.
pkg:
	cd packaging/arch && PKGDEST="$$PWD" PKGEXT=.pkg.tar.zst makepkg -f

check:
	scripts/check.sh

audit:
	scripts/audit.sh

clean:
	$(CARGO) clean

help:
	@echo 'Targets:'
	@echo '  all             default, same as release'
	@echo '  build           debug build of the workspace'
	@echo '  release         release build of sagiri, sagiri-cli and sagiri-session'
	@echo '  sagiri          release build of that binary only'
	@echo '  sagiri-cli      release build of that binary only'
	@echo '  sagiri-session  release build of that binary only'
	@echo '  install         copy the release binaries to $$(DESTDIR)$$(PREFIX)/bin (does not build);'
	@echo '                  on Linux also the systemd user unit to $$(DESTDIR)$$(USERUNITDIR)'
	@echo '  uninstall       remove them and the unit from there'
	@echo '  pkg             build an Arch Linux package in packaging/arch (makepkg -f)'
	@echo '  check           run scripts/check.sh'
	@echo '  audit           run scripts/audit.sh (cargo auditable build, cargo audit bin)'
	@echo '  clean           cargo clean'
	@echo '  help            print this help'
	@echo ''
	@echo 'Variables:'
	@echo '  CARGO        = $(CARGO)'
	@echo '  CARGO_FLAGS  = $(CARGO_FLAGS)'
	@echo '  PREFIX       = $(PREFIX)'
	@echo '                 a leading ~ or ~/ means $$HOME; ~user is not expanded'
	@echo '  DESTDIR      = $(DESTDIR)'
	@echo '  USERUNITDIR  = $(USERUNITDIR)'
	@echo '                 same ~ expansion as PREFIX; for an install into the home'
	@echo '                 directory use ~/.local/share/systemd/user'
	@echo '  BINS         = $(BINS)'
	@echo '  binaries in  $(RELEASE_DIR) (CARGO_TARGET_DIR/release if set)'
	@echo 'Paths must not contain a single quote.'
