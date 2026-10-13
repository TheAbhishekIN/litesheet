//! A minimal OpenAI-compatible chat-completions client.

use std::sync::mpsc::Sender;

use super::{AiSettings, Request, Response};

/// Fire off a chat completion on the tokio runtime; the answer arrives on `tx`.
pub fn spawn(
    rt: &tokio::runtime::Runtime,
    client: reqwest::Client,
    settings: AiSettings,
    key: String,
    req: Request,
    tx: Sender<Response>,
    ctx: egui::Context,
) {
    rt.spawn(async move {
        let task = req.task;
        let result = chat(&client, &settings, &key, &req).await;
        let _ = tx.send(Response { task, result });
        ctx.request_repaint();
    });
}

async fn chat(
    client: &reqwest::Client,
    settings: &AiSettings,
    key: &str,
    req: &Request,
) -> Result<String, String> {
    let user = if req.prompt.trim().is_empty() {
        req.context.clone()
    } else {
        format!("{}\n\nRequest: {}", req.context, req.prompt.trim())
    };
    let body = serde_json::json!({
        "model": settings.model,
        "temperature": 0.2,
        "messages": [
            {"role": "system", "content": req.task.system_prompt()},
            {"role": "user", "content": user},
        ],
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
