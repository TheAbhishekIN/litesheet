//! Bring-Your-Own-Key AI: task prompts, an OpenAI-compatible client,
//! settings persistence and Keychain storage for the API key.

mod client;
mod keychain;
mod settings;

use serde::Deserialize;

use crate::model::filter::Condition;

pub use client::spawn;
pub use keychain::{delete_key, load_key, store_key};
pub use settings::AiSettings;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Task {
    Formula,
    Explain,
    Filter,
    Summarize,
}

impl Task {
    pub const ALL: [Task; 4] = [Task::Formula, Task::Explain, Task::Filter, Task::Summarize];

    pub fn label(self) -> &'static str {
        match self {
            Task::Formula => "Formula",
            Task::Explain => "Explain",
            Task::Filter => "Filter",
            Task::Summarize => "Summarize",
        }
    }

    pub fn placeholder(self) -> &'static str {
        match self {
            Task::Formula => "e.g. total of Price where Region is \"West\"",
            Task::Explain => "Optional: what do you want to know about the selection?",
            Task::Filter => "e.g. orders over 500 from 2024 that aren't shipped",
            Task::Summarize => "Optional: anything to focus on?",
        }
    }

    fn system_prompt(self) -> &'static str {
        match self {
            Task::Formula => {
                "You write Microsoft Excel formulas. Reply with exactly one formula that starts \
                 with '=' and nothing else: no explanation, no code fences. Use A1 references \
                 that match the sheet layout you are given. Prefer widely supported functions."
            }
            Task::Explain => {
                "You are a concise data analyst. Explain the spreadsheet data you are given in \
                 plain language: what it represents, notable patterns, outliers, and anything \
                 that looks wrong. Use short paragraphs or bullet points. Plain text, no tables."
            }
            Task::Filter => {
                "You convert natural-language filter requests into JSON for a spreadsheet. \
                 Reply with only a JSON object of the form \
                 {\"filters\":[{\"column\":\"<column letter>\",\"op\":\"<op>\",\"value\":\"<text>\"}]}. \
                 Valid ops: eq, neq, contains, not_contains, starts_with, ends_with, gt, gte, lt, \
                 lte, empty, not_empty. All filters are combined with AND. Several filters may \
                 target the same column (e.g. a range uses gte and lte). Dates are written as \
                 YYYY-MM-DD. Text comparisons are case-insensitive. No commentary."
            }
            Task::Summarize => {
                "You are a concise data analyst. Summarize this spreadsheet for a busy reader: \
                 what it contains, key figures and totals, trends, and notable outliers. Start \
                 with a one-sentence overview, then up to 8 bullet points. Plain text."
            }
        }
    }
}

pub struct Request {
    pub task: Task,
    pub prompt: String,
    pub context: String,
}

pub struct Response {
    pub task: Task,
    pub result: Result<String, String>,
}

fn strip_fences(s: &str) -> &str {
    let s = s.trim();
    let s = s
        .strip_prefix("```")
        .map(|r| r.split_once('\n').map(|x| x.1).unwrap_or(r))
        .unwrap_or(s);
    s.strip_suffix("```").unwrap_or(s).trim()
}

/// Pull a single formula out of a model reply.
pub fn extract_formula(reply: &str) -> Option<String> {
    let s = strip_fences(reply);
    s.lines()
        .map(|l| l.trim().trim_matches('`').trim())
        .find(|l| l.starts_with('=') && l.len() > 1)
        .map(str::to_string)
}

#[derive(Debug, Deserialize)]
struct FilterReply {
    filters: Vec<FilterItem>,
}

#[derive(Debug, Deserialize)]
pub struct FilterItem {
    pub column: String,
    #[serde(flatten)]
    pub condition: Condition,
}

pub fn parse_filters(reply: &str) -> Result<Vec<FilterItem>, String> {
    let s = strip_fences(reply);
    let start = s.find('{').ok_or("The AI reply didn't contain JSON")?;
    let end = s.rfind('}').ok_or("The AI reply didn't contain JSON")?;
    let parsed: FilterReply = serde_json::from_str(&s[start..=end])
        .map_err(|e| format!("Couldn't read filter JSON: {e}"))?;
    Ok(parsed.filters)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::filter::Op;

    #[test]
    fn parsing() {
        assert_eq!(
            extract_formula("```excel\n=SUM(A1:A3)\n```").as_deref(),
            Some("=SUM(A1:A3)")
        );
        assert_eq!(extract_formula("`=A1*2`").as_deref(), Some("=A1*2"));
        let f = parse_filters(
            "```json\n{\"filters\":[{\"column\":\"B\",\"op\":\"gte\",\"value\":\"10\"},{\"column\":\"Status\",\"op\":\"not_empty\"}]}\n```",
        )
        .unwrap();
        assert_eq!(f.len(), 2);
        assert_eq!(f[0].condition.op, Op::Gte);
        assert_eq!(f[1].column, "Status");
    }
}
