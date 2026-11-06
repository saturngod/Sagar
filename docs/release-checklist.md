# Release checklist — Sagar EPUB Translation Studio

Status legend: [x] verified, [ ] open. "Verified" means executed with
recorded evidence on the named OS; nothing is marked verified without it.

## Data-loss cases

- [x] Editing a block, switching chapters, reopening the app restores the
      last committed translation (autosave flush on navigation; revision
      checks; covered by storage/integration tests).
- [x] Save failure keeps the local draft and shows a persistent Retry state
      (unit-tested revision conflicts; UI shows per-block errors).
- [x] Interrupted import leaves no partial project (single transaction).
- [x] Export never writes to the source file or the stored snapshot
      (integration-tested rejection).
- [ ] App killed mid-autosave: recovery tested by process interruption
      (partially covered by commit-only recovery tests; a dedicated
      kill-during-save test is open).
- [ ] Database upgrade path from future schema changes re-tested with a
      real user database.

## Export validation

- [x] Translated fixture round trip: patch, repackage (mimetype first,
      stored), validate, re-import and compare spine/titles (integration
      tests).
- [x] Untranslated blocks and unsupported content preserved byte-for-byte.
- [x] Metadata (dc:title, dc:language) and nav labels updated without
      breaking hrefs.
- [ ] EPUBCheck run on the exported fixture set (needs Java; not installed
      in this environment). The app does **not** claim full EPUB
      conformance from its internal checks.
- [ ] Exported book opened in at least one third-party reader per OS.

## Platforms

- [x] macOS (Apple Silicon): unit/integration suites, dev boot, UI smoke.
- [ ] macOS (Intel): GitHub release build, install, run, import/export,
      Myanmar shaping (release target configured; execution unverified).
- [ ] Windows: install, run, import/export, Myanmar shaping (untested).
- [ ] Linux: install, run, import/export, Myanmar shaping (untested).
- [ ] Windows ARM64: tagged release build and native install/import/export
      smoke test (`windows-11-arm`, NSIS installer; execution unverified).
- [ ] Linux ARM64: tagged release build and native install/import/export
      smoke test (`ubuntu-22.04-arm`; execution unverified).
- [x] macOS (Apple Silicon) bundle: `bun tauri build` produced
      `Sagar.app` (9.94 MiB) and `Sagar_0.1.0_aarch64.dmg` (5.40 MiB)
      during the 2026-10-13 release sweep;
      launch smoke passed in dev; installer opened on a clean machine is
      still recommended before distribution.
- [ ] Windows/Linux installers (cannot be built or tested from macOS here).

## Accessibility & typography

- [x] Keyboard: chapter list, splitters, editor, dialogs operable without
      the mouse; focus visible; dialogs restore focus on Escape.
- [ ] Screen reader pass (VoiceOver at minimum) on real content.
- [x] Light/dark text, muted/disabled labels and status token pairs measured
      at >= 4.5:1 by `tests/frontend/contrast.test.ts` (2026-10-13).
      Native rendered controls and screen-reader verification remain open.
- [x] Myanmar text input, wrapping and storage verified via synthetic
      fixtures at the storage level (UTF-8 intact; combining marks are text).
- [ ] Myanmar rendering check with real Padauk/Myanmar Text fonts on each
      target OS (WebView shaping differs across platforms).

## Privacy

- [x] No network access in the request path: book content renders only from
      the app-owned snapshot protocol; external book URLs are never fetched;
      CSP restricts the WebView.
- [x] No telemetry, no logging of book text or filesystem paths in errors.
- [ ] CSP reviewed again after any new WebView feature (current policy in
      `src-tauri/tauri.conf.json`).

## Performance

- [x] Chapters load on demand; export streams per-resource from the
      snapshot.
- [x] Large-book service sweep on Apple Silicon macOS (2026-10-13):
      5,001 chapters, 56,857,768 bytes; import 3.89 s, export 12.05 s;
      export reimported successfully. Maximum resident set size 313,229,312
      bytes for the timed Cargo/test invocation. Native WebView memory and
      other platforms remain unmeasured.

Reproduce: in `src-tauri`, run
`cargo test --locked --test epub_import large_book_import_export_sweep -- --ignored --nocapture`.
On macOS, prefix with `/usr/bin/time -l` to record peak resident memory.
The fixture is synthetic and removed automatically after the sweep.

Cross-platform PR/manual build automation: `.github/workflows/desktop-checks.yml`.
It runs service tests and the sweep, then retains installers as artifacts;
it does not publish. No remote run or native installer pass is claimed yet.

## Local macOS package verification — 2026-10-13

The full ad hoc app signature passes `codesign --verify --deep --strict` and
the DMG passes `hdiutil verify`. Reproduce the local packaging command with
`bun tauri build --config '{"bundle":{"macOS":{"signingIdentity":"-"}}}'`.
This local override does not change release signing configuration. These
artifacts are not Developer ID signed or notarized; clean-machine installation
is still open. The latest Settings focus fix is verified in the dev build
separately and requires a fresh package rebuild before distribution.
