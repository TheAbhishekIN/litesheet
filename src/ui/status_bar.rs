//! The status bar: messages, filter summary and selection statistics.

use std::time::Duration;

use egui::RichText;

use super::app::App;
use crate::model::{self, Pos, Value};

impl App {
    pub(crate) fn status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if let Some((msg, at, is_err)) = &self.status
                && at.elapsed() < Duration::from_secs(6)
            {
                let text = RichText::new(msg.as_str());
                ui.label(if *is_err {
                    text.color(ui.visuals().error_fg_color)
                } else {
                    text
                });
                ui.ctx().request_repaint_after(Duration::from_secs(1));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let stats = self.selection_stats();
                ui.label(stats);
                let v = self.view();
                if let Some(rows) = &v.rows {
                    let total = self.sheet().n_rows.saturating_sub(v.header_row as u32);
                    let shown = rows.len().saturating_sub(v.header_row as usize);
                    ui.label(RichText::new(format!("{shown} of {total} rows")).strong());
                    if ui.small_button("Clear filters").clicked() {
                        self.clear_filters();
                        self.views[self.active].search_only_matching = false;
                        self.data_changed();
                    }
                }
            });
        });
    }

    pub(crate) fn selection_stats(&mut self) -> String {
        let key = (self.active, self.sel, self.data_version);
        if let Some((k, s)) = &self.stats_cache
            && *k == key
        {
            return s.clone();
        }
        let s = self.sheet();
        let (mut count, mut nums, mut sum) = (0usize, 0usize, 0.0);
        let mut visit = |pos: &Pos| {
            match s.value(*pos) {
                Value::Empty => return,
                Value::Number(n) => {
                    nums += 1;
                    sum += n;
                }
                _ => {}
            }
            count += 1;
        };
        let ((r0, c0), (r1, c1)) = self.sel.range();
        if (r1 - r0 + 1) as u64 * (c1 - c0 + 1) as u64 > 50_000 {
            s.cells
                .keys()
                .filter(|p| self.sel.contains(**p))
                .for_each(&mut visit);
        } else {
            for r in r0..=r1 {
                for c in c0..=c1 {
                    visit(&(r, c));
                }
            }
        }
        let text = if self.sel.is_single() || count == 0 {
            String::new()
        } else if nums > 0 {
            format!(
                "Sum: {}   Average: {}   Count: {}",
                model::fmt_number(sum),
                model::fmt_number(sum / nums as f64),
                count
            )
        } else {
            format!("Count: {count}")
        };
        self.stats_cache = Some((key, text.clone()));
        text
    }
}
