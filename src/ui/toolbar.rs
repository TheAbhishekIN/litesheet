//! The formatting toolbar: font emphasis, colours, alignment, borders and
//! number formats for the selection. Icons are painted, because egui's
//! bundled font lacks most of the symbols a toolbar would use.

use egui::{Color32, Pos2, Rect, Response, RichText, Sense, Stroke, Ui, Vec2, pos2, vec2};

use super::app::App;
use super::commands::BorderKind;
use crate::model::filter::column_label;
use crate::model::{Borders, Currency, Freeze, HAlign, NumFmt, PALETTE, Rgb, refs};

const BUTTON: Vec2 = vec2(28.0, 24.0);

impl App {
    pub(crate) fn toolbar(&mut self, ui: &mut Ui) {
        let style = self.cursor_style();
        let fmt = self.sheet().fmt(self.sel.cursor);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;

            // Font emphasis.
            let toggle = |ui: &mut Ui, text: RichText, on: bool, tip: &str| {
                ui.add_sized(BUTTON, egui::Button::selectable(on, text))
                    .on_hover_text(tip)
                    .clicked()
            };
            if toggle(
                ui,
                RichText::new("B").strong().size(15.0),
                style.bold,
                "Bold (⌘B)",
            ) {
                self.toggle_bold();
            }
            if toggle(
                ui,
                RichText::new("I").italics().size(15.0),
                style.italic,
                "Italic (⌘I)",
            ) {
                self.toggle_italic();
            }
            if toggle(
                ui,
                RichText::new("U").underline().size(15.0),
                style.underline,
                "Underline (⌘U)",
            ) {
                self.toggle_underline();
            }
            if toggle(
                ui,
                RichText::new("S").strikethrough().size(15.0),
                style.strike,
                "Strikethrough",
            ) {
                self.toggle_strike();
            }
            separator(ui);

            // Colours.
            let text_color = style
                .color
                .map(to_color32)
                .unwrap_or(ui.visuals().text_color());
            ui.menu_button(color_label("A", text_color), |ui| {
                if let Some(choice) = palette(ui, "Automatic") {
                    self.set_text_color(choice);
                    ui.close();
                }
            })
            .response
            .on_hover_text("Text color");
            let fill = style.fill.map(to_color32).unwrap_or(Color32::TRANSPARENT);
            ui.menu_button(color_label("Fill", fill), |ui| {
                if let Some(choice) = palette(ui, "No fill") {
                    self.set_fill(choice);
                    ui.close();
                }
            })
            .response
            .on_hover_text("Fill color");
            separator(ui);

            // Alignment.
            for (align, tip) in [
                (HAlign::Left, "Align left"),
                (HAlign::Center, "Center"),
                (HAlign::Right, "Align right"),
            ] {
                let on = style.align == align;
                if icon_button(ui, tip, on, |p, r, c| paint_align(p, r, c, align)).clicked() {
                    self.set_align(if on { HAlign::General } else { align });
                }
            }
            separator(ui);

            // Borders.
            let r = icon_button(ui, "Borders", false, |p, r, c| {
                paint_borders_icon(p, r, c, Borders::ALL, true)
            });
            egui::Popup::menu(&r).show(|ui| {
                for (kind, label, icon) in [
                    (BorderKind::All, "All Borders", Borders::ALL),
                    (BorderKind::Outline, "Outside Borders", Borders::ALL),
                    (
                        BorderKind::Top,
                        "Top Border",
                        Borders {
                            top: true,
                            ..Borders::NONE
                        },
                    ),
                    (
                        BorderKind::Bottom,
                        "Bottom Border",
                        Borders {
                            bottom: true,
                            ..Borders::NONE
                        },
                    ),
                    (
                        BorderKind::Left,
                        "Left Border",
                        Borders {
                            left: true,
                            ..Borders::NONE
                        },
                    ),
                    (
                        BorderKind::Right,
                        "Right Border",
                        Borders {
                            right: true,
                            ..Borders::NONE
                        },
                    ),
                    (BorderKind::None, "No Border", Borders::NONE),
                ] {
                    let inner = kind == BorderKind::All;
                    ui.horizontal(|ui| {
                        let (rect, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
                        paint_borders_icon(
                            ui.painter(),
                            rect,
                            ui.visuals().text_color(),
                            icon,
                            inner,
                        );
                        if ui.button(label).clicked() {
                            self.set_borders(kind);
                            ui.close();
                        }
                    });
                }
            });
            separator(ui);

            // Merge & Center.
            let merged = self.selection_has_merge();
            if icon_button(ui, "Merge & Center", merged, paint_merge_icon).clicked() {
                self.toggle_merge_center();
            }
            separator(ui);

            // Charts.
            if icon_button(ui, "Insert Chart", false, paint_chart_icon).clicked() {
                self.open_chart_dialog(None);
            }
            separator(ui);

            // Number format.
            egui::ComboBox::from_id_salt("toolbar_number_format")
                .selected_text(format_name(fmt))
                .width(110.0)
                .show_ui(ui, |ui| {
                    for (label, choice) in [
                        ("General", NumFmt::General),
                        (
                            "Number",
                            NumFmt::Number {
                                decimals: 2,
                                thousands: true,
                            },
                        ),
                        (
                            "Currency",
                            NumFmt::Currency {
                                decimals: 2,
                                symbol: Currency::Dollar,
                            },
                        ),
                        ("Percent", NumFmt::Percent { decimals: Some(0) }),
                        ("Scientific", NumFmt::Scientific { decimals: 2 }),
                        ("Date", NumFmt::Date),
                        ("Time", NumFmt::Time),
                        ("Date & Time", NumFmt::DateTime),
                        ("Text", NumFmt::Text),
                    ] {
                        let same = std::mem::discriminant(&fmt) == std::mem::discriminant(&choice);
                        if ui.selectable_label(same, label).clicked() {
                            self.set_format(choice);
                        }
                    }
                });
            if ui
                .add_sized(BUTTON, egui::Button::new(RichText::new(".0").size(12.0)))
                .on_hover_text("Decrease decimal")
                .clicked()
            {
                self.adjust_decimals(-1);
            }
            if ui
                .add_sized(BUTTON, egui::Button::new(RichText::new(".00").size(12.0)))
                .on_hover_text("Increase decimal")
                .clicked()
            {
                self.adjust_decimals(1);
            }

            // Everyday data actions, on the right.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                self.data_actions(ui);
            });
        });
    }

    /// Sort, filter, freeze, find and AI: the actions people reach for most,
    /// one click away instead of inside the menus. Laid out right to left.
    fn data_actions(&mut self, ui: &mut Ui) {
        // Drop the labels when the window is too narrow for them.
        let labels = ui.available_width() > 430.0;
        let text = |t: &'static str| if labels { t } else { "" };

        if labeled_button(
            ui,
            text("AI"),
            "AI Assistant (⌘⇧I)",
            self.ai.open,
            paint_sparkle,
        )
        .clicked()
        {
            self.ai.open = !self.ai.open;
        }
        separator(ui);

        if labeled_button(
            ui,
            text("Find"),
            "Find (⌘F)",
            self.show_search,
            paint_search,
        )
        .clicked()
        {
            if self.show_search {
                self.show_search = false;
            } else {
                self.commit_edit((0, 0));
                self.show_search = true;
                self.focus_search = true;
            }
        }

        let frozen = self.sheet().freeze;
        let r = labeled_button(
            ui,
            text("Freeze"),
            "Freeze panes",
            frozen.is_active(),
            paint_freeze,
        );
        egui::Popup::menu(&r).show(|ui| {
            let cell = refs::cell_name(self.sel.cursor.0, self.sel.cursor.1);
            if ui.button(format!("Freeze Panes at {cell}")).clicked() {
                self.freeze_at_cursor();
            }
            if ui.button("Freeze Top Row").clicked() {
                self.set_freeze(Freeze { rows: 1, cols: 0 });
            }
            if ui.button("Freeze First Column").clicked() {
                self.set_freeze(Freeze { rows: 0, cols: 1 });
            }
            if ui
                .add_enabled(frozen.is_active(), egui::Button::new("Unfreeze Panes"))
                .clicked()
            {
                self.set_freeze(Freeze::default());
            }
        });

        let filtered = self.view().has_filters();
        if filtered
            && ui
                .button(RichText::new("✕ Clear").small())
                .on_hover_text("Clear all filters")
                .clicked()
        {
            self.clear_filters();
        }
        let on = self.filter_buttons || filtered;
        if labeled_button(
            ui,
            text("Filter"),
            "Filter buttons in the column headers (⌘⇧L)",
            on,
            paint_funnel,
        )
        .clicked()
        {
            self.filter_buttons = !self.filter_buttons;
        }

        let col = self.sel.cursor.1;
        let r = labeled_button(
            ui,
            text("Sort"),
            "Sort by the selected column",
            false,
            paint_sort,
        );
        egui::Popup::menu(&r).show(|ui| {
            let label = column_label(self.sheet(), col, self.view().header_row);
            if ui.button(format!("Sort A to Z by {label}")).clicked() {
                self.sort_by(col, true);
            }
            if ui.button(format!("Sort Z to A by {label}")).clicked() {
                self.sort_by(col, false);
            }
        });
    }
}

