//! Excel-style display formatting for numbers, percentages and dates.

use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime};

use super::{NumFmt, Value};

pub fn format_value(v: &Value, fmt: NumFmt) -> String {
    match v {
        Value::Empty => String::new(),
        Value::Text(s) => s.clone(),
        Value::Bool(b) => if *b { "TRUE" } else { "FALSE" }.to_string(),
        Value::Error(e) => e.clone(),
        Value::Number(n) => format_number(*n, fmt),
    }
}

pub fn format_number(n: f64, fmt: NumFmt) -> String {
    let date = |pattern: &str| {
        serial_to_datetime(n)
            .map(|d| d.format(pattern).to_string())
            .unwrap_or_else(|| fmt_number(n))
    };
    match fmt {
        NumFmt::General | NumFmt::Text => fmt_number(n),
        NumFmt::Number {
            decimals,
            thousands,
        } => fmt_fixed(n, decimals, thousands),
        NumFmt::Currency { decimals, symbol } => {
            let body = fmt_fixed(n.abs(), decimals, true);
            let negative = n < 0.0 && body.chars().any(|c| c.is_ascii_digit() && c != '0');
            format!(
                "{}{}{body}",
                if negative { "-" } else { "" },
                symbol.symbol()
            )
        }
        NumFmt::Percent { decimals: None } => format!("{}%", fmt_number(round_sig(n * 100.0, 12))),
        NumFmt::Percent { decimals: Some(d) } => format!("{}%", fmt_fixed(n * 100.0, d, false)),
        NumFmt::Scientific { decimals } => fmt_scientific(n, decimals),
        NumFmt::Date => date("%Y-%m-%d"),
        NumFmt::DateTime => date("%Y-%m-%d %H:%M"),
        NumFmt::Time => {
            let secs = (n.fract().abs() * 86400.0).round() as u32 % 86400;
            format!("{:02}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60)
        }
    }
}

/// `1234.567` with 2 decimals and separators → `1,234.57`.
pub fn fmt_fixed(n: f64, decimals: u8, thousands: bool) -> String {
    if !n.is_finite() {
        return "#NUM!".into();
    }
    let s = format!("{:.*}", decimals as usize, round_excel(n.abs(), decimals));
    let (int, frac) = match s.split_once('.') {
        Some((i, f)) => (i, Some(f)),
        None => (s.as_str(), None),
    };
    let mut out = String::new();
    // No sign on values that round to zero ("-0.00").
    if n < 0.0 && s.chars().any(|c| c.is_ascii_digit() && c != '0') {
        out.push('-');
    }
    if thousands {
        for (i, ch) in int.chars().enumerate() {
            if i > 0 && (int.len() - i) % 3 == 0 {
                out.push(',');
            }
            out.push(ch);
        }
    } else {
        out.push_str(int);
    }
    if let Some(f) = frac {
        out.push('.');
        out.push_str(f);
    }
    out
}

/// `12345` with 2 decimals → `1.23E+04`.
pub fn fmt_scientific(n: f64, decimals: u8) -> String {
    if !n.is_finite() {
        return "#NUM!".into();
    }
    let d = decimals as usize;
    if n == 0.0 {
        return format!("{:.d$}E+00", 0.0);
    }
    let mut exp = n.abs().log10().floor() as i32;
    let mut mantissa = round_excel(n / 10f64.powi(exp), decimals);
    // Rounding can carry into the next power of ten (9.999 → 10.00).
    if mantissa.abs() >= 10.0 {
        exp += 1;
        mantissa = round_excel(n / 10f64.powi(exp), decimals);
    }
    let sign = if exp < 0 { '-' } else { '+' };
    format!("{mantissa:.d$}E{sign}{:02}", exp.abs())
}

/// `14:30`, `14:30:15`, `2:30 PM` → fraction of a day.
pub fn parse_time(s: &str) -> Option<f64> {
    let s = s.trim();
    let upper = s.to_ascii_uppercase();
    let (body, pm) = if let Some(b) = upper.strip_suffix("PM") {
        (b.trim(), Some(true))
    } else if let Some(b) = upper.strip_suffix("AM") {
        (b.trim(), Some(false))
    } else {
        (upper.as_str(), None)
    };
    let parts: Vec<&str> = body.split(':').collect();
    if !(2..=3).contains(&parts.len())
        || parts
            .iter()
            .any(|p| p.is_empty() || !p.chars().all(|c| c.is_ascii_digit()))
    {
        return None;
    }
    let mut h: u32 = parts[0].parse().ok()?;
    let m: u32 = parts[1].parse().ok()?;
    let sec: u32 = parts.get(2).map_or(Some(0), |p| p.parse().ok())?;
    if m > 59 || sec > 59 || parts[1].len() != 2 {
        return None;
    }
    match pm {
        Some(pm) => {
            if !(1..=12).contains(&h) {
                return None;
            }
            h = h % 12 + if pm { 12 } else { 0 };
        }
        None if h > 23 => return None,
        None => {}
    }
    Some((h * 3600 + m * 60 + sec) as f64 / 86400.0)
}

/// Round like Excel: to 15 significant digits first (so 1.005 is treated as
/// exactly 1.005), then half away from zero (0.5 → 1, 2.5 → 3).
fn round_excel(n: f64, decimals: u8) -> f64 {
    let factor = 10f64.powi(decimals as i32);
    let scaled = round_sig(n, 15) * factor;
    if scaled.abs() >= 1e17 {
        n
    } else {
        round_sig(scaled, 15).round() / factor
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
