# Changelog

All notable changes to LiteSheet are documented here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Fixed

- Install instructions for macOS 15 and later, where right-click → Open no longer bypasses Gatekeeper

### Changed

- Release builds are signed with a Developer ID and notarized when the Apple signing secrets are configured

## [0.1.0] - 2026-10-13

First public release.

### Added

- Open `.xlsx`, `.xlsm`, `.xls`, `.ods`, `.csv` and `.tsv`; save `.xlsx` and `.csv`
- Virtualized grid that stays smooth with hundreds of thousands of rows
- Multiple sheets with tabs (add, rename, delete)
- In-cell and formula-bar editing, name box navigation, point mode for building formulas
- 400+ Excel-compatible functions via formualizer, with automatic recalculation and cross-sheet references
- Cut, copy and paste (tab-separated, compatible with other spreadsheet apps), fill down and fill right
- Sorting, column filters (conditions and value checklists) and search with highlighting
- 200-step undo and redo
- General, Percent, Date and Date & Time number formats
- Selection Sum, Average and Count in the status bar
- BYOK AI: generate formulas, explain selections, filter in plain English and summarize sheets, with any OpenAI-compatible provider; the API key is stored in the macOS Keychain
- macOS `.app` bundle script with an optional universal (Apple Silicon + Intel) build

[Unreleased]: https://github.com/TheAbhishekIN/litesheet/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/TheAbhishekIN/litesheet/releases/tag/v0.1.0
