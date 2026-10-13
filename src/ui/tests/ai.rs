//! AI features end to end against a mock OpenAI-compatible server.

use egui::Modifiers;
use egui_kittest::kittest::Queryable;

use super::*;

#[test]
fn ai_filter_and_formula_end_to_end() {
    let (url, last) = mock_ai_server(|body| {
        Some(if body.contains("natural-language filter") {
            "```json\n{\"filters\":[{\"column\":\"Region\",\"op\":\"eq\",\"value\":\"west\"},{\"column\":\"E\",\"op\":\"gte\",\"value\":\"10\"}]}\n```".into()
        } else if body.contains("Excel formulas") {
            "=SUMIF(C2:C9,\"West\",F2:F9)".into()
        } else {
            "These are sales orders.".into()
        })
    });
    let mut h = harness(SALES_CSV);
    h.state_mut().ai.use_test_backend(&url);
    h.state_mut().ai.open = true;
    h.run_steps(2);

    // Plain-English filter.
    h.state_mut().ai.task = crate::ai::Task::Filter;
    h.state_mut().ai.prompt = "west orders with at least 10 units".into();
    h.run_steps(2);
    h.get_by_label("Apply Filter").click();
    wait_for_ai(&mut h);
    let result = h.state().ai.result_text();
    assert!(
        matches!(&result, Some(Ok(t)) if t.contains("Showing 2")),
        "{result:?}"
    );
    assert_eq!(h.state().view().rows.as_deref(), Some(&[0, 1, 7][..]));
    // The model got column context, not the whole sheet.
    assert!(last.lock().unwrap().contains("C (Region)"));
    shot(&mut h, "09-ai-filter");

    // Formula generation, then insert into the target cell.
    h.state_mut().clear_filters();
    click(&mut h, cell(10, 5), Modifiers::NONE);
    h.state_mut().ai.task = crate::ai::Task::Formula;
    h.state_mut().ai.prompt = "total price for West".into();
    h.run_steps(2);
    h.get_by_label("Generate").click();
    wait_for_ai(&mut h);
    assert_eq!(
        h.state().ai.result_text(),
        Some(Ok("=SUMIF(C2:C9,\"West\",F2:F9)".into()))
    );
    h.get_by_label("Insert into F11").click();
    h.run_steps(3);
    assert_eq!(display(&h, 10, 5), "30.74");
    shot(&mut h, "10-ai-formula");

    // Explain the selection: the selected cells are sent, and the answer shown.
    click(&mut h, cell(1, 4), Modifiers::NONE);
    click(&mut h, cell(3, 5), Modifiers::SHIFT);
    h.state_mut().ai.task = crate::ai::Task::Explain;
    h.state_mut().ai.prompt.clear();
    h.run_steps(2);
    h.get_by_label("Explain Selection").click();
    wait_for_ai(&mut h);
    assert_eq!(
        h.state().ai.result_text(),
        Some(Ok("These are sales orders.".into()))
    );
    let body = last.lock().unwrap().clone();
    assert!(body.contains("Selected range E2:F4"), "{body}");
    assert!(body.contains("4\\t40\\t18.5"), "{body}");
}
