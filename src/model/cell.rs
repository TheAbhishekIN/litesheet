//! Cells: raw user input, number formats, and interpreting typed text.

use super::Style;
use super::format::{fmt_number, parse_date, parse_time};

/// Zero-based (row, col).
pub type Pos = (u32, u32);

/// How a number is displayed. The value itself never changes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum NumFmt {
    /// As many digits as needed, like Excel's General.
    #[default]
    General,
    /// Fixed decimals, optionally with a thousands separator: `1,234.50`.
    Number {
        decimals: u8,
        thousands: bool,
    },
    /// `$1,234.50`
    Currency {
        decimals: u8,
        symbol: Currency,
    },
    /// `15.5%`; `None` shows as many decimals as needed.
    Percent {
        decimals: Option<u8>,
    },
    /// `1.23E+04`
    Scientific {
        decimals: u8,
    },
    Date,
    DateTime,
    Time,
    /// Shown exactly as typed; nothing is converted to a number or formula.
    Text,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Currency {
    Dollar,
    Euro,
    Pound,
    Yen,
    Rupee,
}

impl Currency {
    pub const ALL: [Currency; 5] = [
        Currency::Dollar,
        Currency::Euro,
        Currency::Pound,
        Currency::Yen,
        Currency::Rupee,
    ];

    pub fn symbol(self) -> &'static str {
        match self {
            Currency::Dollar => "$",
            Currency::Euro => "€",
            Currency::Pound => "£",
            Currency::Yen => "¥",
            Currency::Rupee => "₹",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Currency::Dollar => "Dollar",
            Currency::Euro => "Euro",
            Currency::Pound => "Pound",
            Currency::Yen => "Yen",
            Currency::Rupee => "Rupee",
        }
    }

    fn from_symbol(c: char) -> Option<Currency> {
        Currency::ALL
            .into_iter()
            .find(|cur| cur.symbol().starts_with(c))
    }
}

pub const MAX_DECIMALS: u8 = 15;

impl NumFmt {
    /// Decimal places, for formats that have a fixed number of them.
    pub fn decimals(self) -> Option<u8> {
        match self {
            NumFmt::Number { decimals, .. }
            | NumFmt::Currency { decimals, .. }
            | NumFmt::Scientific { decimals } => Some(decimals),
            NumFmt::Percent { decimals } => decimals,
            _ => None,
        }
    }

    /// The same format with a different number of decimals (General becomes Number).
    pub fn with_decimals(self, decimals: u8) -> NumFmt {
        let decimals = decimals.min(MAX_DECIMALS);
        match self {
            NumFmt::Number { thousands, .. } => NumFmt::Number {
                decimals,
                thousands,
            },
            NumFmt::Currency { symbol, .. } => NumFmt::Currency { decimals, symbol },
            NumFmt::Percent { .. } => NumFmt::Percent {
                decimals: Some(decimals),
            },
            NumFmt::Scientific { .. } => NumFmt::Scientific { decimals },
            _ => NumFmt::Number {
                decimals,
                thousands: false,
            },
        }
    }

    /// Whether decimals can be added or removed.
    pub fn adjusts_decimals(self) -> bool {
        !matches!(
            self,
            NumFmt::Date | NumFmt::DateTime | NumFmt::Time | NumFmt::Text
        )
    }

