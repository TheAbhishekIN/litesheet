//! Computed cell values, parsing literals, and sort order.

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Empty,
    Number(f64),
    Text(String),
    Bool(bool),
    Error(String),
}

impl Value {
    pub fn as_number(&self) -> Option<f64> {
        match self {
            Value::Number(n) => Some(*n),
            _ => None,
        }
    }
}

/// Interpret raw (non-formula) cell text as a value.
pub fn parse_literal(raw: &str) -> Value {
    if let Some(text) = raw.strip_prefix('\'') {
        return Value::Text(text.to_string());
    }
    let t = raw.trim();
    if t.is_empty() {
        return Value::Empty;
    }
    if let Ok(n) = t.replace(',', "").parse::<f64>()
        && n.is_finite()
        && !t.starts_with(',')
        && !t.ends_with(',')
    {
        return Value::Number(n);
    }
    if t.eq_ignore_ascii_case("true") {
        return Value::Bool(true);
    }
    if t.eq_ignore_ascii_case("false") {
        return Value::Bool(false);
    }
    Value::Text(raw.to_string())
}

/// Sort order between two values: numbers < text < booleans < errors < empty.
pub fn compare_values(a: &Value, b: &Value) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    fn rank(v: &Value) -> u8 {
        match v {
            Value::Number(_) => 0,
            Value::Text(_) => 1,
            Value::Bool(_) => 2,
            Value::Error(_) => 3,
            Value::Empty => 4,
        }
    }
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.partial_cmp(y).unwrap_or(Ordering::Equal),
        (Value::Text(x), Value::Text(y)) => x.to_lowercase().cmp(&y.to_lowercase()),
        (Value::Bool(x), Value::Bool(y)) => x.cmp(y),
        _ => rank(a).cmp(&rank(b)),
    }
}