fn format_name(fmt: NumFmt) -> String {
    match fmt {
        NumFmt::General => "General".into(),
        NumFmt::Number { .. } => "Number".into(),
        NumFmt::Currency { symbol, .. } => format!("Currency ({})", symbol.symbol()),
        NumFmt::Percent { .. } => "Percent".into(),
        NumFmt::Scientific { .. } => "Scientific".into(),
        NumFmt::Date => "Date".into(),
        NumFmt::Time => "Time".into(),
        NumFmt::DateTime => "Date & Time".into(),
        NumFmt::Text => "Text".into(),
    }
}

fn separator(ui: &mut Ui) {
    ui.add_space(4.0);
    ui.separator();
    ui.add_space(4.0);
}

fn to_color32(c: Rgb) -> Color32 {
    Color32::from_rgb(c.0, c.1, c.2)
}

/// A label with a colour bar under it, for the colour menus.
fn color_label(text: &str, color: Color32) -> RichText {
    RichText::new(text)
        .underline()
        .color(if color == Color32::TRANSPARENT {
            Color32::GRAY
        } else {
            color
        })
}

/// A grid of colour swatches. Returns `Some(None)` for the reset option.
fn palette(ui: &mut Ui, reset: &str) -> Option<Option<Rgb>> {
    let mut picked = None;
    if ui.button(reset).clicked() {
        picked = Some(None);
    }
    ui.add_space(4.0);
    egui::Grid::new(("palette", reset))
        .spacing(vec2(3.0, 3.0))
        .show(ui, |ui| {
            for row in PALETTE {
                for c in row {
                    let (rect, resp) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::click());
                    let stroke_color = if resp.hovered() {
                        ui.visuals().strong_text_color()
                    } else {
                        Color32::from_gray(140)
                    };
                    ui.painter().rect(
                        rect,
                        2.0,
                        to_color32(c),
                        Stroke::new(1.0, stroke_color),
                        egui::StrokeKind::Inside,
                    );
                    if resp.clicked() {
                        picked = Some(Some(c));
                    }
                }
                ui.end_row();
            }
        });
    ui.add_space(4.0);
    let id = ui.id().with(("custom_color", reset));
    let mut custom = ui
        .data(|d| d.get_temp::<[u8; 3]>(id))
        .unwrap_or([22, 163, 74]);
    ui.horizontal(|ui| {
        ui.label("Custom");
        if ui.color_edit_button_srgb(&mut custom).changed() {
            ui.data_mut(|d| d.insert_temp(id, custom));
        }
        if ui.button("Apply").clicked() {
            picked = Some(Some(Rgb(custom[0], custom[1], custom[2])));
        }
    });
    picked
}

