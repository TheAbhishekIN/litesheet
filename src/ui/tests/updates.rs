//! The update button in the menu bar.

use egui_kittest::kittest::Queryable;

use super::*;
use crate::ui::updates::UpdateState;
use crate::update::Release;

fn release(version: &str) -> Release {
    Release {
        version: version.into(),
        notes: "Faster loading".into(),
        page: "https://github.com/TheAbhishekIN/litesheet/releases/tag/v9.9.9".into(),
        download: Some("https://example.invalid/LiteSheet-macos.zip".into()),
        size: 1,
    }
}

#[test]
fn a_newer_release_shows_in_the_menu_bar() {
    let mut h = harness(SALES_CSV);
    // Tests never run an update check of their own.
    assert!(matches!(h.state().updater.state, UpdateState::Idle));

    // Not running from an installed app bundle, so it can't install itself:
    // it offers the release instead.
    h.state_mut().fake_update_check(release("9.9.9"), false);
    h.run_steps(2);
    h.get_by_label("⬆ LiteSheet 9.9.9 available");

    // Once downloaded, the button restarts into the new version.
    h.state_mut().updater.state = UpdateState::Ready(release("9.9.9"), "/tmp/LiteSheet.app".into());
    h.run_steps(2);
    h.get_by_label("⬆ Restart to update to 9.9.9");
    shot(&mut h, "update-ready");
}

#[test]
fn up_to_date_and_older_releases_are_ignored() {
    let mut h = harness(SALES_CSV);
    h.state_mut()
        .fake_update_check(release(crate::update::current_version()), true);
    h.run_steps(2);
    assert!(matches!(h.state().updater.state, UpdateState::Idle));
    h.get_by_label_contains("You're up to date");
    h.state_mut().fake_update_check(release("0.0.1"), false);
    assert!(matches!(h.state().updater.state, UpdateState::Idle));
}
