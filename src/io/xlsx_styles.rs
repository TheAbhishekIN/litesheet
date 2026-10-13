//! Reads what calamine doesn't from an `.xlsx` package: cell styles, number
//! formats, column widths, merged cells, frozen panes and charts. It walks
//! `styles.xml`, the theme, each worksheet and its drawings and charts.

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

use anyhow::{Context, Result};
use quick_xml::events::{BytesStart, Event};
use quick_xml::{Reader, XmlVersion};

use crate::model::{
    Borders, Chart, ChartKind, Currency, DEFAULT_CHART_SIZE, DataRef, HAlign, NumFmt, Pos, Rgb,
    Series, Style,
};

/// Formatting found on one worksheet.
#[derive(Debug, Default)]
pub struct SheetFormatting {
    /// Cells whose number format or style isn't the default.
    pub cells: Vec<(Pos, NumFmt, Style)>,
    /// Column widths in Excel character units.
    pub col_widths: Vec<(u32, f64)>,
    /// Merged ranges.
    pub merges: Vec<(Pos, Pos)>,
    /// Frozen (rows, columns), if the sheet has frozen panes.
    pub freeze: Option<(u32, u32)>,
    pub charts: Vec<Chart>,
}

/// Formatting for every sheet, keyed by sheet name.
pub fn read(path: &Path) -> Result<HashMap<String, SheetFormatting>> {
    let file =
        std::fs::File::open(path).with_context(|| format!("Couldn't open {}", path.display()))?;
    let mut zip = zip::ZipArchive::new(file).context("Not a valid .xlsx file")?;
    let mut part = |name: &str| -> Option<String> {
        let mut s = String::new();
        zip.by_name(name).ok()?.read_to_string(&mut s).ok()?;
        Some(s)
    };

    let theme = part("xl/theme/theme1.xml")
        .map(|x| parse_theme(&x))
        .unwrap_or_else(default_theme);
    let styles = part("xl/styles.xml")
        .map(|x| parse_styles(&x, &theme))
        .unwrap_or_default();
    let workbook = part("xl/workbook.xml").unwrap_or_default();
    let rels = part("xl/_rels/workbook.xml.rels").unwrap_or_default();
    let targets = parse_relationships(&rels);

    let mut out = HashMap::new();
    for (name, rel_id) in parse_sheet_list(&workbook) {
        let Some(target) = targets.get(&rel_id) else {
            continue;
        };
        let path = match target.strip_prefix('/') {
            Some(abs) => abs.to_string(),
            None => format!("xl/{target}"),
        };
        if let Some(xml) = part(&path) {
            let mut f = parse_worksheet(&xml, &styles);
            f.charts = read_charts(&mut part, &path, &f.col_widths);
            out.insert(name, f);
        }
    }
    Ok(out)
}

/* --------------------------------- XML helpers --------------------------------- */

fn attr(e: &BytesStart, name: &str) -> Option<String> {
    e.attributes()
        .flatten()
        .find(|a| a.key.local_name().as_ref() == name)
        .and_then(|a| {
            a.normalized_value(XmlVersion::Implicit1_0)
                .ok()
                .map(|v| v.into_owned())
        })
}

fn flag(e: &BytesStart) -> bool {
    // <b/>, <i/> etc. are on unless val="0"/"false".
    !matches!(attr(e, "val").as_deref(), Some("0" | "false"))
}

/// Visit every start/empty element as (local name, element, is_empty).
fn for_each_element(xml: &str, mut f: impl FnMut(&str, &BytesStart, bool)) {
    let mut reader = Reader::from_str(xml);
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => f(e.local_name().as_ref(), &e, false),
            Ok(Event::Empty(e)) => f(e.local_name().as_ref(), &e, true),
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
}

/* ------------------------------ workbook structure ------------------------------ */

fn parse_sheet_list(xml: &str) -> Vec<(String, String)> {
    let mut sheets = Vec::new();
    for_each_element(xml, |name, e, _| {
        if name == "sheet"
            && let (Some(n), Some(id)) = (attr(e, "name"), attr(e, "id"))
        {
            sheets.push((n, id));
        }
    });
    sheets
}

fn parse_relationships(xml: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for_each_element(xml, |name, e, _| {
        if name == "Relationship"
            && let (Some(id), Some(target)) = (attr(e, "Id"), attr(e, "Target"))
        {
            map.insert(id, target);
        }
    });
    map
}

/* ---------------------------------- colours ---------------------------------- */

/// Theme colours in the order spreadsheet XML indexes them: lt1, dk1, lt2,
/// dk2, accent1–6, hlink, folHlink. (The theme file lists dk before lt.)
type Theme = Vec<Rgb>;

