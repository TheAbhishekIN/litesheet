//! Charts: what they plot (ranges of cells) and how, independent of drawing.
//!
//! A chart mirrors Excel's model: a category range plus one or more series,
//! each with a name and a value range. That maps one-to-one onto .xlsx and
//! allows non-contiguous data.

use super::refs::{self, Axis, Structural};
use super::{Book, Pos, Sheet, Value};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ChartKind {
    Column,
    Bar,
    Line,
    Area,
    Pie,
    Scatter,
}

impl ChartKind {
    pub const ALL: [ChartKind; 6] = [
        ChartKind::Column,
        ChartKind::Bar,
        ChartKind::Line,
        ChartKind::Area,
        ChartKind::Pie,
        ChartKind::Scatter,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ChartKind::Column => "Column",
            ChartKind::Bar => "Bar",
            ChartKind::Line => "Line",
            ChartKind::Area => "Area",
            ChartKind::Pie => "Pie",
            ChartKind::Scatter => "Scatter",
        }
    }
}

/// A rectangular range on a named sheet (normally a single row or column).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DataRef {
    pub sheet: String,
    pub start: Pos,
    pub end: Pos,
}

impl DataRef {
    pub fn new(sheet: &str, start: Pos, end: Pos) -> Self {
        let (a, b) = (
            (start.0.min(end.0), start.1.min(end.1)),
            (start.0.max(end.0), start.1.max(end.1)),
        );
        Self {
            sheet: sheet.to_string(),
            start: a,
            end: b,
        }
    }

    /// `Sheet1!$B$2:$B$9`, as Excel writes chart references.
    pub fn to_formula(&self) -> String {
        let cell = |(r, c): Pos| format!("${}${}", refs::col_name(c), r + 1);
        let range = if self.start == self.end {
            cell(self.start)
        } else {
            format!("{}:{}", cell(self.start), cell(self.end))
        };
        format!("{}!{range}", refs::quote_sheet(&self.sheet))
    }

