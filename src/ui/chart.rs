//! Drawing charts with the egui painter: column, bar, line, area, pie and
//! scatter, with a title, axes, gridlines and a legend.

use std::f32::consts::{PI, TAU};

use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Shape, Stroke, Visuals, pos2, vec2};

use crate::model::{ChartData, ChartKind, fmt_number};

/// Series colours, chosen to stay distinguishable in both themes.
pub const PALETTE: [Color32; 8] = [
    Color32::from_rgb(0x4E, 0x79, 0xA7),
    Color32::from_rgb(0xF2, 0x8E, 0x2B),
    Color32::from_rgb(0x59, 0xA1, 0x4F),
    Color32::from_rgb(0xE1, 0x57, 0x59),
    Color32::from_rgb(0x76, 0xB7, 0xB2),
    Color32::from_rgb(0xED, 0xC9, 0x48),
    Color32::from_rgb(0xB0, 0x7A, 0xA1),
    Color32::from_rgb(0xFF, 0x9D, 0xA7),
];

pub fn color(i: usize) -> Color32 {
    PALETTE[i % PALETTE.len()]
}

/// Paint a whole chart (background, title, plot, legend) into `rect`.
pub fn paint(
    painter: &Painter,
    rect: Rect,
    kind: ChartKind,
    title: &str,
    data: &ChartData,
    visuals: &Visuals,
) {
    let bg = if visuals.dark_mode {
        Color32::from_gray(30)
    } else {
        Color32::WHITE
    };
    let border = if visuals.dark_mode {
        Color32::from_gray(70)
    } else {
        Color32::from_gray(200)
    };
    painter.rect(
        rect,
        6.0,
        bg,
        Stroke::new(1.0, border),
        egui::StrokeKind::Inside,
    );
    let painter = painter.with_clip_rect(rect.intersect(painter.clip_rect()));
    let text = visuals.text_color();
    let weak = visuals.weak_text_color();

    let mut area = rect.shrink(12.0);
    if !title.is_empty() {
        painter.text(
            pos2(area.center().x, area.top()),
            Align2::CENTER_TOP,
            title,
            FontId::proportional(15.0),
            text,
        );
        area.min.y += 24.0;
    }
    let entries: Vec<(String, Color32)> = if kind == ChartKind::Pie {
        data.categories
            .iter()
            .enumerate()
            .map(|(i, c)| (c.clone(), color(i)))
            .collect()
    } else {
        data.series
            .iter()
            .enumerate()
            .map(|(i, s)| (s.0.clone(), color(i)))
            .collect()
    };
    if entries.len() > 1 || kind == ChartKind::Pie {
        area.max.y -= 20.0;
        legend(
            &painter,
            pos2(area.center().x, area.bottom() + 12.0),
            &entries,
            area.width(),
            weak,
        );
    }
    if data.series.iter().all(|s| s.1.iter().all(Option::is_none)) {
        painter.text(
            area.center(),
            Align2::CENTER_CENTER,
            "No numeric data",
            FontId::proportional(13.0),
            weak,
        );
        return;
    }
    match kind {
        ChartKind::Pie => pie(&painter, area, data, bg),
        ChartKind::Bar => bars_horizontal(&painter, area, data, visuals),
        _ => xy(&painter, area, kind, data, visuals),
    }
}

fn legend(
    painter: &Painter,
    center: Pos2,
    entries: &[(String, Color32)],
    max_w: f32,
    text: Color32,
) {
    let font = FontId::proportional(11.5);
    let galleys: Vec<_> = entries
        .iter()
        .take(12)
        .map(|(name, c)| (painter.layout_no_wrap(name.clone(), font.clone(), text), *c))
        .collect();
    let total: f32 = galleys
        .iter()
        .map(|(g, _)| g.size().x + 26.0)
        .sum::<f32>()
        .min(max_w);
    let mut x = center.x - total / 2.0;
    for (g, c) in galleys {
        if x + g.size().x + 14.0 > center.x + max_w / 2.0 {
            break;
        }
        painter.rect_filled(
            Rect::from_center_size(pos2(x + 5.0, center.y), vec2(10.0, 10.0)),
            2.0,
            c,
        );
        let w = g.size().x;
        painter.galley(pos2(x + 14.0, center.y - g.size().y / 2.0), g, text);
        x += w + 26.0;
    }
}

/// A "nice" axis: (min, max, step) covering `lo..=hi` with about `n` steps.
pub fn nice_axis(lo: f64, hi: f64, n: f64) -> (f64, f64, f64) {
    let (lo, hi) = if (hi - lo).abs() < 1e-12 {
        (lo - 1.0, hi + 1.0)
    } else {
        (lo, hi)
    };
    let raw = (hi - lo) / n;
    let mag = 10f64.powf(raw.log10().floor());
    let step = [1.0, 2.0, 2.5, 5.0, 10.0]
        .iter()
        .map(|m| m * mag)
        .find(|s| *s >= raw)
        .unwrap_or(10.0 * mag);
    ((lo / step).floor() * step, (hi / step).ceil() * step, step)
}

