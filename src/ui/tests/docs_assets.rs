//! Generates the README screenshots, the sample workbook and the app icon
//! from the real app, so they never drift from what LiteSheet looks like.
//!
//! Run with `./scripts/generate-docs-assets.sh`, which calls:
//! `cargo test --release generate_docs_assets -- --ignored`

use std::path::{Path, PathBuf};

use egui::{Theme, pos2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use image::{Rgba, RgbaImage};

use super::{mock_ai_server, wait_for_ai};
use crate::ai::Task;
use crate::model::filter::{ColumnFilter, Condition, Op};
use crate::model::{Book, Cell, NumFmt, Style, interpret_input};
use crate::ui::Selection;
use crate::ui::app::{App, CELL_EDITOR_ID, EditState};

const SIZE: [f32; 2] = [1440.0, 810.0];
const PIXELS_PER_POINT: f32 = 1.5;

fn repo_path(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(rel)
}

#[test]
#[ignore = "writes files into docs/, samples/ and assets/"]
fn generate_docs_assets() {
    std::fs::create_dir_all(repo_path("docs/screenshots")).unwrap();
    std::fs::create_dir_all(repo_path("samples")).unwrap();
    std::fs::create_dir_all(repo_path("assets")).unwrap();

    icon().save(repo_path("assets/icon.png")).unwrap();
    crate::io::save(&sample_book(), 0, &repo_path("samples/sales.xlsx")).unwrap();

    hero(Theme::Light, "overview-light");
    hero(Theme::Dark, "overview-dark");
    formula_editing();
    multiple_sheets();
    filtering();
    search();
    ai_formula();
    ai_filter();
    ai_summary();
    charts();
    formatting();
}

/* ------------------------------ sample data ------------------------------ */

/// A deterministic two-sheet sales workbook: an order list with a formula
/// column, and a per-region summary built with SUMIF/COUNTIF.
pub fn sample_book() -> Book {
    let regions = ["West", "East", "North", "South"];
    let reps = [
        "Ana Ruiz",
        "Ben Okafor",
        "Chen Wei",
        "Dana Kim",
        "Eli Novak",
    ];
    let products = [
        ("Widget", "19.99"),
        ("Gadget", "120"),
        ("Sprocket", "7.25"),
        ("Gizmo", "42"),
        ("Doohickey", "3.5"),
    ];
    let statuses = ["Shipped", "Shipped", "Shipped", "Pending", "Returned"];
    let mut seed: u64 = 0x5eed;
    let mut rnd = |n: u64| {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (seed >> 33) % n
    };

    let header = [
        "Order",
        "Date",
        "Region",
        "Sales Rep",
        "Product",
        "Units",
        "Unit Price",
        "Total",
        "Status",
    ];
    let mut orders = Vec::new();
    let put = |cells: &mut Vec<_>, r: u32, c: u32, text: &str| {
        if let Some(cell) = interpret_input(text, NumFmt::General) {
            cells.push(((r, c), cell));
        }
    };
    for (c, h) in header.iter().enumerate() {
        put(&mut orders, 0, c as u32, h);
    }
    let start = chrono::NaiveDate::from_ymd_opt(2024, 1, 3).unwrap();
    for i in 0..48u32 {
        let r = i + 1;
        let (product, price) = products[rnd(5) as usize];
        let date = start + chrono::Duration::days(i as i64 * 7 + rnd(5) as i64);
        let row = [
            (1001 + i).to_string(),
            date.format("%Y-%m-%d").to_string(),
            regions[rnd(4) as usize].to_string(),
            reps[rnd(5) as usize].to_string(),
            product.to_string(),
            (1 + rnd(40)).to_string(),
            price.to_string(),
            format!("=F{}*G{}", r + 1, r + 1),
            statuses[rnd(5) as usize].to_string(),
        ];
        for (c, v) in row.iter().enumerate() {
            put(&mut orders, r, c as u32, v);
        }
    }

    let mut summary = Vec::new();
    for (c, h) in ["Region", "Revenue", "Orders", "Avg. order"]
        .iter()
        .enumerate()
    {
        put(&mut summary, 0, c as u32, h);
    }
    for (i, region) in regions.iter().enumerate() {
        let r = i as u32 + 1;
        let n = r + 1;
        put(&mut summary, r, 0, region);
        put(
            &mut summary,
            r,
            1,
            &format!("=SUMIF(Orders!C$2:C$49,A{n},Orders!H$2:H$49)"),
        );
        put(
            &mut summary,
            r,
            2,
            &format!("=COUNTIF(Orders!C$2:C$49,A{n})"),
        );
        put(&mut summary, r, 3, &format!("=ROUND(B{n}/C{n},2)"));
    }
    put(&mut summary, 5, 0, "Total");
    put(&mut summary, 5, 1, "=SUM(B2:B5)");
    put(&mut summary, 5, 2, "=SUM(C2:C5)");
    put(&mut summary, 5, 3, "=ROUND(B6/C6,2)");
    put(&mut summary, 7, 0, "Best region");
    put(
        &mut summary,
        7,
        1,
        "=INDEX(A2:A5,MATCH(MAX(B2:B5),B2:B5,0))",
    );

    let mut book = Book::from_sheets(vec![
        ("Orders".to_string(), orders),
        ("Summary".to_string(), summary),
    ]);
    for (c, w) in [70.0, 96.0, 76.0, 110.0, 100.0, 60.0, 86.0, 96.0, 86.0]
        .iter()
        .enumerate()
    {
        book.sheets[0].col_widths.insert(c as u32, *w);
    }
    for (c, w) in [110.0, 110.0, 80.0, 100.0].iter().enumerate() {
        book.sheets[1].col_widths.insert(c as u32, *w);
    }
    style_sample(&mut book);
    book.dirty = false;
    book.path = Some(PathBuf::from("sales.xlsx"));
    book
}

/// Give the sample the finish a real workbook would have: styled, frozen
/// headers, currency columns, a highlighted total and a chart.
fn style_sample(book: &mut Book) {
    use crate::model::{Borders, Chart, ChartKind, Currency, Freeze, HAlign, Rgb};
    let header = Style {
        bold: true,
        color: Some(Rgb::WHITE),
        fill: Some(Rgb(21, 128, 61)),
        align: HAlign::Center,
        ..Style::default()
    };
    let dollars = NumFmt::Currency {
        decimals: 2,
        symbol: Currency::Dollar,
    };
    let restyle =
        |book: &mut Book, sheet: usize, positions: Vec<(u32, u32)>, f: &dyn Fn(&mut Cell)| {
            let edits = positions
                .into_iter()
                .map(|p| {
                    let mut cell = book.sheets[sheet]
                        .cells
                        .get(&p)
                        .cloned()
                        .unwrap_or_default();
                    f(&mut cell);
                    (sheet, p, Some(cell))
                })
                .collect();
            book.apply("style", edits);
        };
    restyle(book, 0, (0..9).map(|c| (0, c)).collect(), &|c| {
        c.style = header
    });
    restyle(
        book,
        0,
        (1..49).flat_map(|r| [(r, 6), (r, 7)]).collect(),
        &|c| c.fmt = dollars,
    );
    restyle(book, 1, (0..4).map(|c| (0, c)).collect(), &|c| {
        c.style = header
    });
    restyle(
        book,
        1,
        (1..6).flat_map(|r| [(r, 1), (r, 3)]).collect(),
        &|c| c.fmt = dollars,
    );
    restyle(book, 1, (0..4).map(|c| (5, c)).collect(), &|c| {
        c.style.bold = true;
        c.style.borders = Borders {
            top: true,
            ..Borders::NONE
        };
    });
    book.set_freeze(0, Freeze { rows: 1, cols: 0 });
    let mut chart = Chart::from_block(&book.sheets[1], (0, 0), (4, 1), ChartKind::Column).unwrap();
    chart.title = "Revenue by region".into();
    chart.anchor = (0, 5);
    chart.size = (440.0, 280.0);
    book.set_charts(1, vec![chart], "chart");
}

/* ------------------------------ screenshots ------------------------------ */

fn docs_harness(theme: Theme) -> Harness<'static, App> {
    let book = sample_book();
    let mut h = Harness::builder()
        .with_size(SIZE)
        .with_pixels_per_point(PIXELS_PER_POINT)
        .with_theme(theme)
        .with_os(egui::os::OperatingSystem::Mac)
        .wgpu()
        .build_eframe(move |cc| {
            let mut app = App::new(cc, None);
            app.set_book(book);
            app
        });
    h.run_steps(3);
    h
}

