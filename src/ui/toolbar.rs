//! The formatting toolbar: font emphasis, colours, alignment, borders and
//! number formats for the selection. Icons are painted, because egui's
//! bundled font lacks most of the symbols a toolbar would use.

use egui::{Color32, Pos2, Rect, Response, RichText, Sense, Stroke, Ui, Vec2, pos2, vec2};

use super::app::App;
use super::commands::BorderKind;
use crate::model::{Borders, Currency, HAlign, NumFmt, PALETTE, Rgb};

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