/// Short tick labels: 1200 → 1.2K, 3500000 → 3.5M.
pub fn compact(v: f64) -> String {
    let a = v.abs();
    let (scaled, suffix) = if a >= 1e9 {
        (v / 1e9, "B")
    } else if a >= 1e6 {
        (v / 1e6, "M")
    } else if a >= 1e4 {
        (v / 1e3, "K")
    } else {
        (v, "")
    };
    let rounded = (scaled * 100.0).round() / 100.0;
    format!("{}{suffix}", fmt_number(rounded))
}

fn value_range(data: &ChartData, include_zero: bool) -> (f64, f64) {
    let vals = data
        .series
        .iter()
        .flat_map(|s| s.1.iter().flatten().copied());
    let (mut lo, mut hi) = vals.fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| {
        (a.min(v), b.max(v))
    });
    if include_zero {
        lo = lo.min(0.0);
        hi = hi.max(0.0);
    }
    (lo, hi)
}

/// Column, line, area and scatter charts: values up the y axis.
fn xy(painter: &Painter, area: Rect, kind: ChartKind, data: &ChartData, visuals: &Visuals) {
    let font = FontId::proportional(11.0);
    let weak = visuals.weak_text_color();
    let grid = if visuals.dark_mode {
        Color32::from_gray(55)
    } else {
        Color32::from_gray(225)
    };
    let scatter = kind == ChartKind::Scatter;
    let (lo, hi) = value_range(data, kind != ChartKind::Line && !scatter);
    let (y0, y1, ystep) = nice_axis(lo, hi, 5.0);

    let label_w = [y0, y1]
        .iter()
        .map(|v| compact(*v).len())
        .max()
        .unwrap_or(1) as f32
        * 6.5
        + 6.0;
    let plot = Rect::from_min_max(
        pos2(area.left() + label_w, area.top() + 4.0),
        pos2(area.right() - 4.0, area.bottom() - 18.0),
    );
    let y_of = |v: f64| plot.bottom() - ((v - y0) / (y1 - y0)) as f32 * plot.height();

    // Gridlines and y labels.
    let mut v = y0;
    while v <= y1 + ystep * 0.5 {
        let y = y_of(v);
        painter.hline(plot.x_range(), y, Stroke::new(1.0, grid));
        painter.text(
            pos2(plot.left() - 6.0, y),
            Align2::RIGHT_CENTER,
            compact(v),
            font.clone(),
            weak,
        );
        v += ystep;
    }

    let n = data
        .series
        .iter()
        .map(|s| s.1.len())
        .max()
        .unwrap_or(0)
        .max(1);
    // x positions: scatter uses numeric x values, everything else category slots.
    let (x_lo, x_hi, x_step) = if scatter {
        let xs = data.x.iter().flatten().copied();
        let (a, b) = xs.fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| {
            (a.min(v), b.max(v))
        });
        if a.is_finite() {
            nice_axis(a, b, 5.0)
        } else {
            nice_axis(1.0, n as f64, 5.0)
        }
    } else {
        (0.0, n as f64, 1.0)
    };
    let slot = plot.width() / n as f32;
    let x_of = |i: usize| -> Option<f32> {
        if scatter {
            let x = data.x.get(i).copied().flatten().unwrap_or(i as f64 + 1.0);
            Some(plot.left() + ((x - x_lo) / (x_hi - x_lo)) as f32 * plot.width())
        } else {
            Some(plot.left() + slot * (i as f32 + 0.5))
        }
    };

    // x labels.
    if scatter {
        let mut v = x_lo;
        while v <= x_hi + x_step * 0.5 {
            let x = plot.left() + ((v - x_lo) / (x_hi - x_lo)) as f32 * plot.width();
            painter.text(
                pos2(x, plot.bottom() + 4.0),
                Align2::CENTER_TOP,
                compact(v),
                font.clone(),
                weak,
            );
            v += x_step;
        }
    } else {
        let widest = data
            .categories
            .iter()
            .map(|c| c.chars().count())
            .max()
            .unwrap_or(1) as f32
            * 6.5
            + 8.0;
        let every = ((widest / slot).ceil() as usize).max(1);
        for (i, cat) in data.categories.iter().enumerate().step_by(every) {
            if let Some(x) = x_of(i) {
                let label: String = cat.chars().take(14).collect();
                painter.text(
                    pos2(x, plot.bottom() + 4.0),
                    Align2::CENTER_TOP,
                    label,
                    font.clone(),
                    weak,
                );
            }
        }
    }
    let axis = Stroke::new(1.0, weak.gamma_multiply(0.6));
    painter.hline(plot.x_range(), y_of(0.0f64.clamp(y0, y1)), axis);

    let series_n = data.series.len().max(1);
    for (si, (_, values)) in data.series.iter().enumerate() {
        let c = color(si);
        match kind {
            ChartKind::Column => {
                let group = slot * 0.8;
                let w = group / series_n as f32;
                for (i, v) in values.iter().enumerate() {
                    let Some(v) = v else { continue };
                    let x = plot.left() + slot * i as f32 + slot * 0.1 + w * si as f32;
                    let (a, b) = (y_of(*v), y_of(0.0f64.clamp(y0, y1)));
                    let bar =
                        Rect::from_min_max(pos2(x + 1.0, a.min(b)), pos2(x + w - 1.0, a.max(b)));
                    painter.rect_filled(bar, 2.0, c);
                }
            }
            ChartKind::Line | ChartKind::Area | ChartKind::Scatter => {
                let pts: Vec<Option<Pos2>> = values
                    .iter()
                    .enumerate()
                    .map(|(i, v)| Some(pos2(x_of(i)?, y_of((*v)?))))
                    .collect();
                if kind == ChartKind::Area {
                    let base = y_of(0.0f64.clamp(y0, y1));
                    // Fill under each pair of neighbouring points (a gap stays a gap).
                    for w in pts.windows(2) {
                        if let (Some(a), Some(b)) = (w[0], w[1]) {
                            let quad = vec![a, b, pos2(b.x, base), pos2(a.x, base)];
                            painter.add(Shape::convex_polygon(
                                quad,
                                c.gamma_multiply(0.35),
                                Stroke::NONE,
                            ));
                        }
                    }
                }
                if kind != ChartKind::Scatter {
                    for w in pts.windows(2) {
                        if let (Some(a), Some(b)) = (w[0], w[1]) {
                            painter.line_segment([a, b], Stroke::new(2.0, c));
                        }
                    }
                }
                let r = if kind == ChartKind::Scatter { 3.5 } else { 2.5 };
                for p in pts.iter().flatten() {
                    painter.circle_filled(*p, r, c);
                }
            }
            _ => {}
        }
    }
}