fn parse_theme(xml: &str) -> Theme {
    let names: [&str; 12] = [
        "dk1", "lt1", "dk2", "lt2", "accent1", "accent2", "accent3", "accent4", "accent5",
        "accent6", "hlink", "folHlink",
    ];
    let mut found: HashMap<usize, Rgb> = HashMap::new();
    let mut current: Option<usize> = None;
    for_each_element(xml, |name, e, _| {
        if let Some(i) = names.iter().position(|n| *n == name) {
            current = Some(i);
        } else if let Some(i) = current {
            let hex = match name {
                "srgbClr" => attr(e, "val"),
                "sysClr" => attr(e, "lastClr"),
                _ => None,
            };
            if let Some(rgb) = hex.and_then(|h| Rgb::from_hex(&h)) {
                found.entry(i).or_insert(rgb);
            }
        }
    });
    let defaults = default_theme_in_file_order();
    let file_order: Vec<Rgb> = (0..12)
        .map(|i| found.get(&i).copied().unwrap_or(defaults[i]))
        .collect();
    to_index_order(file_order)
}

fn to_index_order(mut c: Vec<Rgb>) -> Theme {
    c.swap(0, 1);
    c.swap(2, 3);
    c
}

/// Office theme colours in theme-file order (dk1, lt1, dk2, lt2, accents…).
fn default_theme_in_file_order() -> Vec<Rgb> {
    [
        "000000", "FFFFFF", "44546A", "E7E6E6", "4472C4", "ED7D31", "A5A5A5", "FFC000", "5B9BD5",
        "70AD47", "0563C1", "954F72",
    ]
    .iter()
    .map(|h| Rgb::from_hex(h).unwrap())
    .collect()
}

fn default_theme() -> Theme {
    to_index_order(default_theme_in_file_order())
}

/// The legacy indexed palette (indexes 0–63).
const INDEXED: [u32; 64] = [
    0x000000, 0xFFFFFF, 0xFF0000, 0x00FF00, 0x0000FF, 0xFFFF00, 0xFF00FF, 0x00FFFF, 0x000000,
    0xFFFFFF, 0xFF0000, 0x00FF00, 0x0000FF, 0xFFFF00, 0xFF00FF, 0x00FFFF, 0x800000, 0x008000,
    0x000080, 0x808000, 0x800080, 0x008080, 0xC0C0C0, 0x808080, 0x9999FF, 0x993366, 0xFFFFCC,
    0xCCFFFF, 0x660066, 0xFF8080, 0x0066CC, 0xCCCCFF, 0x000080, 0xFF00FF, 0xFFFF00, 0x00FFFF,
    0x800080, 0x800000, 0x008080, 0x0000FF, 0x00CCFF, 0xCCFFFF, 0xCCFFCC, 0xFFFF99, 0x99CCFF,
    0xFF99CC, 0xCC99FF, 0xFFCC99, 0x3366FF, 0x33CCCC, 0x99CC00, 0xFFCC00, 0xFF9900, 0xFF6600,
    0x666699, 0x969696, 0x003366, 0x339966, 0x003300, 0x333300, 0x993300, 0x993366, 0x333399,
    0x333333,
];

/// A `<color …/>`-style element: rgb, theme (+tint) or indexed. `None` means automatic.
fn parse_color(e: &BytesStart, theme: &Theme) -> Option<Rgb> {
    if let Some(rgb) = attr(e, "rgb") {
        return Rgb::from_hex(&rgb);
    }
    let tint: f32 = attr(e, "tint").and_then(|t| t.parse().ok()).unwrap_or(0.0);
    if let Some(i) = attr(e, "theme").and_then(|t| t.parse::<usize>().ok()) {
        return theme.get(i).map(|c| c.tinted(tint));
    }
    if let Some(i) = attr(e, "indexed").and_then(|t| t.parse::<usize>().ok()) {
        return INDEXED
            .get(i)
            .map(|&v| Rgb((v >> 16) as u8, (v >> 8) as u8, v as u8));
    }
    None
}

/* ----------------------------------- styles ----------------------------------- */

#[derive(Clone, Copy, Default)]
struct Font {
    bold: bool,
    italic: bool,
    underline: bool,
    strike: bool,
    color: Option<Rgb>,
}

#[derive(Default)]
struct Styles {
    /// One entry per `<xf>` in `<cellXfs>`, indexed by a cell's `s` attribute.
    xfs: Vec<(NumFmt, Style)>,
}