/// A toolbar button whose icon is painted by `paint` in the given colour.
/// `label` names it for screen readers and shows as its tooltip.
fn icon_button(
    ui: &mut Ui,
    label: &str,
    selected: bool,
    paint: impl FnOnce(&egui::Painter, Rect, Color32),
) -> Response {
    let (rect, resp) = ui.allocate_exact_size(BUTTON, Sense::click());
    resp.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, label)
    });
    let resp = resp.on_hover_text(label);
    let visuals = ui.style().interact_selectable(&resp, selected);
    if selected || resp.hovered() {
        ui.painter().rect_filled(rect, 4.0, visuals.weak_bg_fill);
    }
    paint(
        ui.painter(),
        rect.shrink2(vec2(7.0, 6.0)),
        visuals.fg_stroke.color,
    );
    resp
}

/// A toolbar button with a painted icon and, if `text` isn't empty, a label.
fn labeled_button(
    ui: &mut Ui,
    text: &str,
    tip: &str,
    selected: bool,
    paint: impl FnOnce(&egui::Painter, Rect, Color32),
) -> Response {
    if text.is_empty() {
        return icon_button(ui, tip, selected, paint);
    }
    let galley = egui::WidgetText::from(text).into_galley(
        ui,
        Some(egui::TextWrapMode::Extend),
        f32::INFINITY,
        egui::TextStyle::Button,
    );
    let size = vec2(BUTTON.x + galley.size().x + 6.0, BUTTON.y);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, text));
    let resp = resp.on_hover_text(tip);
    let visuals = ui.style().interact_selectable(&resp, selected);
    if selected || resp.hovered() {
        ui.painter().rect_filled(rect, 4.0, visuals.weak_bg_fill);
    }
    let icon = Rect::from_min_size(rect.min, BUTTON).shrink2(vec2(7.0, 6.0));
    let color = visuals.fg_stroke.color;
    paint(ui.painter(), icon, color);
    let pos = pos2(
        rect.left() + BUTTON.x - 3.0,
        rect.center().y - galley.size().y / 2.0,
    );
    ui.painter().galley(pos, galley, color);
    resp
}