/// Horizontal bars: categories down the y axis, values along x.
fn bars_horizontal(painter: &Painter, area: Rect, data: &ChartData, visuals: &Visuals) {
    let font = FontId::proportional(11.0);
    let weak = visuals.weak_text_color();
    let grid = if visuals.dark_mode {
        Color32::from_gray(55)
    } else {
        Color32::from_gray(225)
    };
    let (lo, hi) = value_range(data, true);
    let (x0, x1, step) = nice_axis(lo, hi, 4.0);
    let label_w = data
        .categories
        .iter()
        .map(|c| c.chars().count().min(14))
        .max()
        .unwrap_or(1) as f32
        * 6.5
        + 8.0;
    let plot = Rect::from_min_max(
        pos2(area.left() + label_w, area.top() + 4.0),
        pos2(area.right() - 8.0, area.bottom() - 18.0),
    );
    let x_of = |v: f64| plot.left() + ((v - x0) / (x1 - x0)) as f32 * plot.width();
    let mut v = x0;
    while v <= x1 + step * 0.5 {
        let x = x_of(v);
        painter.vline(x, plot.y_range(), Stroke::new(1.0, grid));
        painter.text(
            pos2(x, plot.bottom() + 4.0),
            Align2::CENTER_TOP,
            compact(v),
            font.clone(),
            weak,
        );
        v += step;
    }
    let n = data
        .series
        .iter()
        .map(|s| s.1.len())
        .max()
        .unwrap_or(0)
        .max(1);
    let slot = plot.height() / n as f32;
    let series_n = data.series.len().max(1);
    let h = slot * 0.8 / series_n as f32;
    for (i, cat) in data.categories.iter().enumerate() {
        let y = plot.top() + slot * (i as f32 + 0.5);
        let label: String = cat.chars().take(14).collect();
        painter.text(
            pos2(plot.left() - 6.0, y),
            Align2::RIGHT_CENTER,
            label,
            font.clone(),
            weak,
        );
    }
    for (si, (_, values)) in data.series.iter().enumerate() {
        for (i, v) in values.iter().enumerate() {
            let Some(v) = v else { continue };
            let y = plot.top() + slot * i as f32 + slot * 0.1 + h * si as f32;
            let (a, b) = (x_of(*v), x_of(0.0f64.clamp(x0, x1)));
            painter.rect_filled(
                Rect::from_min_max(pos2(a.min(b), y + 1.0), pos2(a.max(b), y + h - 1.0)),
                2.0,
                color(si),
            );
        }
    }
}

