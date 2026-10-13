//! LiteSheet: a lightweight spreadsheet with an Excel-compatible formula
//! engine and Bring-Your-Own-Key AI.
//!
//! The crate is split into layers that only depend downwards:
//!
//! - [`model`]: workbook data, formulas, undo/redo, filtering (no UI).
//! - [`io`]: reading and writing `.xlsx` and `.csv` files.
//! - [`ai`]: the OpenAI-compatible client, prompts and key storage.
//! - [`ui`]: the egui application.
//! - [`platform`]: OS integration (opening files from Finder).

pub mod ai;
pub mod io;
pub mod model;
pub mod platform;
pub mod ui;

pub use ui::App;