fn paint_sort(p: &egui::Painter, r: Rect, color: Color32) {
    let stroke = Stroke::new(1.5, color);
    // Down arrow on the left, up arrow on the right.
    let (x1, x2) = (r.left() + r.width() * 0.28, r.left() + r.width() * 0.72);
    p.line_segment([pos2(x1, r.top()), pos2(x1, r.bottom())], stroke);
    p.line_segment(
        [pos2(x1 - 3.0, r.bottom() - 3.0), pos2(x1, r.bottom())],
        stroke,
    );
    p.line_segment(
        [pos2(x1 + 3.0, r.bottom() - 3.0), pos2(x1, r.bottom())],
        stroke,
    );
    p.line_segment([pos2(x2, r.top()), pos2(x2, r.bottom())], stroke);
    p.line_segment([pos2(x2 - 3.0, r.top() + 3.0), pos2(x2, r.top())], stroke);
    p.line_segment([pos2(x2 + 3.0, r.top() + 3.0), pos2(x2, r.top())], stroke);
}

fn paint_funnel(p: &egui::Painter, r: Rect, color: Color32) {
    // A wide cone over a narrow stem (each shape convex).
    let c = r.center().x;
    let neck = r.top() + r.height() * 0.55;
    let cone = vec![
        pos2(r.left(), r.top()),
        pos2(r.right(), r.top()),
        pos2(c + 1.75, neck),
        pos2(c - 1.75, neck),
    ];
    p.add(egui::Shape::convex_polygon(cone, color, Stroke::NONE));
    let stem = Rect::from_min_max(pos2(c - 1.75, neck - 0.5), pos2(c + 1.75, r.bottom()));
    p.rect_filled(stem, 0.0, color);
}

fn paint_freeze(p: &egui::Painter, r: Rect, color: Color32) {
    // A grid whose first row and column are drawn heavy.
    let faint = Stroke::new(1.0, color.gamma_multiply(0.45));
    let strong = Stroke::new(2.0, color);
    p.rect_stroke(r, 1.0, faint, egui::StrokeKind::Inside);
    let (x, y) = (r.left() + r.width() * 0.38, r.top() + r.height() * 0.38);
    p.line_segment([pos2(x, r.top()), pos2(x, r.bottom())], strong);
    p.line_segment([pos2(r.left(), y), pos2(r.right(), y)], strong);
}

fn paint_search(p: &egui::Painter, r: Rect, color: Color32) {
    let stroke = Stroke::new(1.5, color);
    let radius = r.height() * 0.36;
    let c = pos2(r.left() + radius + 1.0, r.top() + radius + 0.5);
    p.circle_stroke(c, radius, stroke);
    let d = radius * std::f32::consts::FRAC_1_SQRT_2;
    p.line_segment(
        [pos2(c.x + d, c.y + d), pos2(r.right() - 1.0, r.bottom())],
        Stroke::new(2.0, color),
    );
}