fn parse_styles(xml: &str, theme: &Theme) -> Styles {
    let mut num_fmts: HashMap<u32, String> = HashMap::new();
    let mut fonts: Vec<Font> = Vec::new();
    let mut fills: Vec<Option<Rgb>> = Vec::new();
    let mut borders: Vec<Borders> = Vec::new();
    let mut xfs: Vec<(NumFmt, Style)> = Vec::new();

    #[derive(PartialEq)]
    enum Section {
        None,
        Fonts,
        Fills,
        Borders,
        CellXfs,
    }
    let mut section = Section::None;
    let mut fill_solid = false;

    let mut reader = Reader::from_str(xml);
    loop {
        let (e, empty) = match reader.read_event() {
            Ok(Event::Start(e)) => (e, false),
            Ok(Event::Empty(e)) => (e, true),
            Ok(Event::End(e)) => {
                match e.local_name().as_ref() {
                    "fonts" | "fills" | "borders" | "cellXfs" => section = Section::None,
                    _ => {}
                }
                continue;
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => continue,
        };
        let name = e.local_name();
        match (name.as_ref(), &section) {
            ("numFmt", _) => {
                if let (Some(id), Some(code)) = (
                    attr(&e, "numFmtId").and_then(|v| v.parse().ok()),
                    attr(&e, "formatCode"),
                ) {
                    num_fmts.insert(id, code);
                }
            }
            ("fonts", _) if !empty => section = Section::Fonts,
            ("fills", _) if !empty => section = Section::Fills,
            ("borders", _) if !empty => section = Section::Borders,
            ("cellXfs", _) if !empty => section = Section::CellXfs,

            ("font", Section::Fonts) => fonts.push(Font::default()),
            ("b", Section::Fonts) => fonts.last_mut().into_iter().for_each(|f| f.bold = flag(&e)),
            ("i", Section::Fonts) => fonts
                .last_mut()
                .into_iter()
                .for_each(|f| f.italic = flag(&e)),
            ("strike", Section::Fonts) => fonts
                .last_mut()
                .into_iter()
                .for_each(|f| f.strike = flag(&e)),
            ("u", Section::Fonts) => fonts
                .last_mut()
                .into_iter()
                .for_each(|f| f.underline = attr(&e, "val").as_deref() != Some("none")),
            ("color", Section::Fonts) => {
                let c = parse_color(&e, theme);
                if let Some(f) = fonts.last_mut() {
                    f.color = c;
                }
            }

            ("fill", Section::Fills) => {
                fills.push(None);
                fill_solid = false;
            }
            ("patternFill", Section::Fills) => {
                fill_solid = attr(&e, "patternType").as_deref() == Some("solid");
            }
            ("fgColor", Section::Fills) if fill_solid => {
                let c = parse_color(&e, theme);
                if let Some(f) = fills.last_mut() {
                    *f = c;
                }
            }

            ("border", Section::Borders) => borders.push(Borders::NONE),
            (side @ ("left" | "right" | "top" | "bottom"), Section::Borders) => {
                let on = attr(&e, "style").is_some_and(|s| s != "none");
                let idx = match side {
                    "left" => 0,
                    "right" => 1,
                    "top" => 2,
                    _ => 3,
                };
                if let Some(b) = borders.last_mut() {
                    match idx {
                        0 => b.left = on,
                        1 => b.right = on,
                        2 => b.top = on,
                        _ => b.bottom = on,
                    }
                }
            }

            ("xf", Section::CellXfs) => {
                let num = |a: &str| {
                    attr(&e, a)
                        .and_then(|v| v.parse::<usize>().ok())
                        .unwrap_or(0)
                };
                let fmt_id = num("numFmtId") as u32;
                let code = num_fmts
                    .get(&fmt_id)
                    .cloned()
                    .or_else(|| builtin_format(fmt_id).map(String::from));
                let fmt = code.map(|c| numfmt_from_code(&c)).unwrap_or_default();
                let font = fonts.get(num("fontId")).copied().unwrap_or_default();
                let style = Style {
                    bold: font.bold,
                    italic: font.italic,
                    underline: font.underline,
                    strike: font.strike,
                    // Black text and white fills are what "automatic" looks like in
                    // a light theme; leave them unset so dark mode stays readable.
                    color: font.color.filter(|c| *c != Rgb::BLACK),
                    fill: fills
                        .get(num("fillId"))
                        .copied()
                        .flatten()
                        .filter(|c| *c != Rgb::WHITE),
                    align: HAlign::General,
                    borders: borders.get(num("borderId")).copied().unwrap_or_default(),
                };
                xfs.push((fmt, style));
            }
            ("alignment", Section::CellXfs) => {
                let align = match attr(&e, "horizontal").as_deref() {
                    Some("left") => HAlign::Left,
                    Some("center" | "centerContinuous") => HAlign::Center,
                    Some("right") => HAlign::Right,
                    _ => HAlign::General,
                };
                if let Some(x) = xfs.last_mut() {
                    x.1.align = align;
                }
            }
            _ => {}
        }
    }
    Styles { xfs }
}

fn builtin_format(id: u32) -> Option<&'static str> {
    Some(match id {
        0 => "General",
        1 => "0",
        2 => "0.00",
        3 => "#,##0",
        4 => "#,##0.00",
        5 | 37 => "#,##0 ;(#,##0)",
        6 | 38 => "#,##0 ;[Red](#,##0)",
        7 | 39 => "#,##0.00;(#,##0.00)",
        8 | 40 => "#,##0.00;[Red](#,##0.00)",
        9 => "0%",
        10 => "0.00%",
        11 => "0.00E+00",
        14 => "yyyy-mm-dd",
        15 => "d-mmm-yy",
        16 => "d-mmm",
        17 => "mmm-yy",
        18 => "h:mm AM/PM",
        19 => "h:mm:ss AM/PM",
        20 => "h:mm",
        21 => "h:mm:ss",
        22 => "m/d/yy h:mm",
        44 => "\"$\"#,##0.00",
        45 => "mm:ss",
        46 => "[h]:mm:ss",
        48 => "##0.0E+0",
        49 => "@",
        _ => return None,
    })
}

