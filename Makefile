# Build and install tasks for Sagiri. GNU Make.
# The quality gate is scripts/check.sh; `make check` only delegates to it.

CARGO       ?= cargo
CARGO_FLAGS ?= --locked
PREFIX      ?= /usr/local
DESTDIR     ?=
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

.DEFAULT_GOAL := all

.PHONY: all build release $(BINS) install uninstall check clean help

all: release

build:
	$(CARGO) build $(CARGO_FLAGS) --workspace

release:
	$(CARGO) build $(CARGO_FLAGS) --release -p sagiri $(addprefix --bin ,$(BINS))

$(BINS):
	$(CARGO) build $(CARGO_FLAGS) --release -p sagiri --bin $@

# Never builds: `sudo make install` must not leave root-owned files in the target directory.
install:
	@for bin in $(BINS); do \
		if [ ! -x $(RELEASE_DIR)/$$bin ]; then \
			echo "missing $(RELEASE_DIR)/$$bin: run 'make release' first" >&2; \
			exit 1; \
		fi; \
	done
	@for bin in $(BINS); do \
		echo install -Dm755 $(RELEASE_DIR)/$$bin $(BINDIR)/$$bin; \
		install -Dm755 $(RELEASE_DIR)/$$bin $(BINDIR)/$$bin || exit 1; \
	done

uninstall:
	rm -f $(addprefix $(BINDIR)/,$(BINS))

check:
	scripts/check.sh

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
	@echo '  install         copy the release binaries to $$(DESTDIR)$$(PREFIX)/bin (does not build)'
	@echo '  uninstall       remove them from there'
	@echo '  check           run scripts/check.sh'
	@echo '  clean           cargo clean'
	@echo '  help            print this help'
	@echo ''
	@echo 'Variables:'
	@echo '  CARGO        = $(CARGO)'
	@echo '  CARGO_FLAGS  = $(CARGO_FLAGS)'
	@echo '  PREFIX       = $(PREFIX)'
	@echo '  DESTDIR      = $(DESTDIR)'
	@echo '  BINS         = $(BINS)'
	@echo '  binaries in  $(RELEASE_DIR) (CARGO_TARGET_DIR/release if set)'
