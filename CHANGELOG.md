# Changelog

All notable changes to LiteSheet are documented here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- Open files from Finder: LiteSheet registers for `.xlsx`, `.xlsm`, `.xls`, `.ods`, `.csv` and `.tsv` (as an alternative, so it doesn't take over the default app), and handles double-click, *Open With* and dropping on the Dock icon, both at launch and while running. Unsaved changes trigger the usual save prompt, which now also applies to files dropped on the window
- Charts: column, bar, line, area, pie and scatter, inserted from Insert → Chart… or the toolbar with a live preview. Charts can be moved, resized, edited and deleted, update with their data, follow structural changes, and are saved to and read from `.xlsx` as native Excel charts
- Freeze panes (View menu): freeze the rows and columns above and left of the active cell, the top row, or the first column. Frozen panes follow inserted and deleted rows and columns, can be undone, and are kept in `.xlsx` files
- Merged cells: Merge & Center (toolbar), Merge Cells and Unmerge (Format menu). A merged range selects, navigates and edits as one cell, follows inserted and deleted rows and columns, and is kept when opening and saving `.xlsx`
- Cell styles: bold, italic, underline, strikethrough, text and fill colours, alignment and borders, from a new formatting toolbar and ⌘B/⌘I/⌘U. Styles survive typing, copy/paste, fill and sort; Edit → Clear Formats and Clear All remove them
- Opening an `.xlsx` file now keeps its fonts, colours (including theme colours and tints), fills, borders, alignment, number formats and column widths, and saving writes them back
- Number formats: Number with thousands separators, Currency ($, €, £, ¥, ₹), Percent and Scientific with a chosen number of decimals, Time and Text, plus Increase/Decrease Decimal. Currency, times and grouped numbers are recognized as you type, formats stay on empty cells, and all formats are saved to `.xlsx` as Excel format codes
- Insert and delete rows and columns (Edit menu and right-click on headers or cells). References in every sheet are adjusted the way Excel does it: ranges grow and shrink, references to deleted cells become `#REF!`, and column widths move with their columns
- Renaming a sheet updates every formula that refers to it, and the rename can be undone

### Changed

- Release builds are signed with a Developer ID and notarized when the Apple signing secrets are configured

### Fixed

- Fixed-decimal display rounds like Excel (half away from zero, after rounding to 15 significant digits)
- Install instructions for macOS 15 and later, where right-click → Open no longer bypasses Gatekeeper

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
