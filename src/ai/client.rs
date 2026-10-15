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
    let mut body = serde_json::json!({
        "model": settings.model,
        "messages": messages,
    });
    if let Some(t) = settings.temperature {
        body["temperature"] = t.into();
    }
    if let Some(n) = settings.max_tokens {
        body[settings.provider.max_tokens_param()] = n.into();
    }
    if let Some(e) = settings.reasoning_effort {
        body["reasoning_effort"] = e.as_str().into();
    }
    let url = format!("{}/chat/completions", base(settings));
    // A model may reject an option it doesn't support (OpenAI's reasoning
    // models only take the default temperature, for example). Drop it and
    // ask again rather than failing.
    loop {
        match post(client, settings, &url, key, &body).await {
            Err((400, msg)) => {
                let lower = msg.to_lowercase();
                let option = OPTIONAL_PARAMS
                    .iter()
                    .find(|p| body.get(**p).is_some() && lower.contains(**p));
                match option {
                    Some(p) => {
                        body.as_object_mut().map(|b| b.remove(*p));
                    }
                    None => return Err(msg),
                }
            }
            r => return r.map_err(|(_, msg)| msg),
        }
    }
}

/// Request options a model may refuse, which are safe to leave out.
const OPTIONAL_PARAMS: [&str; 4] = [
    "temperature",
    "reasoning_effort",
    "max_completion_tokens",
    "max_tokens",
];

fn base(settings: &AiSettings) -> &str {
    settings.base_url.trim().trim_end_matches('/')
}

fn authorize(
    req: reqwest::RequestBuilder,
    settings: &AiSettings,
    key: &str,
) -> reqwest::RequestBuilder {
    if key.is_empty() {
        return req;
    }
    let req = req.bearer_auth(key);
    if settings.provider == super::Provider::Anthropic {
        // Anthropic's own endpoints (like the model list) use these headers.
        req.header("x-api-key", key)
            .header("anthropic-version", "2023-06-01")
    } else {
        req
    }
}

/// Send one chat-completions request. Errors carry the HTTP status (0 if
/// the request never got an answer) and a message for the user.
async fn post(
    client: &reqwest::Client,
    settings: &AiSettings,
    url: &str,
    key: &str,
    body: &serde_json::Value,
) -> Result<String, (u16, String)> {
    let resp = authorize(client.post(url), settings, key)
        .json(body)
        .timeout(std::time::Duration::from_secs(
            settings.timeout_secs.max(10),
        ))
        .send()
        .await
        .map_err(|e| (0, request_error(&e)))?;
    let status = resp.status();
    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| (status.as_u16(), format!("Bad response: {e}")))?;
    if !status.is_success() {
        return Err((status.as_u16(), error_message(status, &json)));
    }
    json["choices"][0]["message"]["content"]
        .as_str()
        .map(|s| s.trim().to_string())
        .ok_or_else(|| {
            (
                status.as_u16(),
                "The response had no message content".into(),
            )
        })
}

fn request_error(e: &reqwest::Error) -> String {
    if e.is_timeout() {
        "The request timed out. Try again, or raise the timeout in AI Settings.".into()
    } else if e.is_connect() {
        format!("Couldn't connect to the AI provider: {e}")
    } else {
        format!("Request failed: {e}")
    }
}

fn error_message(status: reqwest::StatusCode, json: &serde_json::Value) -> String {
    let msg = json["error"]["message"]
        .as_str()
        .or_else(|| json["error"].as_str())
        .or_else(|| json["message"].as_str())
        .unwrap_or("unknown error");
    let hint = match status.as_u16() {
        401 | 403 => " Check the API key in AI Settings.",
        404 => " Check the model name and base URL in AI Settings.",
        429 => " The provider is rate limiting requests or the account is out of credit.",
        _ => "",
    };
    format!("{status}: {msg}{hint}")
}

/// The models the provider offers, from its `/models` endpoint.
pub async fn list_models(
    client: &reqwest::Client,
    settings: &AiSettings,
    key: &str,
) -> Result<Vec<String>, String> {
    let url = format!("{}/models", base(settings));
    let resp = authorize(client.get(&url), settings, key)
        .timeout(std::time::Duration::from_secs(20))
        .send()
        .await
        .map_err(|e| request_error(&e))?;
    let status = resp.status();
    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Bad response: {e}"))?;
    if !status.is_success() {
        return Err(error_message(status, &json));
    }
    let mut models: Vec<String> = json["data"]
        .as_array()
        .or_else(|| json["models"].as_array())
        .map(|list| {
            list.iter()
                .filter_map(|m| m["id"].as_str().or_else(|| m["name"].as_str()))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    models.sort();
    models.dedup();
    if models.is_empty() {
        return Err("The provider didn't list any models".into());
    }
    Ok(models)
}
