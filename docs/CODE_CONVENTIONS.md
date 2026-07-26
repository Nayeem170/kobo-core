# Code Conventions - kobo-core

The rule book for this published hardware-adapter SDK. Every new or changed file must
follow it. This crate is consumed by the KoThok app and any downstream crate targeting
Kobo hardware. Rules are normative: **MUST** = required, **SHOULD** = strong default.

---

## 0. Design principles

- **SDK mindset.** Everything `pub` is part of a semver contract. Internal modules
  communicate by direct calls; traits are reserved for the one place a real substitution
  happens (`Capabilities` trait - real device vs `MockCapabilities` for tests).
- **Fix the root cause, never mask it.** No fallback that hides a bug.
- **One responsibility per module.** `device/bt/` = Bluetooth, `device/wifi/` = WiFi,
  `device/fb.rs` = framebuffer, `rendering/` = pixel/text output, `formats/` = EPUB/html
  parsing. No grab-bag modules.
- **Fail loud at boundaries, fail soft on device paths.** Setup functions may return
  `Result`/`Option` for the caller to handle. Device I/O paths (sysfs writes, ioctl) log
  at `warn` and degrade gracefully.
- **Separation of pure logic from hardware.** SSID parsing, text extraction, pagination
  math are pure functions with unit tests. sysfs/ioctl/Command calls are in `device/*`
  wrappers.
- **KISS.** This is a constrained ARM target. No clever abstractions, no speculative
  generality. Boring, obviously-correct code wins.

---

## 1. Module & file organization

- **MUST** keep files under **~400 lines**. Split by responsibility past that.
- **MUST** keep functions under **~60 lines**.
- **MUST** be one responsibility per module. Directory splits follow the pattern:
  `device/bt/{mod.rs, discover.rs}`, `device/wifi/{mod.rs, power.rs, scan.rs, wpa.rs}`.
- **MUST NOT** pass **5+ parameters** to a function. Group into a named `struct`.
- **SHOULD** order a file: `use` -> consts -> types -> `impl`s -> free functions -> `#[cfg(test)]`.

---

## 2. Naming

- `snake_case` items/modules/files, `CamelCase` types/traits, `SCREAMING_SNAKE_CASE`
  consts. (`cargo fmt` enforced.)
- Names state intent: `wifi_status`, `bt_toggle`, `frontlight_set`. Booleans read as
  predicates: `is_charging`, `wifi_connected`.
- No abbreviations beyond the established domain set (`fb`, `bt`, `tts`, `px`).

---

## 3. Types over tuples

- **MUST NOT** thread tuples with **3+ fields** through more than one function. Define a
  named `struct`.
- 2-tuples for obvious pairs (`(w, h)`, `(start, end)`) are fine.
- **SHOULD** use newtypes for unit-bearing integers that get mixed up (byte offsets vs
  page indices, raw touch coords vs display coords).

---

## 4. Constants - no magic numbers or strings

- **MUST** name every non-trivial literal as a `const` with a unit/why in the name.
- **MUST** group related consts in their owning module (`fb.rs` for ioctl/fb codes,
  `paths.rs` for device filesystem paths).
- **MUST NOT** re-literal `/sys/class/...`, `/dev/...` paths inline. They live in
  `device/paths.rs` or `device/config.rs`.
- A string compared or emitted in **more than one** place MUST be a named const.

---

## 5. Error handling & subprocess safety

- **MUST NOT** use bare `let _ = fallible();` without a `// best-effort: <why>` comment.
- **SHOULD** use typed error enums (`EpubError`, `TtsError`) at public boundaries where
  callers branch on the variant.
- **MUST** log every swallowed device-I/O failure at `warn`.
- **MUST** build subprocess args as an **argv array**
  (`Command::new("btmgmt").args([...])`), never interpolate external input (device names,
  SSIDs) into a shell string. BT device paths and WiFi SSIDs are attacker-influenceable
  RF input on a device that pairs/scans automatically.
- **MUST** guard EPUB/XHTML parsing (untrusted book files) against zip-bomb
  (decompression-size limit) and XXE (disable external entity resolution).

---

## 6. Logging

- **MUST** use the `log` crate (`error`/`warn`/`info`/`debug`/`trace`), not `println!`.
- **MUST NOT** set a global logger. This is a library crate; the application owns the
  logger. (See `logger.rs` - it provides a `FileLogger` for the app to install, not for
  this crate to use at init.)