fn paint_sparkle(p: &egui::Painter, r: Rect, color: Color32) {
    // A four-pointed star (four kites around the centre) with a small one beside it.
    let star = |c: Pos2, size: f32| {
        let w = size * 0.3;
        let tips = [
            vec2(0.0, -1.0),
            vec2(1.0, 0.0),
            vec2(0.0, 1.0),
            vec2(-1.0, 0.0),
        ];
        for t in tips {
            let side = vec2(-t.y, t.x);
            let kite = vec![c, c + (t + side) * w, c + t * size, c + (t - side) * w];
            p.add(egui::Shape::convex_polygon(kite, color, Stroke::NONE));
        }
    };
    star(
        pos2(r.left() + r.width() * 0.42, r.center().y + 1.0),
        r.height() * 0.55,
    );
    star(pos2(r.right() - 1.5, r.top() + 1.5), 3.0);
}

fn paint_align(p: &egui::Painter, r: Rect, color: Color32, align: HAlign) {
    let stroke = Stroke::new(1.5, color);
    let widths = [1.0, 0.6, 0.85, 0.5];
    for (i, w) in widths.iter().enumerate() {
        let y = r.top() + i as f32 * r.height() / 3.0;
        let len = r.width() * w;
        let x0 = match align {
            HAlign::Center => r.center().x - len / 2.0,
            HAlign::Right => r.right() - len,
            _ => r.left(),
        };
        p.line_segment([pos2(x0, y), pos2(x0 + len, y)], stroke);
    }
}

fn paint_borders_icon(p: &egui::Painter, r: Rect, color: Color32, b: Borders, inner: bool) {
    let faint = Stroke::new(1.0, color.gamma_multiply(0.35));
    let strong = Stroke::new(1.5, color);
    let c = r.center();
    let side = |on: bool| if on { strong } else { faint };
    let line = |a: Pos2, z: Pos2, s: Stroke| p.line_segment([a, z], s);
    line(r.left_top(), r.right_top(), side(b.top));
    line(r.left_bottom(), r.right_bottom(), side(b.bottom));
    line(r.left_top(), r.left_bottom(), side(b.left));
    line(r.right_top(), r.right_bottom(), side(b.right));
    let inner_stroke = if inner { strong } else { faint };
    line(pos2(c.x, r.top()), pos2(c.x, r.bottom()), inner_stroke);
    line(pos2(r.left(), c.y), pos2(r.right(), c.y), inner_stroke);
}

fn paint_merge_icon(p: &egui::Painter, r: Rect, color: Color32) {
    let stroke = Stroke::new(1.5, color);
    p.rect_stroke(r, 1.0, stroke, egui::StrokeKind::Inside);
    // Two arrows pointing at each other across the middle.
    let y = r.center().y;
    let (l, rr, c) = (r.left() + 2.0, r.right() - 2.0, r.center().x);
    p.line_segment([pos2(l, y), pos2(c - 1.5, y)], stroke);
    p.line_segment([pos2(c + 1.5, y), pos2(rr, y)], stroke);
    p.line_segment([pos2(c - 4.0, y - 2.5), pos2(c - 1.5, y)], stroke);
    p.line_segment([pos2(c - 4.0, y + 2.5), pos2(c - 1.5, y)], stroke);
    p.line_segment([pos2(c + 4.0, y - 2.5), pos2(c + 1.5, y)], stroke);
    p.line_segment([pos2(c + 4.0, y + 2.5), pos2(c + 1.5, y)], stroke);
}

fn paint_chart_icon(p: &egui::Painter, r: Rect, color: Color32) {
    let base = r.bottom();
    let w = r.width() / 4.0;
    for (i, h) in [0.45, 0.8, 0.6].iter().enumerate() {
        let x = r.left() + w * (i as f32 + 0.3) + i as f32 * 1.0;
        let bar = Rect::from_min_max(pos2(x, base - r.height() * h), pos2(x + w * 0.85, base));
        p.rect_filled(bar, 1.0, color);
    }
    p.line_segment(
        [pos2(r.left(), base + 0.5), pos2(r.right(), base + 0.5)],
        Stroke::new(1.0, color),
    );
}
