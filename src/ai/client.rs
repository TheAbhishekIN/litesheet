//! A minimal OpenAI-compatible chat-completions client.

use std::sync::mpsc::Sender;

use super::{AiSettings, Request, Response, Role, SYSTEM_PROMPT};

/// How many earlier messages go with each request.
const MAX_HISTORY: usize = 20;

/// Fire off a chat completion on the tokio runtime; the answer arrives on `tx`.
pub fn spawn(
    rt: &tokio::runtime::Runtime,
    client: reqwest::Client,
    settings: AiSettings,
    key: String,
    req: Request,
    tx: Sender<Response>,
    ctx: egui::Context,
) -> tokio::task::JoinHandle<()> {
    rt.spawn(async move {
        let result = chat(&client, &settings, &key, &req).await;
        let _ = tx.send(Response { id: req.id, result });
        ctx.request_repaint();
    })
}

async fn chat(
    client: &reqwest::Client,
    settings: &AiSettings,
    key: &str,
    req: &Request,
) -> Result<String, String> {
    let mut messages = vec![
        serde_json::json!({"role": "system", "content": SYSTEM_PROMPT}),
        serde_json::json!({
            "role": "system",
            "content": format!("Snapshot of the user's spreadsheet:\n{}", req.context),
        }),
    ];
    // Keep the conversation within a sensible size.
    let skip = req.history.len().saturating_sub(MAX_HISTORY);
    for m in &req.history[skip..] {
        let role = match m.role {
            Role::User => "user",
            Role::Assistant => "assistant",
        };
        messages.push(serde_json::json!({"role": role, "content": m.text}));
    }
    let body = serde_json::json!({
        "model": settings.model,
        "temperature": 0.2,
        "messages": messages,
    });
    let url = format!(
        "{}/chat/completions",
        settings.base_url.trim_end_matches('/')
    );
    let resp = client
        .post(&url)
        .bearer_auth(key)
        .json(&body)
        .timeout(std::time::Duration::from_secs(90))
        .send()
        .await
        .map_err(|e| format!("Request failed: {e}"))?;
    let status = resp.status();
    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Bad response: {e}"))?;
    if !status.is_success() {
        let msg = json["error"]["message"].as_str().unwrap_or("unknown error");
        return Err(format!("{status}: {msg}"));
    }
    json["choices"][0]["message"]["content"]
        .as_str()
        .map(|s| s.trim().to_string())
        .ok_or_else(|| "The response had no message content".into())
}
