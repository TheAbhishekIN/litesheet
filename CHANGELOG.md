# Changelog

All notable changes to LiteSheet are documented here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses [Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.3.1] - 2026-10-15

### Added

- AI provider presets for OpenAI, Anthropic (Claude), OpenRouter, Groq, Ollama and LM Studio, each with its base URL and a current default model (`gpt-6-luna` for OpenAI), plus any OpenAI-compatible API. Each provider keeps its own key in the Keychain
- **Load models** lists the models your provider offers and checks the key
- Model options: temperature, max reply length, reasoning effort and timeout

### Fixed

- The AI chat failed with "temperature does not support 0.2" on models that only accept their default temperature. LiteSheet now sends no temperature unless you set one, and if a model rejects any option it drops that option and retries
- The AI now sees every distinct value of columns with up to 50 of them, so questions like "what are the unique codes?" get a complete answer
- Clearer errors for a bad key, an unknown model, rate limits and timeouts
- Closing Find from the toolbar now clears the search highlights, and the toolbar's filter Clear button shows its icon

## [0.3.0] - 2026-10-14

### Added

- Several workbooks open at once, in tabs. Opening a file (File → Open, Finder, drag and drop) adds a tab instead of replacing the workbook you're in. Open… can pick several files. ⌘W closes a tab, ⌃Tab / ⌃⇧Tab switch tabs, and copy and paste work between workbooks. Quitting asks about each workbook with unsaved changes
- The AI panel is now a chat about your data, with follow-up questions, Stop, Retry and New chat. Replies with a formula get *Insert* and *Copy* buttons, and suggested filters get an *Apply filter* button. An empty chat offers quick starts
- Automatic updates. LiteSheet checks for new versions, downloads them in the background, verifies the signature, and installs with *Restart to update* or the next time you quit. Help → Check for Updates… and Help → Install Updates Automatically
- Sort, Filter, Freeze, Find and AI buttons on the right of the toolbar

### Changed

- Opening large files is about 3× faster. A 10 MB `.xlsx` with 2.1 million cells now opens in about 2 s instead of 6.6 s
- The loading screen shows the file name and size, what's being read, the elapsed time and a Cancel button, instead of dim text on a black background


## [0.2.1] - 2026-10-13

### Changed

- The Mac app is signed with a Developer ID and notarized by Apple, so it opens without the "Apple could not verify" warning. The install instructions in the README and on the website are simpler to match

## [0.2.0] - 2026-10-13

The whole roadmap from 0.1.0: styling, structure and charts, with much better Excel fidelity.

### Added

- Cell styles: bold, italic, underline, strikethrough, text and fill colours, alignment and borders, from a new formatting toolbar and ⌘B/⌘I/⌘U. Styles survive typing, copy/paste, fill and sort; Edit → Clear Formats and Clear All remove them
- Opening an `.xlsx` file now keeps its fonts, colours (including theme colours and tints), fills, borders, alignment, number formats and column widths, and saving writes them back
- Charts: column, bar, line, area, pie and scatter, inserted from Insert → Chart… or the toolbar with a live preview. Charts can be moved, resized, edited and deleted, update with their data, follow structural changes, and are saved to and read from `.xlsx` as native Excel charts
- Number formats: Number with thousands separators, Currency ($, €, £, ¥, ₹), Percent and Scientific with a chosen number of decimals, Time and Text, plus Increase/Decrease Decimal. Currency, times and grouped numbers are recognized as you type, formats stay on empty cells, and all formats are saved to `.xlsx` as Excel format codes
- Merged cells: Merge & Center (toolbar), Merge Cells and Unmerge (Format menu). A merged range selects, navigates and edits as one cell, follows inserted and deleted rows and columns, and is kept when opening and saving `.xlsx`
- Freeze panes (View menu): freeze the rows and columns above and left of the active cell, the top row, or the first column. Frozen panes follow inserted and deleted rows and columns, can be undone, and are kept in `.xlsx` files
- Insert and delete rows and columns (Edit menu and right-click on headers or cells). References in every sheet are adjusted the way Excel does it: ranges grow and shrink, references to deleted cells become `#REF!`, and column widths move with their columns
- Renaming a sheet updates every formula that refers to it, and the rename can be undone
- Open files from Finder: LiteSheet registers for `.xlsx`, `.xlsm`, `.xls`, `.ods`, `.csv` and `.tsv` (as an alternative, so it doesn't take over the default app), and handles double-click, *Open With* and dropping on the Dock icon, both at launch and while running. Unsaved changes trigger the usual save prompt, which now also applies to files dropped on the window
- A Help menu (website, source code, report an issue) and a Buy Me a Coffee button in the menu bar, on the website and in the README

### Changed

- The website works well on phones: compact navigation, stacked buttons and tap-to-enlarge screenshots
- README screenshots, the website and `samples/sales.xlsx` show the new styling, frozen header and charts
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

[Unreleased]: https://github.com/TheAbhishekIN/litesheet/compare/v0.3.1...HEAD
[0.3.1]: https://github.com/TheAbhishekIN/litesheet/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/TheAbhishekIN/litesheet/compare/v0.2.1...v0.3.0
[0.2.1]: https://github.com/TheAbhishekIN/litesheet/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/TheAbhishekIN/litesheet/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/TheAbhishekIN/litesheet/releases/tag/v0.1.0
