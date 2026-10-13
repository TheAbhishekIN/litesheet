# Architecture

This document explains how LiteSheet is put together, so you can find the right place to make a change. It describes the code as of version 0.1.

## Layers

LiteSheet is a library crate (`src/lib.rs`) plus a small binary (`src/main.rs`) that opens the window. The library has four modules. Each depends only on the modules listed before it:

```text
model  ←  io  ←  ai  ←  ui
```

| Module | Depends on | Responsibility |
|---|---|---|
| `model` | formualizer, chrono | Cells, values, formulas, undo/redo, filtering. No UI code. |
| `io` | model, calamine, rust_xlsxwriter, csv | Converting files to and from a `Book` |
| `ai` | model, reqwest, tokio, keyring | Prompts, the HTTP client, reading replies, key storage |
| `ui` | everything, egui/eframe | The desktop app |

Keeping `model` free of UI code means almost all spreadsheet behaviour can be unit-tested quickly, without a window.

## `model`: the data model

```text
model/
├── mod.rs      re-exports the public API
├── cell.rs     Pos, Cell (raw input + NumFmt), interpret_input()
├── value.rs    Value (computed result), parse_literal(), compare_values()
├── format.rs   number/date/percent display, Excel date serials
├── sheet.rs    Sheet: cells, values, column widths, used extent
├── book.rs     Book: sheets, undo/redo, formula-engine bridge
├── refs.rs     A1 references, shift_formula() for relative references
├── filter.rs   ViewState: column filters, search, visible rows
└── tests.rs
```

### Cells and values

Each cell stores two things:

- **`Cell`**: what the user typed (`raw`, e.g. `=SUM(A1:A3)`, `42`, `'00042`) and a display format (`NumFmt`).
- **`Value`**: the computed result (`Number`, `Text`, `Bool`, `Error` or `Empty`).

`Sheet` keeps both in `HashMap<Pos, _>`, so empty cells cost nothing. `Pos` is a zero-based `(row, col)`.

Typed text is turned into a `Cell` by `interpret_input`: `2024-05-01` becomes a date serial with `NumFmt::Date`, and `15%` becomes `0.15` with `NumFmt::Percent`. The same function is used when importing CSV.

### The formula engine