/// Map an Excel number-format code onto LiteSheet's formats.
pub fn numfmt_from_code(code: &str) -> NumFmt {
    let section = code.split(';').next().unwrap_or("");
    if section.eq_ignore_ascii_case("general") || section.is_empty() {
        return NumFmt::General;
    }
    // Currency symbols may sit in quotes ("€") or a locale tag ([$€-407]);
    // collect them, then strip quoted text and brackets for the rest.
    let mut symbol = None;
    let mut plain = String::new();
    let mut chars = section.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                for q in chars.by_ref() {
                    if q == '"' {
                        break;
                    }
                    symbol = symbol.or_else(|| currency_symbol(q));
                }
            }
            '[' => {
                let mut inner = String::new();
                for q in chars.by_ref() {
                    if q == ']' {
                        break;
                    }
                    inner.push(q);
                }
                if let Some(rest) = inner.strip_prefix('$') {
                    symbol = symbol.or_else(|| rest.chars().find_map(currency_symbol));
                }
                if inner.to_ascii_lowercase().starts_with('h')
                    || inner.to_ascii_lowercase().starts_with('m')
                {
                    plain.push('h'); // elapsed time like [h]:mm
                }
            }
            '\\' => {
                if let Some(q) = chars.next() {
                    symbol = symbol.or_else(|| currency_symbol(q));
                }
            }
            '_' | '*' => {
                chars.next(); // padding / fill character
            }
            c => {
                symbol = symbol.or_else(|| currency_symbol(c));
                plain.push(c);
            }
        }
    }
    let lower = plain.to_ascii_lowercase();
    if lower.contains('@') {
        return NumFmt::Text;
    }
    let has_date = lower.contains('y') || lower.contains('d');
    let has_time = lower.contains('h') || lower.contains('s');
    if has_date && has_time {
        return NumFmt::DateTime;
    }
    if has_date || (lower.contains('m') && !has_time && !lower.contains('0')) {
        return NumFmt::Date;
    }
    if has_time {
        return NumFmt::Time;
    }
    let decimals = plain.split_once('.').map_or(0, |(_, d)| {
        d.chars().take_while(|c| matches!(c, '0' | '#')).count() as u8
    });
    if lower.contains("e+") || lower.contains("e-") {
        return NumFmt::Scientific { decimals };
    }
    if plain.contains('%') {
        return NumFmt::Percent {
            decimals: Some(decimals),
        };
    }
    if let Some(symbol) = symbol {
        return NumFmt::Currency { decimals, symbol };
    }
    if plain.contains('0') || plain.contains('#') {
        return NumFmt::Number {
            decimals,
            thousands: plain.contains(','),
        };
    }
    NumFmt::General
}

fn currency_symbol(c: char) -> Option<Currency> {
    Currency::ALL
        .into_iter()
        .find(|cur| cur.symbol().starts_with(c))
}

/* ----------------------------------- charts ----------------------------------- */

/// Relationship id → (type, target).
fn parse_typed_relationships(xml: &str) -> HashMap<String, (String, String)> {
    let mut map = HashMap::new();
    for_each_element(xml, |name, e, _| {
        if name == "Relationship"
            && let (Some(id), Some(target)) = (attr(e, "Id"), attr(e, "Target"))
        {
            map.insert(id, (attr(e, "Type").unwrap_or_default(), target));
        }
    });
    map
}

/// `xl/worksheets/sheet1.xml` → `xl/worksheets/_rels/sheet1.xml.rels`.
fn rels_path(part: &str) -> String {
    match part.rsplit_once('/') {
        Some((dir, file)) => format!("{dir}/_rels/{file}.rels"),
        None => format!("_rels/{part}.rels"),
    }
}

/// Resolve a relationship target relative to the part that refers to it.
fn resolve_target(from_part: &str, target: &str) -> String {
    if let Some(abs) = target.strip_prefix('/') {
        return abs.to_string();
    }
    let mut parts: Vec<&str> = from_part
        .rsplit_once('/')
        .map_or(vec![], |(d, _)| d.split('/').collect());
    for seg in target.split('/') {
        match seg {
            ".." => {
                parts.pop();
            }
            "." | "" => {}
            s => parts.push(s),
        }
    }
    parts.join("/")
}

