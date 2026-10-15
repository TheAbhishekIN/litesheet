//! The AI chat end to end against a mock OpenAI-compatible server.

use egui::Modifiers;
use egui_kittest::kittest::Queryable;

use super::*;
use crate::ai::{Part, Task};

fn send(h: &mut Harness<'_, App>, text: &str) {
    h.state_mut().ai.prompt = text.into();
    h.run_steps(2);
    h.get_by_label("Send").click();
    wait_for_ai(h);
}

/// The newest user message in a chat-completions request body.
pub fn last_user_message(body: &str) -> String {
    let json: serde_json::Value = serde_json::from_str(body).unwrap_or_default();
    json["messages"]
        .as_array()
        .and_then(|m| m.iter().rev().find(|m| m["role"] == "user"))
        .and_then(|m| m["content"].as_str())
        .unwrap_or_default()
        .to_string()
}

#[test]
fn ai_chat_end_to_end() {
    let (url, last) = mock_ai_server(|body| {
        let asked = last_user_message(body);
        Some(if asked.contains("only rows") {
            "Here are the western orders with at least 10 units:\n```filter\n{\"filters\":[{\"column\":\"Region\",\"op\":\"eq\",\"value\":\"west\"},{\"column\":\"E\",\"op\":\"gte\",\"value\":\"10\"}]}\n```".into()
        } else if asked.contains("formula") {
            "Use SUMIF:\n```formula\n=SUMIF(C2:C9,\"West\",F2:F9)\n```".into()
        } else {
            "These are **sales orders**:\n- 8 orders\n- 4 regions".into()
        })
    });
    let mut h = harness(SALES_CSV);
    h.state_mut().ai.use_test_backend(&url);
    h.state_mut().ai.open = true;
    h.run_steps(2);
    // An empty chat offers suggestions.
    h.get_by_label("Ask anything about your data");
    shot(&mut h, "ai-chat-empty");

    // A filter suggestion is applied from its button.
    send(
        &mut h,
        "Show only rows from the west with at least 10 units",
    );
    let reply = h.state().ai.last_reply().unwrap();
    assert!(
        matches!(&reply.parts[1], Part::Filter(f) if f.len() == 2),
        "{:?}",
        reply.parts
    );
    assert!(
        last.lock().unwrap().contains("C (Region)"),
        "column context is sent"
    );
    h.get_by_label("Apply filter").click();
    h.run_steps(2);
    assert_eq!(h.state().view().rows.as_deref(), Some(&[0, 1, 7][..]));
    h.get_by_label_contains("Showing 2 matching rows");
    h.state_mut().clear_filters();

    // A formula goes into the cell that was selected when it was asked for.
    click(&mut h, cell(10, 5), Modifiers::NONE);
    send(&mut h, "Write a formula for the total price in the West");
    click(&mut h, cell(12, 0), Modifiers::NONE);
    h.get_by_label("Insert into F11").click();
    h.run_steps(3);
    assert_eq!(display(&h, 10, 5), "30.74");
    shot(&mut h, "ai-chat-formula");

    // Follow-ups carry the conversation and the selected cells.
    click(&mut h, cell(1, 4), Modifiers::NONE);
    click(&mut h, cell(3, 5), Modifiers::SHIFT);
    let ctx = h.ctx.clone();
    h.state_mut().ai_quick(Task::Explain, &ctx);
    wait_for_ai(&mut h);
    let body = last.lock().unwrap().clone();
    assert!(
        body.contains("Explain the selected cells (E2:F4)"),
        "{body}"
    );
    assert!(body.contains("4\\t40\\t18.5"), "{body}");
    assert!(
        body.contains("Write a formula for the total price"),
        "history is sent: {body}"
    );
    assert_eq!(h.state().ai.chat.len(), 6);
    h.get_by_label("These are sales orders:"); // **bold** markers are rendered, not shown

    // New chat starts over.
    h.get_by_label("New chat").click();
    h.run_steps(2);
    assert!(h.state().ai.chat.is_empty());
}

#[test]
fn ai_errors_can_be_retried() {
    let (url, _) = mock_ai_server(|_| None);
    let mut h = harness(SALES_CSV);
    h.state_mut().ai.use_test_backend(&url);
    h.state_mut().ai.open = true;
    send(&mut h, "Summarize this");
    let reply = h.state().ai.last_reply().unwrap();
    assert!(reply.error && reply.text.contains("401"), "{}", reply.text);
    h.get_by_label("Retry").click();
    wait_for_ai(&mut h);
    // The failed attempt is replaced, not stacked up.
    assert_eq!(h.state().ai.chat.len(), 2);
}

#[test]
fn models_that_reject_temperature_still_work() {
    let (url, last) = mock_ai_server(|body| {
        Some(if body.contains("temperature") {
            "400 Unsupported value: 'temperature' does not support 0.2 with this model. Only the default (1) value is supported.".into()
        } else {
            "It lists 8 orders.".into()
        })
    });
    let mut h = harness(SALES_CSV);
    h.state_mut().ai.use_test_backend(&url);
    // Temperature is only sent when the user sets one; here they have.
    h.state_mut().ai.settings_mut().temperature = Some(0.2);
    h.state_mut().ai.open = true;
    send(&mut h, "Summarize this sheet.");
    let reply = h.state().ai.last_reply().unwrap();
    assert!(!reply.error, "{}", reply.text);
    assert_eq!(reply.text, "It lists 8 orders.");
    assert!(!last.lock().unwrap().contains("temperature"));
}

#[test]
fn ai_settings_screen() {
    let (url, _) = mock_ai_server(|_| Some("ok".into()));
    let mut h = harness(SALES_CSV);
    h.state_mut().ai.use_test_backend(&url);
    h.state_mut().ai.open = true;
    h.state_mut().ai.show_settings = true;
    h.run_steps(2);
    h.get_by_label("Model options").click();
    h.run_steps(2);
    h.get_by_label("Reasoning effort");
    shot(&mut h, "ai-settings");
    // Defaults send no optional parameters.
    let s = h.state_mut().ai.settings_mut().clone();
    assert_eq!(
        (s.temperature, s.max_tokens, s.reasoning_effort),
        (None, None, None)
    );
}