fn save(h: &mut Harness<'_, App>, name: &str) {
    // Keep the mouse pointer out of the picture.
    h.event(egui::Event::PointerGone);
    h.run_steps(3);
    let img = h.render().expect("render");
    img.save(repo_path(&format!("docs/screenshots/{name}.png")))
        .unwrap();
}

fn select(h: &mut Harness<'_, App>, anchor: (u32, u32), cursor: (u32, u32)) {
    h.state_mut().sel = Selection { anchor, cursor };
}

fn hero(theme: Theme, name: &str) {
    let mut h = docs_harness(theme);
    select(&mut h, (1, 7), (14, 7));
    save(&mut h, name);
}

fn formula_editing() {
    let mut h = docs_harness(Theme::Light);
    let prefix = "=SUMIF(C2:C49,\"West\",";
    let buf = format!("{prefix}H2:H18");
    h.state_mut().sel = Selection::single((1, 10));
    h.state_mut().edit = Some(EditState {
        pos: (1, 10),
        buf,
        in_bar: false,
        place_cursor: true,
        pointing: Some(((1, 7), prefix.len())),
    });
    h.ctx
        .memory_mut(|m| m.request_focus(egui::Id::new(CELL_EDITOR_ID)));
    save(&mut h, "formula-editing");
}

