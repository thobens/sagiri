---
id: "0018"
title: Session settings and runtime directory
status: todo
depends_on: ["0014"]
---

# 0018 — Session settings and runtime directory

## Why

Session mode needs three settings (the hotkey, how the PRIMARY selection is treated, whether
desktop notifications are shown) and a place for its control socket. Both belong to modules
that already own such things: the config file and the path resolution in `sagiri-core`. This
ticket adds them, without any code that uses them yet.

## Scope

The names are pinned in `tickets/README.md`, section "Binary crate API".

### `sagiri-core/src/config.rs`

- `Config` gains `pub session: SessionConfig`.
- `pub struct SessionConfig { pub hotkey: Option<String>, pub primary: PrimaryMode,
  pub notify: bool }` with `deny_unknown_fields` and defaults `None`, `Off`, `true`.
- `pub enum PrimaryMode { Off, Mirror, Watch }`, written `"off"`, `"mirror"`, `"watch"` in
  the file.
- Example:

  ```toml
  [session]
  hotkey = "ctrl+alt+o"
  primary = "mirror"
  notify = true
  ```

- `hotkey` is stored as written. Its syntax is checked by ticket 0023.
- Errors follow the rules of ticket 0012: they name the file, the key (`session.hotkey`,
  `session.primary`, `session.notify`, or `session` itself) and the line and column, and
  never contain a value from the file. `locate()` learns the `session` table:
  - `session.hotkey`: `expected a string`
  - `session.primary`: `expected "off", "mirror" or "watch"` (for a wrong type and for an
    unknown word alike)
  - `session.notify`: `expected a boolean`
  - `session` that is not a table: `expected a table`

### `sagiri-core/src/paths.rs`

- `pub fn runtime_dir() -> Result<PathBuf, PathError>` and
  `pub fn runtime_dir_with(env: EnvLookup<'_>) -> Result<PathBuf, PathError>`:
  `$XDG_RUNTIME_DIR/sagiri`.
- New `PathError::NoRuntimeDir` with the message
  `XDG_RUNTIME_DIR is not set to an absolute path`. It is returned when the variable is
  unset, empty or relative. There is no fallback to `HOME` or a temporary directory.
- The function only computes the path. It creates nothing.

### `crates/sagiri/src/setup.rs`

- `pub struct Loaded { pub engine: Engine, pub config: Config }`.
- `pub fn load(opts: &Options) -> anyhow::Result<Loaded>`: what `build_engine` does today,
  returning the config as well.
- `build_engine` becomes `load(opts).map(|l| l.engine)`.

### Documentation

- `README.md`, section "Configuration": the `[session]` table with its three keys, their
  defaults and one sentence each. It says that these settings are read by `sagiri-session`
  and have no effect on `sagiri` and `sagiri-cli`.

## Out of scope

- Checking the hotkey syntax (ticket 0023).
- Any code that reads the new settings or creates the runtime directory (ticket 0020).
- New flags or environment variables.

## Design notes

- **Unknown keys stay errors**, also inside `[session]`, for the reason given in ticket
  0012: a typo must not silently switch something off.
- **An unknown word for `primary` must not be echoed.** The deserializer's own message
  quotes the offending value; it has to be replaced by the fixed text above, as the
  existing code already does for other value errors.
- **No fallback for the runtime directory.** A socket in a shared temporary directory
  would be a security problem, and `XDG_RUNTIME_DIR` is set on every Linux desktop session.
  `sagiri-session` offers `--socket` for the rest (ticket 0020).
- `paths.rs` remains the only module that reads path variables.
- Old binaries reject a config file with a `[session]` table. That is the accepted price of
  `deny_unknown_fields`.

## Acceptance criteria

Unit tests in `config.rs`:

- [ ] `session_defaults`: an empty file gives no hotkey, `PrimaryMode::Off` and
      `notify == true`.
- [ ] `session_section_parses`: the example above.
- [ ] `session_primary_accepts_all_three_words`.
- [ ] `unknown_session_key_is_error`: the message names the key and the position.
- [ ] `invalid_primary_value_is_error`: `primary = "on"` and `primary = 1` both name
      `session.primary` with the fixed text; the message does not contain `on`.
- [ ] `wrong_type_for_hotkey_and_notify_is_error`, naming `session.hotkey` and
      `session.notify`.
- [ ] `session_that_is_not_a_table_is_error`.

Unit tests in `paths.rs`:

- [ ] `runtime_dir_uses_xdg_runtime_dir`: `/run/user/1000` gives
      `/run/user/1000/sagiri`.
- [ ] `runtime_dir_unset_empty_or_relative_is_error`: each gives
      `PathError::NoRuntimeDir`, also when `HOME` is set.

Unit test in `setup.rs` or an integration test:

- [ ] `load_returns_config`: with a config file that sets `[session] notify = false`,
      `load` returns that value, and the engine obfuscates as before.

And:

- [ ] Every existing test in `sagiri-core` and `sagiri` passes unchanged, including all
      golden vectors.
- [ ] No test calls `std::env::set_var`.
- [ ] `README.md` documents the `[session]` table.
- [ ] `scripts/check.sh` passes.

## Test plan

- Unit tests in `config.rs` and `paths.rs` in the style of the existing ones (injected
  environment, table-driven).
- One test for `setup::load` with temporary key and config files.