/// A pie of the first series, with percentage labels on larger slices.
fn pie(painter: &Painter, area: Rect, data: &ChartData, bg: Color32) {
    let Some((_, values)) = data.series.first() else {
        return;
    };
    let vals: Vec<f64> = values.iter().map(|v| v.unwrap_or(0.0).max(0.0)).collect();
    let total: f64 = vals.iter().sum();
    if total <= 0.0 {
        return;
    }
    let center = area.center();
    let r = area.width().min(area.height()) / 2.0 - 4.0;
    let mut angle = -PI / 2.0;
    for (i, v) in vals.iter().enumerate() {
        let sweep = (v / total) as f32 * TAU;
        if sweep <= 0.0 {
            continue;
        }
        // Convex pieces of at most half a turn each.
        let pieces = (sweep / PI).ceil() as usize;
        for p in 0..pieces {
            let a0 = angle + sweep * p as f32 / pieces as f32;
            let a1 = angle + sweep * (p + 1) as f32 / pieces as f32;
            let steps = ((a1 - a0) / 0.05).ceil().max(2.0) as usize;
            let mut pts = vec![center];
            for s in 0..=steps {
                let a = a0 + (a1 - a0) * s as f32 / steps as f32;
                pts.push(center + vec2(a.cos(), a.sin()) * r);
            }
            // A hairline in the slice colour hides the seam between pieces.
            painter.add(Shape::convex_polygon(
                pts,
                color(i),
                Stroke::new(0.75, color(i)),
            ));
        }
        // Slice separator.
        painter.line_segment(
            [center, center + vec2(angle.cos(), angle.sin()) * r],
            Stroke::new(1.5, bg),
        );
        if sweep > 0.3 {
            let mid = angle + sweep / 2.0;
            let at = center + vec2(mid.cos(), mid.sin()) * r * 0.62;
            let pct = format!("{:.0}%", v / total * 100.0);
            painter.text(
                at,
                Align2::CENTER_CENTER,
                pct,
                FontId::proportional(12.0),
                Color32::WHITE,
            );
        }
        angle += sweep;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn axes_and_labels() {
        assert_eq!(nice_axis(0.0, 97.0, 5.0), (0.0, 100.0, 20.0));
        assert_eq!(nice_axis(3.0, 12.0, 5.0), (2.0, 12.0, 2.0));
        let (lo, hi, step) = nice_axis(-35.0, 120.0, 5.0);
        assert!(lo <= -35.0 && hi >= 120.0 && step > 0.0);
        assert_eq!(nice_axis(5.0, 5.0, 5.0).0, 4.0);
        assert_eq!(compact(1200.0), "1200");
        assert_eq!(compact(12_500.0), "12.5K");
        assert_eq!(compact(3_500_000.0), "3.5M");
        assert_eq!(compact(-0.25), "-0.25");
    }
}

#[cfg(test)]
mod render_tests {
    use super::*;
    use crate::model::ChartKind;

    /// `LITESHEET_SHOTS=<dir> cargo test chart_gallery -- --ignored`
    #[test]
    #[ignore = "writes a screenshot"]
    fn chart_gallery() {
        let data = ChartData {
            categories: ["Q1", "Q2", "Q3", "Q4", "Q5"].map(String::from).to_vec(),
            x: vec![Some(1.0), Some(2.5), Some(4.0), Some(6.0), Some(7.5)],
            series: vec![
                (
                    "North".into(),
                    vec![
                        Some(120.0),
                        Some(150.0),
                        Some(90.0),
                        Some(180.0),
                        Some(160.0),
                    ],
                ),
                (
                    "South".into(),
                    vec![Some(80.0), Some(-30.0), Some(110.0), None, Some(140.0)],
                ),
            ],
        };
        let mut h = egui_kittest::Harness::builder()
            .with_size([1200.0, 560.0])
            .with_theme(egui::Theme::Light)
            .wgpu()
            .build_ui(|ui| {
                let origin = ui.max_rect().min;
                for (i, kind) in ChartKind::ALL.iter().enumerate() {
                    let at =
                        origin + vec2((i % 3) as f32 * 395.0 + 8.0, (i / 3) as f32 * 275.0 + 8.0);
                    let rect = Rect::from_min_size(at, vec2(385.0, 265.0));
                    paint(ui.painter(), rect, *kind, kind.label(), &data, ui.visuals());
                }
            });
        h.run();
        if let Ok(dir) = std::env::var("LITESHEET_SHOTS") {
            h.render()
                .unwrap()
                .save(std::path::Path::new(&dir).join("chart-gallery.png"))
                .unwrap();
        }
    }
}
