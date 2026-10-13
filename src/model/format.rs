//! Excel-style display formatting for numbers, percentages and dates.

use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime};

use super::{NumFmt, Value};

pub fn format_value(v: &Value, fmt: NumFmt) -> String {
    match v {
        Value::Empty => String::new(),
        Value::Text(s) => s.clone(),
        Value::Bool(b) => if *b { "TRUE" } else { "FALSE" }.to_string(),
        Value::Error(e) => e.clone(),
        Value::Number(n) => match fmt {
            NumFmt::General => fmt_number(*n),
            NumFmt::Percent => format!("{}%", fmt_number(round_sig(n * 100.0, 12))),
            NumFmt::Date => serial_to_datetime(*n)
                .map(|d| d.format("%Y-%m-%d").to_string())
                .unwrap_or_else(|| fmt_number(*n)),
            NumFmt::DateTime => serial_to_datetime(*n)
                .map(|d| d.format("%Y-%m-%d %H:%M").to_string())
                .unwrap_or_else(|| fmt_number(*n)),
        },
    }
}

fn round_sig(n: f64, sig: i32) -> f64 {
    if n == 0.0 || !n.is_finite() {
        return n;
    }
    let mag = n.abs().log10().floor() as i32;
    let factor = 10f64.powi(sig - 1 - mag);
    (n * factor).round() / factor
}

/// Excel-like "General" number format: up to 15 significant digits.
pub fn fmt_number(n: f64) -> String {
    if !n.is_finite() {
        return "#NUM!".into();
    }
    if n == 0.0 {
        return "0".into();
    }
    let a = n.abs();
    if !(1e-9..1e15).contains(&a) {
        return format!("{n:.6E}");
    }
    if n.fract() == 0.0 {
        return format!("{n:.0}");
    }
    let mag = a.log10().floor() as i32;
    let decimals = (14 - mag).clamp(0, 15) as usize;
    let s = format!("{n:.decimals$}");
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        s
    }
}

fn excel_epoch() -> NaiveDateTime {
    NaiveDate::from_ymd_opt(1899, 12, 30)
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap()
}

pub fn serial_to_datetime(serial: f64) -> Option<NaiveDateTime> {
    if !(0.0..2_958_466.0).contains(&serial) {
        return None;
    }
    // Serials below 61 sit before Excel's phantom 1900-02-29.
    let adj = if serial < 61.0 { serial + 1.0 } else { serial };
    let secs = (adj * 86400.0).round() as i64;
    Some(excel_epoch() + Duration::seconds(secs))
}

pub fn date_to_serial(dt: NaiveDateTime) -> f64 {
    let secs = (dt - excel_epoch()).num_seconds() as f64;
    let serial = secs / 86400.0;
    if serial < 61.0 { serial - 1.0 } else { serial }
}

/// Parse `YYYY-MM-DD`, `YYYY/MM/DD`, optionally followed by ` HH:MM[:SS]`.
pub fn parse_date(s: &str) -> Option<f64> {
    let s = s.trim();
    if s.len() < 8 || !s.as_bytes()[0].is_ascii_digit() {
        return None;
    }
    let norm = s.replace('/', "-");
    for f in ["%Y-%m-%d %H:%M:%S", "%Y-%m-%d %H:%M", "%Y-%m-%dT%H:%M:%S"] {
        if let Ok(dt) = NaiveDateTime::parse_from_str(&norm, f) {
            return Some(date_to_serial(dt));
        }
    }
    let d = NaiveDate::parse_from_str(&norm, "%Y-%m-%d").ok()?;
    if d.year() < 1900 {
        return None;
    }
    Some(date_to_serial(d.and_hms_opt(0, 0, 0)?))
}
