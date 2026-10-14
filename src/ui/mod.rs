//! The egui desktop application.
//!
//! [`App`] holds all UI state; its behaviour is spread over the modules
//! below, each contributing an `impl App` block for one area of the UI.

mod ai_panel;
mod app;
mod chart;
mod chart_layer;
pub(crate) mod commands;
mod documents;
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

/// Where the "Buy me a coffee" buttons lead.
pub const SUPPORT_URL: &str = "https://buymeacoffee.com/mrabhishek";
pub const WEBSITE_URL: &str = "https://theabhishekin.github.io/litesheet/";
pub const REPO_URL: &str = "https://github.com/TheAbhishekIN/litesheet";
pub const ISSUES_URL: &str = "https://github.com/TheAbhishekIN/litesheet/issues";
pub use selection::Selection;