- **MUST NOT** leave `debug!` calls in committed code. Use `info!`, `warn!`, `error!`
  only in production paths.

---

## 7. Comments

- **No comments unless they explain *why*** (a hardware quirk, a non-obvious constraint,
  a deliberate best-effort ignore). Do not narrate *what* the code does.
- **MUST** write doc comments (`///`) on all `pub` items - this is a published SDK.

---

## 8. Resource management (RAII)

- **MUST** wrap every acquired OS resource (mmap, fd, wakelock, A2DP socket) in a type
  with `impl Drop` so cleanup is automatic.
- **Drop does NOT run on panic=abort.** Do not add code that depends on cleanup running
  mid-crash.

---

## 9. Unsafe code

- **MUST** precede every `unsafe` block with a `// SAFETY:` comment stating the
  invariant (pointer validity, length/alignment, lifetime, exclusive access).
- **MUST** keep each `unsafe` block as small as possible.
- **SHOULD** wrap recurring unsafe patterns (e.g. the RGB565 buffer->bytes cast) in one
  helper with the SAFETY argument written once.

---

## 10. Panic policy

- **MUST NOT** use `unwrap()` / `expect()` / `slice[i]` indexing on device I/O paths
  where the value can be absent/out-of-range. Use `.get()`, `?`, `unwrap_or(...)`.
- **`expect()` is allowed only in one-time startup wiring** where failure means the
  device genuinely cannot run.
- **MUST** validate any index derived from touch coords, page/chapter numbers, or
  offsets before using it to index.

---

## 11. Tests

- **MUST** keep pure logic desktop-unit-tested. Logic added to `formats/`, `html_text/`,
  `rendering/`, SSID/network parsers (`device/wifi/scan.rs`) ships with tests.
- **MUST** pass `cargo test` before commit.
- **SHOULD** use `MockCapabilities` for testing code that depends on device state.

---

## 12. Build & formatting

- **MUST** use **LF** line endings.
- **MUST** be `cargo fmt`-clean; **SHOULD** be `cargo clippy`-clean.
- **MUST** compile with `--no-default-features` (feature gates must be correct).

---

## 13. Backward compatibility (published crate)

`kobo-core` is published to crates.io. Its public API is a **semver contract** with
downstream users.

- **MUST NOT** ship a breaking public-API change in a patch or minor release while
  pre-1.0. Bump the version: `0.3` -> `0.4` for breaking, `0.3.0` -> `0.3.1` for patch.
- **MUST** preserve public-API paths across internal refactors. Module splits keep old
  paths reachable via `pub use` re-exports.
- **SHOULD** deprecate (`#[deprecated]`) for one release before removing.
- **MUST** maintain a `CHANGELOG.md` (Keep-a-Changelog format), updated in the same
  commit as the version bump.

---

## 14. Security & supply chain

- **SHOULD** run `cargo audit` before merging dependency changes.
- **MUST NOT** add a dependency without checking it is maintained, permissively licensed,
  and does not duplicate an existing dependency.
- **MUST** keep `.env` files and secrets gitignored.

---

## 15. Review process

- **SHOULD** keep PRs under ~400 changed lines (excluding generated/lockfiles).
- **MUST** have CI green before merge.
- **SHOULD** use Conventional Commits (`feat:`, `fix:`, `chore:`, `docs:`, `refactor:`).

---

## Quick checklist before committing

- [ ] File < ~400 lines, function < ~60 lines
- [ ] No 3+ field tuple threaded across functions
- [ ] No function with 5+ params
- [ ] No magic numbers or strings / inline device paths
- [ ] No bare `let _ =` without a `// best-effort:` reason
- [ ] Every `unsafe` block has a `// SAFETY:` comment
- [ ] No `unwrap`/`expect`/`[i]` indexing on device paths
- [ ] `log`, not `println!`; no global logger set in this crate
- [ ] Doc comments on all `pub` items
- [ ] `cargo fmt` clean, LF endings
- [ ] `cargo test` green
- [ ] No breaking public-API change without a version bump + CHANGELOG entry
- [ ] Subprocess args as argv array, never shell string
- [ ] No secrets staged