    /// Parse `Sheet1!$B$2:$B$9` or `'My Data'!A1`.
    pub fn parse(formula: &str) -> Option<DataRef> {
        let f = formula.trim().trim_start_matches('=');
        let (sheet, range) = f.rsplit_once('!')?;
        let sheet = match sheet.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')) {
            Some(quoted) => quoted.replace("''", "'"),
            None => sheet.to_string(),
        };
        let (a, b) = range.split_once(':').unwrap_or((range, range));
        Some(DataRef::new(
            &sheet,
            refs::parse_cell(a)?,
            refs::parse_cell(b)?,
        ))
    }

    /// The cells in row-major order.
    pub fn cells(&self) -> impl Iterator<Item = Pos> + '_ {
        (self.start.0..=self.end.0)
            .flat_map(move |r| (self.start.1..=self.end.1).map(move |c| (r, c)))
    }

    pub fn len(&self) -> usize {
        ((self.end.0 - self.start.0 + 1) * (self.end.1 - self.start.1 + 1)) as usize
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Follow rows/columns inserted or deleted on `change.sheet`; `None` if
    /// the whole range was deleted.
    pub fn adjusted(&self, change: &Structural) -> Option<DataRef> {
        if !self.sheet.eq_ignore_ascii_case(change.sheet) {
            return Some(self.clone());
        }
        let (start, end) = match change.axis {
            Axis::Rows => {
                let (r0, r1) = change.map_span(self.start.0, self.end.0)?;
                ((r0, self.start.1), (r1, self.end.1))
            }
            Axis::Cols => {
                let (c0, c1) = change.map_span(self.start.1, self.end.1)?;
                ((self.start.0, c0), (self.end.0, c1))
            }
        };
        Some(DataRef {
            sheet: self.sheet.clone(),
            start,
            end,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Series {
    /// A cell holding the series name, or literal text.
    pub name_ref: Option<DataRef>,
    pub name: String,
    pub values: DataRef,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Chart {
    pub kind: ChartKind,
    pub title: String,
    pub categories: Option<DataRef>,
    pub series: Vec<Series>,
    /// Top-left cell the chart is anchored to, and its size in points.
    pub anchor: Pos,
    pub size: (f32, f32),
}

pub const DEFAULT_CHART_SIZE: (f32, f32) = (480.0, 288.0);

/// Values ready to draw.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChartData {
    pub categories: Vec<String>,
    /// Numeric categories, for scatter charts.
    pub x: Vec<Option<f64>>,
    pub series: Vec<(String, Vec<Option<f64>>)>,
}

impl Chart {
    /// A chart for a block of data: a text first row becomes series names and
    /// a text (or date) first column becomes categories, as Excel guesses.
    pub fn from_block(sheet: &Sheet, start: Pos, end: Pos, kind: ChartKind) -> Option<Chart> {
        let ((r0, c0), (r1, c1)) = (
            (start.0.min(end.0), start.1.min(end.1)),
            (start.0.max(end.0), start.1.max(end.1)),
        );
        let is_text =
            |p: Pos| matches!(sheet.value(p), Value::Text(_)) || sheet.value(p) == &Value::Empty;
        let header = r1 > r0
            && (c0..=c1).any(|c| matches!(sheet.value((r0, c)), Value::Text(_)))
            && (c0..=c1).all(|c| is_text((r0, c)));
        let data_r0 = if header { r0 + 1 } else { r0 };
        let label_col = c1 > c0
            && (data_r0..=r1).all(|r| {
                let v = sheet.value((r, c0));
                matches!(v, Value::Text(_) | Value::Empty)
                    || matches!(
                        sheet.fmt((r, c0)),
                        super::NumFmt::Date | super::NumFmt::DateTime | super::NumFmt::Time
                    )
            });
        let first_series = if label_col { c0 + 1 } else { c0 };
        if data_r0 > r1 || first_series > c1 {
            return None;
        }
        let name = &sheet.name;
        let categories = label_col.then(|| DataRef::new(name, (data_r0, c0), (r1, c0)));
        let series = (first_series..=c1)
            .map(|c| Series {
                name_ref: header.then(|| DataRef::new(name, (r0, c), (r0, c))),
                name: if header {
                    sheet.display((r0, c))
                } else {
                    format!("Series {}", c - first_series + 1)
                },
                values: DataRef::new(name, (data_r0, c), (r1, c)),
            })
            .collect();
        let title = match (header, label_col) {
            (true, _) if c1 == first_series => sheet.display((r0, c1)),
            _ => String::new(),
        };
        Some(Chart {
            kind,
            title,
            categories,
            series,
            anchor: (r0, c1 + 2),
            size: DEFAULT_CHART_SIZE,
        })
    }

    /// The whole block of cells the chart reads (for display in the editor).
    pub fn data_block(&self) -> Option<DataRef> {
        let refs: Vec<&DataRef> = self
            .categories
            .iter()
            .chain(self.series.iter().map(|s| &s.values))
            .chain(self.series.iter().filter_map(|s| s.name_ref.as_ref()))
            .collect();
        let first = refs.first()?;
        let mut block = (*first).clone();
        for r in &refs[1..] {
            if !r.sheet.eq_ignore_ascii_case(&block.sheet) {
                return None;
            }
            block.start = (block.start.0.min(r.start.0), block.start.1.min(r.start.1));
            block.end = (block.end.0.max(r.end.0), block.end.1.max(r.end.1));
        }
        Some(block)
    }

    /// Read the current values from the workbook.
    pub fn data(&self, book: &Book) -> ChartData {
        let sheet = |name: &str| {
            book.sheets
                .iter()
                .find(|s| s.name.eq_ignore_ascii_case(name))
        };
        let mut out = ChartData::default();
        if let Some(cats) = &self.categories
            && let Some(s) = sheet(&cats.sheet)
        {
            out.categories = cats.cells().map(|p| s.display(p)).collect();
            out.x = cats.cells().map(|p| s.value(p).as_number()).collect();
        }
        for series in &self.series {
            let name = series
                .name_ref
                .as_ref()
                .and_then(|r| sheet(&r.sheet).map(|s| s.display(r.start)))
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| series.name.clone());
            let values = match sheet(&series.values.sheet) {
                Some(s) => series
                    .values
                    .cells()
                    .map(|p| s.value(p).as_number())
                    .collect(),
                None => Vec::new(),
            };
            out.series.push((name, values));
        }
        let n = out.series.iter().map(|s| s.1.len()).max().unwrap_or(0);
        if out.categories.is_empty() {
            out.categories = (1..=n).map(|i| i.to_string()).collect();
            out.x = (1..=n).map(|i| Some(i as f64)).collect();
        }
        out
    }

    /// Follow a structural change; `None` when no data is left to plot.
    pub fn adjusted(
        &self,
        change: &Structural,
        own_sheet: usize,
        changed_sheet: usize,
    ) -> Option<Chart> {
        let mut c = self.clone();
        c.categories = self.categories.as_ref().and_then(|r| r.adjusted(change));
        c.series = self
            .series
            .iter()
            .filter_map(|s| {
                Some(Series {
                    name_ref: s.name_ref.as_ref().and_then(|r| r.adjusted(change)),
                    name: s.name.clone(),
                    values: s.values.adjusted(change)?,
                })
            })
            .collect();
        if c.series.is_empty() {
            return None;
        }
        if own_sheet == changed_sheet {
            let (r, col) = self.anchor;
            c.anchor = match change.axis {
                Axis::Rows => (change.map_index(r).unwrap_or(change.at), col),
                Axis::Cols => (r, change.map_index(col).unwrap_or(change.at)),
            };
        }
        Some(c)
    }

    /// Point references at a renamed sheet.
    pub fn renamed(&self, old: &str, new: &str) -> Chart {
        let fix = |r: &DataRef| {
            let mut r = r.clone();
            if r.sheet.eq_ignore_ascii_case(old) {
                r.sheet = new.to_string();
            }
            r
        };
        let mut c = self.clone();
        c.categories = self.categories.as_ref().map(fix);
        for s in &mut c.series {
            s.values = fix(&s.values);
            s.name_ref = s.name_ref.as_ref().map(fix);
        }
        c
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Book {
        let mut b = Book::new_blank();
        let rows = [
            ["Month", "Sales", "Costs"],
            ["Jan", "10", "4"],
            ["Feb", "12", "5"],
            ["Mar", "9", "6"],
        ];
        for (r, row) in rows.iter().enumerate() {
            for (c, v) in row.iter().enumerate() {
                b.set_input(0, (r as u32, c as u32), v);
            }
        }
        b
    }

    #[test]
    fn block_detection_and_data() {
        let b = sample();
        let chart = Chart::from_block(&b.sheets[0], (0, 0), (3, 2), ChartKind::Column).unwrap();
        assert_eq!(
            chart.categories,
            Some(DataRef::new("Sheet1", (1, 0), (3, 0)))
        );
        assert_eq!(chart.series.len(), 2);
        assert_eq!(chart.series[1].values.to_formula(), "Sheet1!$C$2:$C$4");
        assert_eq!(chart.anchor, (0, 4));
        let data = chart.data(&b);
        assert_eq!(data.categories, ["Jan", "Feb", "Mar"]);
        assert_eq!(
            data.series[0],
            ("Sales".to_string(), vec![Some(10.0), Some(12.0), Some(9.0)])
        );
        assert_eq!(
            chart.data_block(),
            Some(DataRef::new("Sheet1", (0, 0), (3, 2)))
        );

        // A single numeric column: no categories, title from its header.
        let single = Chart::from_block(&b.sheets[0], (0, 1), (3, 1), ChartKind::Line).unwrap();
        assert!(single.categories.is_none());
        assert_eq!(single.title, "Sales");
        assert_eq!(single.data(&b).categories, ["1", "2", "3"]);
    }

    #[test]
    fn references_parse_and_follow_changes() {
        let r = DataRef::parse("='My Data'!$B$2:$B$9").unwrap();
        assert_eq!(r, DataRef::new("My Data", (1, 1), (8, 1)));
        assert_eq!(r.to_formula(), "'My Data'!$B$2:$B$9");
        assert_eq!(DataRef::parse("Sheet1!C3").unwrap().len(), 1);

        let ins = Structural {
            sheet: "My Data",
            axis: Axis::Rows,
            at: 0,
            count: 2,
        };
        assert_eq!(
            r.adjusted(&ins).unwrap(),
            DataRef::new("My Data", (3, 1), (10, 1))
        );
        let del = Structural {
            sheet: "My Data",
            axis: Axis::Cols,
            at: 1,
            count: -1,
        };
        assert_eq!(r.adjusted(&del), None);
        let other = Structural {
            sheet: "Elsewhere",
            axis: Axis::Rows,
            at: 0,
            count: 5,
        };
        assert_eq!(r.adjusted(&other).unwrap(), r);
    }
}