fn multiple_sheets() {
    let mut h = docs_harness(Theme::Light);
    h.state_mut().switch_sheet(1);
    h.state_mut().sel = Selection::single((1, 1));
    save(&mut h, "multiple-sheets");
}

fn filtering() {
    let mut h = docs_harness(Theme::Light);
    h.state_mut().filter_buttons = true;
    let allowed = ["West", "East"].iter().map(|s| s.to_string()).collect();
    h.state_mut().set_column_filter(
        2,
        Some(ColumnFilter {
            conditions: vec![],
            allowed: Some(allowed),
        }),
    );
    h.state_mut().set_column_filter(
        7,
        Some(ColumnFilter {
            conditions: vec![Condition {
                op: Op::Gt,
                value: "200".into(),
            }],
            allowed: None,
        }),
    );
    h.state_mut().open_filter_popup(2);
    if let Some(p) = h.state_mut().filter_popup.as_mut() {
        p.anchor = Some(pos2(196.0, 122.0));
    }
    save(&mut h, "filtering");
}

fn search() {
    let mut h = docs_harness(Theme::Light);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::F);
    h.run_steps(2);
    h.event(egui::Event::Text("gadget".into()));
    h.run_steps(2);
    let matches = h.state().view().matches.clone();
    h.state_mut().sel = Selection::single(matches[2]);
    h.state_mut().views[0].current_match = 2;
    save(&mut h, "search");
}

fn charts() {
    let mut h = docs_harness(Theme::Light);
    h.state_mut().switch_sheet(1);
    h.state_mut().selected_chart = Some(0);
    h.state_mut().sel = Selection::single((1, 1));
    save(&mut h, "charts");
}

fn formatting() {
    let mut h = docs_harness(Theme::Dark);
    h.state_mut().sel = Selection {
        anchor: (1, 6),
        cursor: (12, 7),
    };
    h.get_by_label_contains("Fill").click();
    save(&mut h, "formatting");
}

fn ai_harness(theme: Theme, task: Task, prompt: &str) -> Harness<'static, App> {
    let (url, _) = mock_ai_server(|body| {
        Some(if body.contains("natural-language filter") {
            r#"{"filters":[{"column":"Region","op":"eq","value":"West"},{"column":"Total","op":"gt","value":"300"}]}"#.into()
        } else if body.contains("Excel formulas") {
            "=SUMIFS(H2:H49,C2:C49,\"West\",I2:I49,\"Shipped\")".into()
        } else {
            "This sheet lists 48 product orders placed between January and December 2024, \
             with the units, unit price and order total for each.\n\n\
             • Orders are spread across four regions and five sales reps.\n\
             • Most orders have shipped; the rest are pending or were returned.\n\
             • Order totals vary widely, from under $20 to several thousand dollars.\n\
             • Gadget has the highest unit price, so its orders are among the largest.\n\
             • Every row has a value in every column."
                .into()
        })
    });
    let mut h = docs_harness(theme);
    h.state_mut().ai.use_test_backend(&url);
    h.state_mut().ai.open = true;
    h.state_mut().ai.task = task;
    h.state_mut().ai.prompt = prompt.to_string();
    h.run_steps(2);
    h
}

fn ai_formula() {
    let mut h = ai_harness(
        Theme::Dark,
        Task::Formula,
        "Total revenue from shipped West region orders",
    );
    h.state_mut().sel = Selection::single((1, 10));
    h.run_steps(2);
    h.get_by_label("Generate").click();
    wait_for_ai(&mut h);
    h.get_by_label("Insert into K2").click();
    save(&mut h, "ai-formula");
}

fn ai_filter() {
    let mut h = ai_harness(Theme::Dark, Task::Filter, "West region orders over $300");
    h.get_by_label("Apply Filter").click();
    wait_for_ai(&mut h);
    save(&mut h, "ai-filter");
}

fn ai_summary() {
    let mut h = ai_harness(Theme::Light, Task::Summarize, "");
    h.get_by_label("Summarize Sheet").click();
    wait_for_ai(&mut h);
    save(&mut h, "ai-summary");
}

/* --------------------------------- icon --------------------------------- */