`Book` owns a [formualizer](https://crates.io/crates/formualizer) `Workbook`, which acts as the calculation engine. The rule is:

> Every write goes through `Book::write_cell`, which updates both our `Sheet` and the engine. After a batch of writes, `Book::recalc` evaluates the engine and copies the results of formula cells back into `Sheet::values`.

Plain values are parsed by us (`parse_literal`) and passed to the engine as literals. Formulas are passed as text. The engine converts between its 1-based coordinates and our 0-based ones.

**Performance note:** the engine stores data in columns, and growing a column one row at a time is O(n²). `Book::reserve` grows the engine to the final size before a bulk write. Without it, loading 200k rows took 48 s instead of 1.5 s.

### Undo and redo

We keep our own history rather than using the engine's. `Book::apply(label, edits)` is the only entry point for changes that can be undone:

1. Record each cell's old and new `Cell` as a `CellChange`.
2. Write the new cells, then recalculate.
3. Push one `HistoryEntry` for the whole batch.

Undo replays the batch in reverse with the old cells. Paste, fill, sort, clear and format each produce a single entry. Sheet-level operations (add, rename, delete a sheet) are not undoable, and deleting a sheet clears the history, because entries refer to sheets by index.

### Inserting and deleting rows and columns

`Book::structural` moves every cell after the insertion point, rewrites references in all sheets with `refs::adjust_for_structure`, and shifts column widths, all in one history entry. Unlike `shift_formula`, structural adjustment moves absolute (`$`) references too, grows or shrinks ranges, and turns references to deleted cells into `#REF!`.

### References

`refs::shift_formula` moves the relative parts of A1 references in a formula. It skips string literals, quoted sheet names and function names like `LOG10`, keeps `$` anchors, and turns references pushed off the sheet into `#REF!`. Fill, paste and sort all use it, so `=E2*F2` filled down becomes `=E3*F3`.

### Filtering and search

Each sheet has a `ViewState` in the UI layer (it lives in `model::filter` so it can be tested). `ViewState::refresh` recomputes:

- `matches`: the cells containing the search text, in row-major order
- `rows`: the visible rows (`None` means every row is visible), after column filters and the optional "only matching rows" option

The grid works in view indexes (0..number of visible rows) and converts them to data rows with `App::row_at` and `App::view_index`.

## `io`: files

`io::load` and `io::save` choose a format from the file extension:

- `io/csv.rs`: CSV/TSV. Fields go through `interpret_input`, so dates and numbers are recognised. Saving writes the displayed values of the current sheet.
- `io/xlsx.rs`: reading uses calamine (`.xlsx`, `.xlsm`, `.xls`, `.ods`), taking both cell values and formula text. Writing uses rust_xlsxwriter and stores each formula together with its computed result, so other apps show values without recalculating.

## `ai`: Bring Your Own Key

```text
ai/
├── mod.rs        Task (prompts), Request/Response, reply parsing
├── client.rs     spawn(): POST /chat/completions on the tokio runtime
├── settings.rs   AiSettings (base URL, model) ↔ settings.json
└── keychain.rs   the API key in the system keychain
```

A request runs on a two-thread tokio runtime owned by `App`. The result comes back through an `mpsc` channel, which `App::poll_ai` reads at the start of each frame, so the UI never waits on the network.

The spreadsheet data sent with each task is built in `ui/ai_panel.rs` (`ai_context`) and capped at 60 KB. The Filter task asks the model for JSON (`{"filters": [...]}`), which `parse_filters` turns into ordinary `ColumnFilter`s. The model never runs anything; it only proposes filters or a formula that the user can see.

## `ui`: the desktop app

`App` (`ui/app.rs`) holds all UI state: the `Book`, one `ViewState` per sheet, the selection, the active edit, dialogs and the AI panel. Its methods are spread over files by area, each adding an `impl App` block:

| File | What it handles |
|---|---|
| `app.rs` | State, constructor, view/row mapping, frame layout (`eframe::App::ui`) |
| `grid.rs` | The virtualized grid: layout, painting, hit-testing, mouse, column resize, in-cell editor, context menu |
| `editing.rs` | Starting, committing and cancelling edits; point mode for formulas |
| `commands.rs` | Clear, copy/cut/paste, fill, sort, undo/redo, number formats |
| `keyboard.rs` | Every keyboard shortcut and grid navigation |
| `menu.rs` | The menu bar |
| `formula_bar.rs` | Name box and formula bar |
| `search.rs` | The find bar |
| `filtering.rs` | The filter window and applying filters |
| `files.rs` | Open, save, export, background loading, unsaved-changes dialog |
| `sheet_tabs.rs` / `status_bar.rs` | Bottom bars |
| `ai_panel.rs` | The AI side panel and the context each task sends |
| `selection.rs` / `util.rs` | Selection rectangle and small helpers |

### One frame

`App::ui` runs once per frame:

1. Read finished file loads and AI replies (`poll_loading`, `poll_ai`).
2. Handle keyboard shortcuts (`shortcuts`) *before* any widget is drawn, so keys like Enter and Tab can be taken before a text field sees them.
3. Lay out the panels: menu, formula bar, status bar, sheet tabs, AI panel, then the grid in the remaining space.
4. Draw floating windows (the filter window and the unsaved-changes dialog).

### The virtualized grid

`grid_ui` gives a `ScrollArea` the full content size (every column width plus row count × row height) and uses `show_viewport` to learn which part is visible. Only the visible cells are laid out and painted. Headers are painted last, pinned to the viewport edges, so they stay on top while scrolling.

The whole grid is one interactive area. Mouse positions are mapped to a `Hit` (cell, header, resize handle, filter button) by `hit_test`.

### Editing and focus

An edit is `App::edit: Option<EditState>`. The cell editor and the formula bar edit the same text buffer, and only one of them has focus. Two egui details shape this code:

- Typing on a selected cell starts an edit from the `Text` event. That event is removed from egui's input, so the newly focused editor doesn't type the character twice.
- egui drops focus when you press Esc or click the grid. The keyboard handler therefore treats "no widget focused" as "the editor still owns the keyboard", and point mode gives focus back to the editor.

## Testing

| Kind | Where | Notes |
|---|---|---|
| Unit | next to the code (`#[cfg(test)]`) | Model, references, formatting, filters, file round trips, AI reply parsing |
| UI | `src/ui/tests/` | Drive the real `App` with egui_kittest: clicks, typing, shortcuts. No window or GPU needed. |
| AI end to end | `src/ui/tests/ai.rs` | A mock OpenAI-compatible HTTP server on localhost |
| Performance | `src/ui/tests/perf.rs` (ignored) | 200k-row timings; run in release mode |
| Doc assets | `src/ui/tests/docs_assets.rs` (ignored) | Renders the README screenshots with wgpu, writes the sample workbook and draws the icon |

## Where do I change…?

| I want to… | Look at |
|---|---|
| Support a new file format | `io/`: add a module and dispatch on the extension in `io/mod.rs` |
| Add a number format | `NumFmt` in `model/cell.rs`, `format_value` in `model/format.rs`, the Format menu in `ui/menu.rs`, `save` in `io/xlsx.rs` |
| Add a keyboard shortcut | `ui/keyboard.rs`, and show it in `ui/menu.rs` |
| Add a filter operator | `Op` in `model/filter.rs` (and the Filter prompt in `ai/mod.rs`) |
| Change what the AI sees | `ai_context` in `ui/ai_panel.rs` |
| Add an AI task | `Task` in `ai/mod.rs`, then the UI and handling in `ui/ai_panel.rs` |
| Change grid drawing | `paint` and `paint_headers` in `ui/grid.rs` |