/// A node seen while walking XML with [`walk`].
enum Node<'a, 'e> {
    Start(&'a BytesStart<'e>),
    Text(&'a str),
    End,
}

/// Walk XML, calling `f` with the path of open element names for every start
/// element, text node and end element.
fn walk(xml: &str, mut f: impl FnMut(&[String], Node)) {
    let mut reader = Reader::from_str(xml);
    let mut stack: Vec<String> = Vec::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                stack.push(e.local_name().as_ref().to_string());
                f(&stack, Node::Start(&e));
            }
            Ok(Event::Empty(e)) => {
                stack.push(e.local_name().as_ref().to_string());
                f(&stack, Node::Start(&e));
                f(&stack, Node::End);
                stack.pop();
            }
            Ok(Event::Text(t)) => f(&stack, Node::Text(&t.xml_content(XmlVersion::Implicit1_0))),
            Ok(Event::GeneralRef(r)) => {
                // Entities arrive separately (&amp; → &).
                let s = match r.as_ref() {
                    "amp" => "&",
                    "lt" => "<",
                    "gt" => ">",
                    "quot" => "\"",
                    "apos" => "'",
                    _ => "",
                };
                f(&stack, Node::Text(s));
            }
            Ok(Event::End(_)) => {
                f(&stack, Node::End);
                stack.pop();
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
}

#[derive(Default, Clone, Copy)]
struct Marker {
    col: u32,
    col_off: i64,
    row: u32,
    row_off: i64,
}

/// A drawing anchor: where a chart sits and which chart part it shows.
#[derive(Default)]
struct Anchor {
    from: Marker,
    to: Option<Marker>,
    /// Size in EMU (one-cell anchors).
    ext: Option<(f64, f64)>,
    chart_rel: String,
}

/// A chart series as written in the XML, before references are parsed.
#[derive(Default)]
struct RawSeries {
    name_ref: Option<String>,
    name: Option<String>,
    categories: Option<String>,
    values: Option<String>,
}

const EMU_PER_PX: f64 = 9525.0;

/// Charts placed on a worksheet, following worksheet → drawing → chart parts.
fn read_charts(
    part: &mut impl FnMut(&str) -> Option<String>,
    sheet_path: &str,
    col_widths: &[(u32, f64)],
) -> Vec<Chart> {
    let Some(rels) = part(&rels_path(sheet_path)) else {
        return Vec::new();
    };
    let mut charts = Vec::new();
    for (kind, target) in parse_typed_relationships(&rels).into_values() {
        if !kind.ends_with("/drawing") {
            continue;
        }
        let drawing_path = resolve_target(sheet_path, &target);
        let Some(drawing) = part(&drawing_path) else {
            continue;
        };
        let drawing_rels = part(&rels_path(&drawing_path))
            .map(|x| parse_typed_relationships(&x))
            .unwrap_or_default();

        // Anchors: (from, to or size in EMU, chart relationship id).
        let mut anchors: Vec<Anchor> = Vec::new();
        let mut cur: Option<Anchor> = None;
        walk(&drawing, |path, node| match node {
            Node::Start(e) => match path.last().map(String::as_str).unwrap_or("") {
                "twoCellAnchor" | "oneCellAnchor" => cur = Some(Anchor::default()),
                "to" => {
                    if let Some(c) = cur.as_mut() {
                        c.to = Some(Marker::default());
                    }
                }
                "ext"
                    if path.iter().any(|p| p == "oneCellAnchor")
                        && !path.iter().any(|p| p == "graphicFrame") =>
                {
                    let n = |a| attr(e, a).and_then(|v| v.parse::<f64>().ok());
                    if let (Some(c), Some(cx), Some(cy)) = (cur.as_mut(), n("cx"), n("cy")) {
                        c.ext = Some((cx, cy));
                    }
                }
                "chart" => {
                    if let (Some(c), Some(id)) = (cur.as_mut(), attr(e, "id")) {
                        c.chart_rel = id;
                    }
                }
                _ => {}
            },
            Node::Text(t) => {
                let (Some(c), n) = (cur.as_mut(), path.len()) else {
                    return;
                };
                if n < 2 {
                    return;
                }
                let marker = match path[n - 2].as_str() {
                    "from" => &mut c.from,
                    "to" => c.to.get_or_insert_with(Marker::default),
                    _ => return,
                };
                let v: i64 = t.trim().parse().unwrap_or(0);
                match path[n - 1].as_str() {
                    "col" => marker.col = v.max(0) as u32,
                    "colOff" => marker.col_off = v,
                    "row" => marker.row = v.max(0) as u32,
                    "rowOff" => marker.row_off = v,
                    _ => {}
                }
            }
            Node::End => {
                if matches!(
                    path.last().map(String::as_str),
                    Some("twoCellAnchor" | "oneCellAnchor")
                ) && let Some(c) = cur.take()
                {
                    anchors.push(c);
                }
            }
        });

        for Anchor {
            from,
            to,
            ext,
            chart_rel,
        } in anchors
        {
            let Some((_, chart_target)) = drawing_rels.get(&chart_rel) else {
                continue;
            };
            let chart_path = resolve_target(&drawing_path, chart_target);
            let Some(xml) = part(&chart_path) else {
                continue;
            };
            let Some(mut chart) = parse_chart(&xml) else {
                continue;
            };
            chart.anchor = (from.row, from.col);
            chart.size = match (to, ext) {
                (_, Some((cx, cy))) => ((cx / EMU_PER_PX) as f32, (cy / EMU_PER_PX) as f32),
                (Some(to), None) => anchor_size(from, to, col_widths),
                _ => DEFAULT_CHART_SIZE,
            };
            charts.push(chart);
        }
    }
    charts
}

/// Size of a two-cell anchor using Excel's default metrics (64 px columns
/// unless set, 20 px rows), which is how the anchor was computed.
fn anchor_size(from: Marker, to: Marker, col_widths: &[(u32, f64)]) -> (f32, f32) {
    let col_px = |c: u32| {
        col_widths
            .iter()
            .find(|(col, _)| *col == c)
            .map_or(64.0, |(_, w)| (w * 7.0 + 5.0).floor())
    };
    let w: f64 = (from.col..to.col).map(col_px).sum::<f64>()
        + (to.col_off - from.col_off) as f64 / EMU_PER_PX;
    let h = (to.row - from.row) as f64 * 20.0 + (to.row_off - from.row_off) as f64 / EMU_PER_PX;
    (w.max(120.0) as f32, h.max(80.0) as f32)
}

/// Kind, title and series of a chart part (`xl/charts/chartN.xml`).
fn parse_chart(xml: &str) -> Option<Chart> {
    let mut kind = None;
    let mut bar_horizontal = false;
    let mut title = String::new();
    let mut series: Vec<RawSeries> = Vec::new();
    walk(xml, |path, node| match node {
        Node::Start(e) => match path.last().map(String::as_str).unwrap_or("") {
            k @ ("barChart" | "bar3DChart" | "lineChart" | "line3DChart" | "pieChart"
            | "pie3DChart" | "doughnutChart" | "scatterChart" | "areaChart"
            | "area3DChart") => {
                kind.get_or_insert(match k {
                    "barChart" | "bar3DChart" => ChartKind::Column,
                    "lineChart" | "line3DChart" => ChartKind::Line,
                    "pieChart" | "pie3DChart" | "doughnutChart" => ChartKind::Pie,
                    "scatterChart" => ChartKind::Scatter,
                    _ => ChartKind::Area,
                });
            }
            "barDir" => bar_horizontal |= attr(e, "val").as_deref() == Some("bar"),
            "ser" => series.push(RawSeries::default()),
            _ => {}
        },
        Node::Text(t) => {
            let has = |n: &str| path.iter().any(|p| p == n);
            let leaf = path.last().map(String::as_str).unwrap_or("");
            if has("title") && !has("ser") && leaf == "t" {
                title.push_str(t);
                return;
            }
            let Some(s) = series.last_mut() else { return };
            let append =
                |slot: &mut Option<String>| slot.get_or_insert_with(String::new).push_str(t);
            match leaf {
                "f" if has("tx") => append(&mut s.name_ref),
                "v" if has("tx") => append(&mut s.name),
                "f" if has("cat") || has("xVal") => append(&mut s.categories),
                "f" if has("val") || has("yVal") => append(&mut s.values),
                _ => {}
            }
        }
        Node::End => {}
    });
    let mut kind = kind?;
    if kind == ChartKind::Column && bar_horizontal {
        kind = ChartKind::Bar;
    }
    let categories = series
        .iter()
        .find_map(|s| s.categories.as_deref().and_then(DataRef::parse));
    let series: Vec<Series> = series
        .into_iter()
        .enumerate()
        .filter_map(|(i, raw)| {
            Some(Series {
                name_ref: raw.name_ref.as_deref().and_then(DataRef::parse),
                name: raw.name.unwrap_or_else(|| format!("Series {}", i + 1)),
                values: DataRef::parse(raw.values.as_deref()?)?,
            })
        })
        .collect();
    if series.is_empty() {
        return None;
    }
    Some(Chart {
        kind,
        title,
        categories,
        series,
        anchor: (0, 0),
        size: DEFAULT_CHART_SIZE,
    })
}

/* --------------------------------- worksheets --------------------------------- */

fn parse_worksheet(xml: &str, styles: &Styles) -> SheetFormatting {
    let mut out = SheetFormatting::default();
    for_each_element(xml, |name, e, _| match name {
        "col" => {
            let (Some(min), Some(max)) = (
                attr(e, "min").and_then(|v| v.parse::<u32>().ok()),
                attr(e, "max").and_then(|v| v.parse::<u32>().ok()),
            ) else {
                return;
            };
            let custom = matches!(attr(e, "customWidth").as_deref(), Some("1" | "true"));
            if let Some(w) = attr(e, "width")
                .and_then(|v| v.parse::<f64>().ok())
                .filter(|_| custom)
            {
                // A trailing <col> often spans to column 16384; cap it.
                for c in min..=max.min(min + 1024) {
                    out.col_widths.push((c - 1, w));
                }
            }
        }
        "pane" => {
            if matches!(attr(e, "state").as_deref(), Some("frozen" | "frozenSplit")) {
                let split = |a| {
                    attr(e, a)
                        .and_then(|v| v.parse::<f64>().ok())
                        .unwrap_or(0.0) as u32
                };
                out.freeze = Some((split("ySplit"), split("xSplit")));
            }
        }
        "mergeCell" => {
            let range = attr(e, "ref").unwrap_or_default();
            if let Some((a, b)) = range.split_once(':')
                && let (Some(a), Some(b)) = (
                    crate::model::refs::parse_cell(a),
                    crate::model::refs::parse_cell(b),
                )
            {
                out.merges
                    .push(((a.0.min(b.0), a.1.min(b.1)), (a.0.max(b.0), a.1.max(b.1))));
            }
        }
        "c" => {
            let Some(s) = attr(e, "s").and_then(|v| v.parse::<usize>().ok()) else {
                return;
            };
            let Some(&(fmt, style)) = styles.xfs.get(s) else {
                return;
            };
            if fmt == NumFmt::General && style.is_default() {
                return;
            }
            if let Some(pos) = attr(e, "r").and_then(|r| crate::model::refs::parse_cell(&r)) {
                out.cells.push((pos, fmt, style));
            }
        }
        _ => {}
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal package shaped like Excel's own output: theme colours with
    /// tints, indexed colours, built-in and custom number formats.
    #[test]
    fn reads_excel_style_package() {
        use std::io::Write;
        let dir = std::env::temp_dir().join(format!("litesheet-xl-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("excel.xlsx");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        let opts = zip::write::SimpleFileOptions::default();
        let mut add = |name: &str, body: &str| {
            zip.start_file(name, opts).unwrap();
            zip.write_all(body.as_bytes()).unwrap();
        };
        add(
            "xl/workbook.xml",
            r#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Budget 2024" sheetId="1" r:id="rId1"/></sheets></workbook>"#,
        );
        add(
            "xl/_rels/workbook.xml.rels",
            r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#,
        );
        add(
            "xl/theme/theme1.xml",
            r#"<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><a:themeElements><a:clrScheme name="Office"><a:dk1><a:sysClr val="windowText" lastClr="000000"/></a:dk1><a:lt1><a:sysClr val="window" lastClr="FFFFFF"/></a:lt1><a:dk2><a:srgbClr val="44546A"/></a:dk2><a:lt2><a:srgbClr val="E7E6E6"/></a:lt2><a:accent1><a:srgbClr val="4472C4"/></a:accent1><a:accent2><a:srgbClr val="ED7D31"/></a:accent2><a:accent3><a:srgbClr val="A5A5A5"/></a:accent3><a:accent4><a:srgbClr val="FFC000"/></a:accent4><a:accent5><a:srgbClr val="5B9BD5"/></a:accent5><a:accent6><a:srgbClr val="70AD47"/></a:accent6><a:hlink><a:srgbClr val="0563C1"/></a:hlink><a:folHlink><a:srgbClr val="954F72"/></a:folHlink></a:clrScheme></a:themeElements></a:theme>"#,
        );
        add(
            "xl/styles.xml",
            r#"<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
            <numFmts count="1"><numFmt numFmtId="164" formatCode="[$&#8364;-407] #,##0.00"/></numFmts>
            <fonts count="3">
              <font><sz val="11"/><color theme="1"/><name val="Calibri"/></font>
              <font><b/><i/><u/><sz val="11"/><color theme="4" tint="0.39997558519241921"/><name val="Calibri"/></font>
              <font><strike/><color indexed="10"/></font>
            </fonts>
            <fills count="3">
              <fill><patternFill patternType="none"/></fill>
              <fill><patternFill patternType="gray125"/></fill>
              <fill><patternFill patternType="solid"><fgColor theme="9"/><bgColor indexed="64"/></patternFill></fill>
            </fills>
            <borders count="2">
              <border><left/><right/><top/><bottom/><diagonal/></border>
              <border><left style="thin"><color indexed="64"/></left><right/><top/><bottom style="medium"/><diagonal/></border>
            </borders>
            <cellXfs count="4">
              <xf numFmtId="0" fontId="0" fillId="0" borderId="0"/>
              <xf numFmtId="0" fontId="1" fillId="2" borderId="1" applyFont="1"><alignment horizontal="center"/></xf>
              <xf numFmtId="10" fontId="2" fillId="0" borderId="0"/>
              <xf numFmtId="164" fontId="0" fillId="0" borderId="0"/>
            </cellXfs>
          </styleSheet>"#,
        );
        add(
            "xl/worksheets/sheet1.xml",
            r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><cols><col min="2" max="3" width="20.5" customWidth="1"/><col min="4" max="4" width="9"/></cols><sheetData><row r="1"><c r="A1" s="1" t="s"><v>0</v></c><c r="B1" s="0"><v>1</v></c></row><row r="2"><c r="A2" s="2"><v>0.25</v></c><c r="C2" s="3"><v>12.5</v></c></row></sheetData></worksheet>"#,
        );
        zip.finish().unwrap();

        let f = &read(&path).unwrap()["Budget 2024"];
        let cell = |r, c| f.cells.iter().find(|x| x.0 == (r, c)).map(|x| (x.1, x.2));

        let (fmt, s) = cell(0, 0).unwrap();
        assert_eq!(fmt, NumFmt::General);
        assert!(s.bold && s.italic && s.underline && !s.strike);
        assert_eq!(s.color, Some(Rgb(0x44, 0x72, 0xC4).tinted(0.39997558)));
        assert_eq!(s.fill, Some(Rgb(0x70, 0xAD, 0x47)));
        assert_eq!(s.align, HAlign::Center);
        assert_eq!(
            s.borders,
            Borders {
                left: true,
                bottom: true,
                ..Borders::NONE
            }
        );
        assert!(cell(0, 1).is_none(), "default-styled cells are skipped");

        let (fmt, s) = cell(1, 0).unwrap();
        assert_eq!(fmt, NumFmt::Percent { decimals: Some(2) });
        assert!(s.strike);
        assert_eq!(s.color, Some(Rgb(255, 0, 0)));

        assert_eq!(
            cell(1, 2).unwrap().0,
            NumFmt::Currency {
                decimals: 2,
                symbol: Currency::Euro
            }
        );
        assert_eq!(
            f.col_widths,
            vec![(1, 20.5), (2, 20.5)],
            "only custom widths"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn excel_chart_xml() {
        let xml = r#"<c:chartSpace xmlns:c="c" xmlns:a="a"><c:chart>
            <c:title><c:tx><c:rich><a:p><a:r><a:t>Revenue &amp; </a:t></a:r><a:r><a:t>Costs</a:t></a:r></a:p></c:rich></c:tx></c:title>
            <c:plotArea><c:barChart><c:barDir val="bar"/><c:grouping val="clustered"/>
              <c:ser><c:idx val="0"/><c:tx><c:strRef><c:f>'Q Data'!$B$1</c:f><c:strCache><c:pt idx="0"><c:v>Revenue</c:v></c:pt></c:strCache></c:strRef></c:tx>
                <c:cat><c:strRef><c:f>'Q Data'!$A$2:$A$5</c:f></c:strRef></c:cat>
                <c:val><c:numRef><c:f>'Q Data'!$B$2:$B$5</c:f></c:numRef></c:val></c:ser>
              <c:ser><c:idx val="1"/><c:tx><c:v>Literal name</c:v></c:tx>
                <c:val><c:numRef><c:f>'Q Data'!$C$2:$C$5</c:f></c:numRef></c:val></c:ser>
            </c:barChart></c:plotArea></c:chart></c:chartSpace>"#;
        let c = parse_chart(xml).unwrap();
        assert_eq!(c.kind, ChartKind::Bar);
        assert_eq!(c.title, "Revenue & Costs");
        assert_eq!(c.categories, Some(DataRef::new("Q Data", (1, 0), (4, 0))));
        assert_eq!(
            c.series[0].name_ref,
            Some(DataRef::new("Q Data", (0, 1), (0, 1)))
        );
        assert_eq!(c.series[1].name, "Literal name");
        assert_eq!(c.series[1].values, DataRef::new("Q Data", (1, 2), (4, 2)));
        assert_eq!(
            resolve_target("xl/drawings/drawing1.xml", "../charts/chart1.xml"),
            "xl/charts/chart1.xml"
        );
        assert_eq!(
            rels_path("xl/worksheets/sheet2.xml"),
            "xl/worksheets/_rels/sheet2.xml.rels"
        );
    }

    #[test]
    fn format_codes() {
        let cur = |decimals, symbol| NumFmt::Currency { decimals, symbol };
        assert_eq!(numfmt_from_code("General"), NumFmt::General);
        assert_eq!(
            numfmt_from_code("0.00"),
            NumFmt::Number {
                decimals: 2,
                thousands: false
            }
        );
        assert_eq!(
            numfmt_from_code("#,##0"),
            NumFmt::Number {
                decimals: 0,
                thousands: true
            }
        );
        assert_eq!(numfmt_from_code("\"€\"#,##0.00"), cur(2, Currency::Euro));
        assert_eq!(
            numfmt_from_code("[$€-407] #,##0.00;-[$€-407] #,##0.00"),
            cur(2, Currency::Euro)
        );
        assert_eq!(
            numfmt_from_code("\\$#,##0_);(\\$#,##0)"),
            cur(0, Currency::Dollar)
        );
        assert_eq!(
            numfmt_from_code("_-[$₹-4009]* #,##0.00_-"),
            cur(2, Currency::Rupee)
        );
        assert_eq!(
            numfmt_from_code("0.0%"),
            NumFmt::Percent { decimals: Some(1) }
        );
        assert_eq!(
            numfmt_from_code("0.00E+00"),
            NumFmt::Scientific { decimals: 2 }
        );
        assert_eq!(numfmt_from_code("yyyy-mm-dd"), NumFmt::Date);
        assert_eq!(numfmt_from_code("mmm-yy"), NumFmt::Date);
        assert_eq!(numfmt_from_code("m/d/yy h:mm"), NumFmt::DateTime);
        assert_eq!(numfmt_from_code("h:mm AM/PM"), NumFmt::Time);
        assert_eq!(numfmt_from_code("[h]:mm:ss"), NumFmt::Time);
        assert_eq!(numfmt_from_code("@"), NumFmt::Text);
        assert_eq!(
            numfmt_from_code("[Red]0.00"),
            NumFmt::Number {
                decimals: 2,
                thousands: false
            }
        );
    }
}
