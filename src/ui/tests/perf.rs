//! Timings for a large sheet. Run in release mode:
//! `cargo test --release perf_big_file -- --ignored --nocapture`

use egui::{Event, Modifiers};

use super::*;

/// `cargo test --release perf_big_file -- --ignored --nocapture`
#[test]
#[ignore]
fn perf_big_file() {
    use std::fmt::Write;
    use std::time::Instant;
    let mut csv = String::from("id,name,value,score,total\n");
    for i in 0..200_000u32 {
        writeln!(csv, "{i},row{i},{}.5,{},", i % 977, i % 101).unwrap();
    }
    let t = Instant::now();
    let mut h = harness(&csv);
    eprintln!("load 200k rows x5 cols + first frames: {:?}", t.elapsed());

    // Add a formula column and recalc.
    let t = Instant::now();
    h.state_mut().book.set_input(0, (1, 4), "=C2*D2");
    eprintln!("single edit + recalc: {:?}", t.elapsed());
    let t = Instant::now();
    h.state_mut().sel = crate::ui::Selection {
        anchor: (1, 4),
        cursor: (20_000, 4),
    };
    h.state_mut().fill(true);
    eprintln!("fill 20k formulas: {:?}", t.elapsed());
    let t = Instant::now();
    h.state_mut().book.set_input(0, (1, 2), "1000");
    eprintln!("edit with 20k dependents: {:?}", t.elapsed());

    let t = Instant::now();
    h.state_mut().set_cursor((150_000, 2), false);
    h.run_steps(3);
    eprintln!("jump to row 150k (3 frames): {:?}", t.elapsed());
    let t = Instant::now();
    for _ in 0..30 {
        h.event(Event::MouseWheel {
            unit: egui::MouseWheelUnit::Line,
            delta: egui::vec2(0.0, -5.0),
            phase: egui::TouchPhase::Move,
            modifiers: Modifiers::NONE,
        });
        h.step();
    }
    eprintln!(
        "30 scroll frames: {:?} ({:?}/frame)",
        t.elapsed(),
        t.elapsed() / 30
    );
    let t = Instant::now();
    h.state_mut().sort_by(3, true);
    eprintln!("sort 200k rows: {:?}", t.elapsed());
    let t = Instant::now();
    h.state_mut().views[0].search = "row1999".into();
    h.state_mut().data_changed();
    eprintln!(
        "search 1M cells: {:?} ({} matches)",
        t.elapsed(),
        h.state().view().matches.len()
    );
    shot(&mut h, "11-big");
}
