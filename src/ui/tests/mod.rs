//! Headless UI tests. They drive the real [`App`] through `egui_kittest`
//! with mouse and keyboard events. Set `LITESHEET_SHOTS=<dir>` to also render
//! each test's frames to PNG with wgpu.

mod ai;
mod docs_assets;
mod documents;
mod interaction;
mod perf;
mod updates;

use std::sync::{Arc, Mutex};

use egui::{Event, Key, Modifiers, PointerButton, Pos2, pos2};
use egui_kittest::Harness;

use super::app::App;

pub const SALES_CSV: &str = "Order,Date,Region,Product,Units,Price,Shipped
1001,2024-03-14,West,Widget,12,19.99,Yes
1002,2024-01-09,East,Gadget,3,120,No
1003,2024-07-22,North,Widget,40,18.5,Yes
1004,2024-02-02,West,Sprocket,7,7.25,No
1005,2024-11-30,South,Gizmo,25,42,Yes
1006,2024-05-18,East,Widget,9,19.99,Yes
1007,2024-09-01,West,Doohickey,16,3.5,No
1008,2024-12-12,North,Gadget,2,115,Yes
";

pub fn harness(csv: &str) -> Harness<'static, App> {
    let dir =
        std::env::temp_dir().join(format!("litesheet-ui-{}-{}", std::process::id(), rand_id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("sales.csv");
    std::fs::write(&path, csv).unwrap();
    let book = crate::io::load(&path).unwrap();
    let mut builder = Harness::builder().with_size([1100.0, 560.0]);
    // Only spin up a GPU renderer when screenshots are wanted (CI may have no GPU).
    if std::env::var_os("LITESHEET_SHOTS").is_some() {
        builder = builder.wgpu();
    }
    let mut h = builder.build_eframe(move |cc| {
        let mut app = App::new(cc, None);
        app.set_book(book);
        app
    });
    h.run_steps(3);
    h
}

fn rand_id() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    N.fetch_add(1, Ordering::Relaxed)
}

pub fn shot(h: &mut Harness<'_, App>, name: &str) {
    if let Ok(dir) = std::env::var("LITESHEET_SHOTS") {
        h.run_steps(2); // pick up state changed directly through state_mut()
        let img = h.render().expect("render");
        img.save(std::path::Path::new(&dir).join(format!("{name}.png")))
            .unwrap();
    }
}

pub fn click(h: &mut Harness<'_, App>, p: Pos2, modifiers: Modifiers) {
    h.event(Event::ModifiersChanged(modifiers));
    h.event(Event::PointerMoved(p));
    h.event(Event::PointerButton {
        pos: p,
        button: PointerButton::Primary,
        pressed: true,
        modifiers,
    });
    h.step();
    h.event(Event::PointerButton {
        pos: p,
        button: PointerButton::Primary,
        pressed: false,
        modifiers,
    });
    h.run_steps(2);
    h.event(Event::ModifiersChanged(Modifiers::NONE));
    h.step();
}

pub fn type_text(h: &mut Harness<'_, App>, text: &str) {
    h.event(Event::Text(text.to_string()));
    h.run_steps(2);
}

pub fn key(h: &mut Harness<'_, App>, key: Key, modifiers: Modifiers) {
    h.key_press_modifiers(modifiers, key);
    h.run_steps(2);
}

/// Vertical centre of the column-header strip with the default layout
/// (menu bar, formatting toolbar and formula bar above it).
pub const HEADER_Y: f32 = 105.0;

/// Screen centre of cell (row, col) with the default layout.
pub fn cell(r: u32, c: u32) -> Pos2 {
    pos2(100.0 + 100.0 * c as f32, HEADER_Y + 22.0 * (r + 1) as f32)
}

pub fn display(h: &Harness<'_, App>, r: u32, c: u32) -> String {
    h.state().sheet().display((r, c))
}

pub fn wait_for_ai(h: &mut Harness<'_, App>) {
    for _ in 0..200 {
        h.step();
        if !h.state().ai.is_busy() {
            h.run_steps(2);
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    panic!("AI request timed out");
}

/// A fake OpenAI-compatible endpoint on localhost. `reply` maps each request
/// body to the assistant's message (or `None` for a 401). The last request
/// body is recorded so tests can check what was sent.
pub fn mock_ai_server(
    reply: impl Fn(&str) -> Option<String> + Send + 'static,
) -> (String, Arc<Mutex<String>>) {
    use std::io::{BufRead, BufReader, Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let last = Arc::new(Mutex::new(String::new()));
    let last2 = last.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let mut stream = stream.unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let (mut len, mut auth) = (0, String::new());
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                let lower = line.to_lowercase();
                if let Some(v) = lower.strip_prefix("content-length:") {
                    len = v.trim().parse().unwrap();
                }
                if lower.starts_with("authorization:") {
                    auth = line.trim().to_string();
                }
                if line == "\r\n" {
                    break;
                }
            }
            let mut body = vec![0; len];
            reader.read_exact(&mut body).unwrap();
            let body = String::from_utf8(body).unwrap();
            *last2.lock().unwrap() = body.clone();
            let content = auth
                .ends_with("Bearer test-key")
                .then(|| reply(&body))
                .flatten();
            let (status, json) = match content {
                Some(c) => (
                    "200 OK",
                    serde_json::json!({"choices": [{"message": {"role": "assistant", "content": c}}]}),
                ),
                None => (
                    "401 Unauthorized",
                    serde_json::json!({"error": {"message": "bad key"}}),
                ),
            };
            let json = json.to_string();
            let resp = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{json}",
                json.len()
            );
            stream.write_all(resp.as_bytes()).unwrap();
        }
    });
    (url, last)
}
