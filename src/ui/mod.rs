//! The egui desktop application.
//!
//! [`App`] holds all UI state; its behaviour is spread over the modules
//! below, each contributing an `impl App` block for one area of the UI.

mod ai_panel;
mod app;
pub(crate) mod commands;
mod editing;
mod files;
mod filtering;
mod formula_bar;
mod grid;
mod keyboard;
mod menu;
mod search;
mod selection;
mod sheet_tabs;
mod status_bar;
mod toolbar;
mod util;

#[cfg(test)]
mod tests;

pub use app::App;
pub use selection::Selection;
