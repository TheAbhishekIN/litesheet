# Contributing to LiteSheet

Thanks for your interest in LiteSheet! Bug reports, ideas, documentation fixes and code are all welcome. This guide explains how to get set up and what we look for in a pull request.

By taking part in this project, you agree to follow the [Code of Conduct](CODE_OF_CONDUCT.md).

## Ways to help

- **Report a bug.** Open an issue using the *Bug report* template. A file or a short list of steps that reproduces the problem helps most. Please remove any private data first.
- **Suggest a feature.** Use the *Feature request* template and describe the problem you're trying to solve, not just the solution.
- **Pick up an issue.** Issues labelled `good first issue` are a gentle start, and `help wanted` marks things we'd love help with. Leave a comment so others know you're working on it.
- **Improve the docs.** The README, this guide and [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) are fair game.

For anything bigger than a small fix, open an issue (or comment on an existing one) before you start, so we can agree on the approach.

## Development setup

You need:

- [Rust](https://rustup.rs/) **1.95 or newer** (`rustup update stable`)
- **macOS:** the Xcode Command Line Tools (`xcode-select --install`)

Then:

```bash
git clone https://github.com/TheAbhishekIN/litesheet.git
cd litesheet
cargo run                         # debug build
cargo run --release -- samples/sales.xlsx
```

The first build compiles about 400 crates and takes a few minutes; later builds are incremental.

## Project layout

```text
litesheet/
├── src/
│   ├── main.rs          # Binary entry point: window setup only
│   ├── lib.rs           # Library root
│   ├── model/           # Workbook data, formulas, undo, filters (no UI)
│   ├── io/              # .xlsx and .csv reading and writing
│   ├── ai/              # OpenAI-compatible client, prompts, Keychain
│   └── ui/              # egui app: grid, menus, panels, keyboard
│       └── tests/       # Headless UI tests and doc-asset generation
├── assets/              # App icon
├── docs/                # Architecture notes and README screenshots
├── samples/             # Example workbook
└── scripts/             # Bundling and asset-generation scripts
```

[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) explains how the pieces fit together and where to make common changes. Read it before your first code change.

## Before you open a pull request

Run the same checks as CI:

```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test
```

### Tests

- **Model, file and AI parsing logic:** add unit tests next to the code, in a `#[cfg(test)] mod tests` block or the module's `tests.rs`.
- **UI behaviour:** add a test in `src/ui/tests/`. These tests drive the real `App` with mouse and keyboard events through [`egui_kittest`](https://crates.io/crates/egui_kittest) and need no window or GPU. See `interaction.rs` for examples.
- **AI features:** test against the mock OpenAI server in `src/ui/tests/mod.rs` (`mock_ai_server`). Tests must never call a real provider or read the real Keychain.
- **Seeing what a UI test does:** save a screenshot of every test with:
  ```bash
  LITESHEET_SHOTS=/tmp/shots cargo test ui::tests
  ```
- **Performance:** if a change could affect speed (loading, recalculation, scrolling), compare timings before and after with:
  ```bash
  cargo test --release perf_big_file -- --ignored --nocapture
  ```

### Screenshots

If your change affects how the app looks, regenerate the README screenshots, sample workbook and icon:

```bash
./scripts/generate-docs-assets.sh
```

Commit the updated files under `docs/screenshots/` with your change.

## Coding guidelines

- **Follow the existing style.** `rustfmt` settings are in `rustfmt.toml`. Match the comment density and naming of the code around you.
- **Keep the layers separate.** `model` and `io` must not depend on `egui` or `ui`. `ai` holds no UI state. Putting logic in `model` makes it easy to unit-test.
- **Avoid panics in app code.** Show errors with `App::error`, which displays them in the status bar. Use `anyhow` for file I/O errors.
- **Mind performance.** Sheets can have a million cells, so avoid work proportional to the whole sheet on every frame. Cache it (see `selection_stats`) or do it only when data changes (`App::data_changed`).
- **Make every data change undoable.** Route edits through `Book::apply` (or `Book::set_input`) so each edit is one undo step.
- **Respect privacy.** Never add network calls, telemetry or logging of cell data. AI context must stay limited and visible to the user (the panel shows how much was sent).
- **Add dependencies sparingly.** Prefer what's already in the tree, and explain any new crate in your PR.
- **Use symbols that render.** egui's bundled fonts don't include every Unicode symbol (⇧, →, ✕ and ▾ are missing), so check new UI text in a screenshot.

## Commits and pull requests

- Keep each pull request focused on one change. Small PRs get reviewed faster.
- Write commit messages in the imperative mood, with a short summary line (e.g. `Add row insertion to the grid context menu`) and a body explaining *why* when it isn't obvious.
- Fill in the pull-request template: what changed, how you tested it, and screenshots for UI changes.
- Add a line to the `Unreleased` section of [CHANGELOG.md](CHANGELOG.md) for anything a user would notice.
- CI must pass. A maintainer will review your PR, usually within a few days.

## Releasing (maintainers)

1. Update the version in `Cargo.toml`, and move the `Unreleased` entries in `CHANGELOG.md` under the new version.
2. Merge that through a PR, then tag `main`: `git tag -a v0.2.0 -m "LiteSheet 0.2.0" && git push origin v0.2.0`.
3. The `Release` workflow builds a universal `LiteSheet.app`, zips it as `LiteSheet-macos.zip` and publishes the GitHub release. The website's download button always points at the latest release's zip.

### Signing and notarization

Without signing secrets, the release is ad-hoc signed and macOS asks users to approve it on first launch. With an [Apple Developer Program](https://developer.apple.com/programs/) membership, add these repository secrets (*Settings → Secrets and variables → Actions*) and the workflow signs and notarizes automatically:

| Secret | Value |
|---|---|
| `APPLE_CERTIFICATE_P12` | Your *Developer ID Application* certificate and private key, exported from Keychain Access as `.p12`, then base64-encoded (`base64 -i cert.p12 \| pbcopy`) |
| `APPLE_CERTIFICATE_PASSWORD` | The password you set when exporting the `.p12` |
| `APPLE_SIGNING_IDENTITY` | The identity name, e.g. `Developer ID Application: Your Name (TEAMID1234)` |
| `APPLE_ID` | The Apple ID email of the developer account |
| `APPLE_TEAM_ID` | Your 10-character Team ID |
| `APPLE_APP_PASSWORD` | An [app-specific password](https://support.apple.com/102654) for that Apple ID |

Locally, `SIGN_IDENTITY="Developer ID Application: …" ./scripts/bundle-macos.sh` signs with the hardened runtime.

## License

LiteSheet is dual-licensed under MIT and Apache-2.0. Unless you explicitly state otherwise, any contribution you intentionally submit for inclusion in LiteSheet, as defined in the Apache-2.0 license, will be dual licensed as above, without any additional terms or conditions.