    /// The Excel number-format code used when saving, if any.
    pub fn excel_code(self) -> Option<String> {
        let fixed = |decimals: u8, thousands: bool| {
            let mut code = if thousands { "#,##0" } else { "0" }.to_string();
            if decimals > 0 {
                code.push('.');
                code.push_str(&"0".repeat(decimals as usize));
            }
            code
        };
        Some(match self {
            NumFmt::General => return None,
            NumFmt::Number {
                decimals,
                thousands,
            } => fixed(decimals, thousands),
            NumFmt::Currency { decimals, symbol } => {
                format!("\"{}\"{}", symbol.symbol(), fixed(decimals, true))
            }
            NumFmt::Percent { decimals } => format!("{}%", fixed(decimals.unwrap_or(2), false)),
            NumFmt::Scientific { decimals } => format!("{}E+00", fixed(decimals, false)),
            NumFmt::Date => "yyyy-mm-dd".into(),
            NumFmt::DateTime => "yyyy-mm-dd hh:mm".into(),
            NumFmt::Time => "hh:mm:ss".into(),
            NumFmt::Text => "@".into(),
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Cell {
    /// What the user typed: `=SUM(A1:A3)`, `42`, `hello`, `'0042`.
    pub raw: String,
    pub fmt: NumFmt,
    pub style: Style,
}

impl Cell {
    pub fn new(raw: impl Into<String>) -> Self {
        Self {
            raw: raw.into(),
            fmt: NumFmt::General,
            style: Style::default(),
        }
    }
    pub fn is_formula(&self) -> bool {
        self.fmt != NumFmt::Text && is_formula(&self.raw)
    }

    pub fn with_fmt(raw: impl Into<String>, fmt: NumFmt) -> Self {
        Self {
            fmt,
            ..Self::new(raw)
        }
    }

    /// True when the cell has no content, number format or style.
    pub fn is_blank(&self) -> bool {
        self.raw.is_empty() && self.fmt == NumFmt::General && self.style.is_default()
    }

    /// The cell's formatting without its content (what Delete leaves behind).
    pub fn formatting_only(&self) -> Option<Cell> {
        let c = Cell {
            raw: String::new(),
            ..self.clone()
        };
        (!c.is_blank()).then_some(c)
    }
}

pub fn is_formula(raw: &str) -> bool {
    raw.len() > 1 && raw.starts_with('=')
}

/// Turn user input into a cell. Recognises dates (`2024-05-01`), times
/// (`14:30`, `2:30 PM`), percentages (`15%`), currency (`$1,250.50`) and
/// thousands separators (`1,234`), and keeps the cell's existing format when
/// a plain number is typed into it.
pub fn interpret_input(input: &str, prev_fmt: NumFmt) -> Option<Cell> {
    let t = input.trim();
    if input.is_empty() {
        return None;
    }
    let cell = |raw: String, fmt: NumFmt| Some(Cell::with_fmt(raw, fmt));
    if prev_fmt == NumFmt::Text {
        return cell(input.to_string(), NumFmt::Text);
    }
    if is_formula(t) || t.starts_with('\'') {
        return cell(input.to_string(), prev_fmt);
    }
    if let Some(serial) = parse_date(t) {
        let fmt = if serial.fract() != 0.0 {
            NumFmt::DateTime
        } else {
            NumFmt::Date
        };
        return cell(fmt_number(serial), fmt);
    }
    if let Some(time) = parse_time(t) {
        return cell(fmt_number(time), NumFmt::Time);
    }
    if let Some(p) = t.strip_suffix('%')
        && let Some(n) = parse_grouped(p.trim())
    {
        let decimals = match prev_fmt {
            NumFmt::Percent { decimals } => decimals,
            _ => None,
        };
        return cell(fmt_number(n / 100.0), NumFmt::Percent { decimals });
    }
    if let Some((n, symbol, decimals)) = parse_currency(t) {
        return cell(fmt_number(n), NumFmt::Currency { decimals, symbol });
    }
    if t.contains(',')
        && let Some(n) = parse_grouped(t)
    {
        let decimals = t
            .split_once('.')
            .map_or(0, |(_, d)| d.len().min(MAX_DECIMALS as usize) as u8);
        return cell(
            fmt_number(n),
            NumFmt::Number {
                decimals,
                thousands: true,
            },
        );
    }
    if let Ok(n) = t.parse::<f64>() {
        // A plain number typed into a percent cell means that many percent.
        if let NumFmt::Percent { .. } = prev_fmt {
            return cell(fmt_number(n / 100.0), prev_fmt);
        }
        return cell(input.to_string(), prev_fmt);
    }
    // Text keeps the cell's number format, ready for when a number is typed.
    cell(input.to_string(), prev_fmt)
}

/// `1,234.5` → 1234.5 (commas only between digit groups).
fn parse_grouped(s: &str) -> Option<f64> {
    let (sign, body) = match s.strip_prefix('-') {
        Some(rest) => (-1.0, rest),
        None => (1.0, s),
    };
    let int = body.split('.').next()?;
    if int.contains(',') {
        let groups: Vec<&str> = int.split(',').collect();
        let ok = !groups[0].is_empty()
            && groups[0].len() <= 3
            && groups[1..].iter().all(|g| g.len() == 3);
        if !ok {
            return None;
        }
    }
    let n: f64 = body.replace(',', "").parse().ok()?;
    n.is_finite().then_some(sign * n)
}

/// `$1,250.50`, `-€12`, `₹ 500` → (value, symbol, decimals shown).
fn parse_currency(s: &str) -> Option<(f64, Currency, u8)> {
    let (neg, rest) = match s.strip_prefix('-') {
        Some(r) => (true, r.trim_start()),
        None => (false, s),
    };
    let first = rest.chars().next()?;
    let symbol = Currency::from_symbol(first)?;
    let digits = rest[first.len_utf8()..].trim_start();
    let (neg, digits) = match digits.strip_prefix('-') {
        Some(d) => (!neg, d),
        None => (neg, digits),
    };
    let n = parse_grouped(digits)?;
    let decimals = if digits.contains('.') { 2 } else { 0 };
    Some((if neg { -n } else { n }, symbol, decimals))
}