/// The app icon: a green rounded square (macOS icon grid) holding a small
/// white spreadsheet with one highlighted cell. Drawn with signed distance
/// fields so the edges are anti-aliased.
fn icon() -> RgbaImage {
    const N: u32 = 1024;
    let mut img = RgbaImage::from_pixel(N, N, Rgba([0, 0, 0, 0]));

    let body = (100.0, 100.0, 924.0, 924.0, 185.0);
    let card = (230.0, 268.0, 794.0, 780.0, 44.0);
    let cols = [230.0, 378.0, 586.0, 794.0];
    let rows = [268.0, 372.0, 508.0, 644.0, 780.0];
    let selected = (cols[1], rows[2], cols[2], rows[3]);

    for y in 0..N {
        for x in 0..N {
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            let mut c = [0.0f32; 4];

            // Body with a vertical gradient.
            let a = coverage(round_rect_sdf(px, py, body));
            if a > 0.0 {
                let t = (py - body.1) / (body.3 - body.1);
                let top = [52.0, 211.0, 113.0];
                let bottom = [21.0, 128.0, 61.0];
                let rgb = [0, 1, 2].map(|i| top[i] + (bottom[i] - top[i]) * t);
                blend(&mut c, rgb, a);

                // Soft shadow under the card.
                let d = round_rect_sdf(px, py - 10.0, card);
                if d > 0.0 {
                    let shadow = (1.0 - d / 40.0).clamp(0.0, 1.0).powi(2) * 0.18;
                    blend(&mut c, [8.0, 60.0, 30.0], shadow * a);
                }
            }

            // White card.
            let ca = coverage(round_rect_sdf(px, py, card));
            if ca > 0.0 {
                blend(&mut c, [255.0, 255.0, 255.0], ca);
                // Header row and first column tint.
                if py < rows[1] || px < cols[1] {
                    blend(&mut c, [220.0, 245.0, 228.0], ca);
                }
                // Selected cell.
                let (x0, y0, x1, y1) = selected;
                if px > x0 && px < x1 && py > y0 && py < y1 {
                    blend(&mut c, [187.0, 240.0, 206.0], ca);
                }
                // Grid lines.
                let line = 5.0;
                let near = |v: f32, edges: &[f32]| {
                    edges[1..edges.len() - 1]
                        .iter()
                        .any(|e| (v - e).abs() < line / 2.0)
                };
                if near(px, &cols) || near(py, &rows) {
                    blend(&mut c, [196.0, 214.0, 202.0], ca);
                }
                // Selection outline.
                let sel_sdf = rect_sdf(px, py, selected).abs();
                let outline = coverage(sel_sdf - 7.0);
                blend(&mut c, [22.0, 163.0, 74.0], outline * ca);
            }

            if c[3] > 0.0 {
                img.put_pixel(
                    x,
                    y,
                    Rgba([
                        (c[0] / c[3]).round() as u8,
                        (c[1] / c[3]).round() as u8,
                        (c[2] / c[3]).round() as u8,
                        (c[3] * 255.0).round() as u8,
                    ]),
                );
            }
        }
    }
    img
}

/// Premultiplied "over" compositing into `c` (rgb premultiplied, alpha 0..1).
fn blend(c: &mut [f32; 4], rgb: [f32; 3], a: f32) {
    for i in 0..3 {
        c[i] = rgb[i] * a + c[i] * (1.0 - a);
    }
    c[3] = a + c[3] * (1.0 - a);
}

fn coverage(sdf: f32) -> f32 {
    (0.5 - sdf).clamp(0.0, 1.0)
}

fn rect_sdf(px: f32, py: f32, (x0, y0, x1, y1): (f32, f32, f32, f32)) -> f32 {
    let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    let (hx, hy) = ((x1 - x0) / 2.0, (y1 - y0) / 2.0);
    let (dx, dy) = ((px - cx).abs() - hx, (py - cy).abs() - hy);
    let outside = (dx.max(0.0).powi(2) + dy.max(0.0).powi(2)).sqrt();
    outside + dx.max(dy).min(0.0)
}

fn round_rect_sdf(px: f32, py: f32, (x0, y0, x1, y1, r): (f32, f32, f32, f32, f32)) -> f32 {
    rect_sdf(px, py, (x0 + r, y0 + r, x1 - r, y1 - r)) - r
}

#[test]
fn sample_book_calculates() {
    let b = sample_book();
    let summary = &b.sheets[1];
    let total: f64 = (1..=4)
        .map(|r| summary.value((r, 1)).as_number().unwrap())
        .sum();
    assert_eq!(summary.value((5, 1)).as_number(), Some(total));
    assert_eq!(summary.display((5, 2)), "48");
    assert!(!summary.display((7, 1)).is_empty());
}
