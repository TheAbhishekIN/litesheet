//! Bring-Your-Own-Key AI: task prompts, an OpenAI-compatible client,
//! settings persistence and Keychain storage for the API key.

mod client;
mod keychain;
mod settings;

use serde::Deserialize;

use crate::model::filter::Condition;

pub use client::{list_models, spawn};
pub use keychain::{delete_key, load_key, store_key};
pub use settings::{AiSettings, Effort, Provider};

/// Shortcuts in the AI menu and the chat's empty state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Task {
    Formula,
    Explain,
    Filter,
    Summarize,
}

impl Task {
    pub const ALL: [Task; 4] = [Task::Summarize, Task::Explain, Task::Formula, Task::Filter];
}

const SYSTEM_PROMPT: &str = "You are the AI assistant built into LiteSheet, a spreadsheet app. \
You help the user understand, analyse and work with the spreadsheet they have open. Every \
request comes with a fresh snapshot of the active sheet (columns, sample rows, the selection); \
base your answers on it and say so when the snapshot doesn't contain what you need instead of \
guessing.

Write concise plain text: short paragraphs, '-' bullet lists and **bold** are fine; no tables \
and no headings.

When the user wants a formula, put exactly one Excel formula that starts with '=' on its own in a \
fenced code block tagged formula, for example:
```formula
=SUMIF(C2:C9,\"West\",F2:F9)
```
Use A1 references that match the sheet layout and widely supported functions. The app offers to \
insert it into the cell the user has selected.

When the user wants to see only some rows, add a fenced code block tagged filter containing JSON: \
{\"filters\":[{\"column\":\"<column letter>\",\"op\":\"<op>\",\"value\":\"<text>\"}]}. Valid ops: eq, \
neq, contains, not_contains, starts_with, ends_with, gt, gte, lt, lte, empty, not_empty. Filters \
are combined with AND; several may target the same column (a range uses gte and lte). Dates are \
written YYYY-MM-DD; text comparisons are case-insensitive. The app offers to apply it.";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
}

#[derive(Clone, Debug)]
pub struct Message {
    pub role: Role,
    pub text: String,
}

pub struct Request {
    /// The conversation so far, ending with the user's new message.
    pub history: Vec<Message>,
    /// A snapshot of the spreadsheet, sent fresh with every request.
    pub context: String,
    /// Identifies the request, so a stopped one's late answer can be ignored.
    pub id: u64,
}

pub struct Response {
    pub id: u64,
    pub result: Result<String, String>,
}

/// A piece of an assistant reply, as the chat shows it.
#[derive(Clone, Debug, PartialEq)]
pub enum Part {
    Text(String),
    /// A formula the user can insert into a cell.
    Formula(String),
    /// Column filters the user can apply.
    Filter(Vec<FilterItem>),
    /// Any other code block.
    Code(String),
}

/// Split a reply into text, formulas, filters and code blocks.
pub fn parse_reply(reply: &str) -> Vec<Part> {
    let mut parts = Vec::new();
    let mut rest = reply;
    while let Some(start) = rest.find("```") {
        push_text(&mut parts, &rest[..start]);
        let after = &rest[start + 3..];
        let (tag, body) = after.split_once('\n').unwrap_or(("", after));
        let (body, next) = match body.find("```") {
            Some(end) => (&body[..end], &body[end + 3..]),
            None => (body, ""),
        };
        let tag = tag.trim().to_ascii_lowercase();
        let body = body.trim();
        let part = if tag == "filter" || (body.starts_with('{') && body.contains("\"filters\"")) {
            match parse_filters(body) {
                Ok(f) if !f.is_empty() => Part::Filter(f),
                _ => Part::Code(body.to_string()),
            }
        } else if let Some(f) = (tag == "formula" || tag == "excel" || body.starts_with('='))
            .then(|| extract_formula(body))
            .flatten()
        {
            Part::Formula(f)
        } else {
            Part::Code(body.to_string())
        };
        parts.push(part);
        rest = next;
    }
    push_text(&mut parts, rest);
    parts
}

fn push_text(parts: &mut Vec<Part>, text: &str) {
    let text = text.trim();
    if !text.is_empty() {
        parts.push(Part::Text(text.to_string()));
    }
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

#[derive(Clone, Debug, PartialEq, Deserialize)]
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

    #[test]
    fn replies_split_into_parts() {
        let parts = parse_reply(
            "Sure:\n```formula\n=SUM(B2:B9)\n```\nThen filter:\n```filter\n{\"filters\":[{\"column\":\"C\",\"op\":\"eq\",\"value\":\"West\"}]}\n```\n```\nnot a formula\n```",
        );
        assert_eq!(parts.len(), 5, "{parts:?}");
        assert_eq!(parts[0], Part::Text("Sure:".into()));
        assert_eq!(parts[1], Part::Formula("=SUM(B2:B9)".into()));
        assert_eq!(parts[2], Part::Text("Then filter:".into()));
        assert!(matches!(&parts[3], Part::Filter(f) if f[0].column == "C"));
        assert_eq!(parts[4], Part::Code("not a formula".into()));
        assert_eq!(
            parse_reply("Just text."),
            vec![Part::Text("Just text.".into())]
        );
    }
}
