//! Tests of the playground's sample-photo menu: the commands restate [`crate::samples::SAMPLES`]
//! exactly, they sit in Help ▸ Sample IR Photos, and they reach the host's `fetch_sample` service
//! (or say the samples are web-only when there is none).

use std::time::Duration;

use serde_json::json;

use crate::headless::Headless;
use crate::{LightcraftApp, Services};

const T: Duration = Duration::from_secs(20);

/// Every sample has its menu command, with the table's name, in Help ▸ Sample IR Photos.
#[test]
fn sample_commands_restate_the_table() {
    for s in crate::samples::SAMPLES {
        let Some(command) = crate::menus::ui_commands().find(|c| c.0 == s.id) else { panic!("{} has no command", s.id) };
        assert_eq!(command.1, s.name, "{} names the table's entry", s.id);
        assert_eq!(command.3, "Help>Sample IR Photos", "{} sits in the submenu", s.id);
    }
    assert!(crate::menus::ui_commands().any(|c| c.0 == "sample.all"), "All Samples exists");
    // nothing else borrows the sample namespace
    assert_eq!(crate::menus::ui_commands().filter(|c| c.0.starts_with("sample.")).count(), crate::samples::SAMPLES.len() + 1);
}

/// Help ▸ Sample IR Photos holds one item per sample plus All Samples, in table order.
#[test]
fn help_menu_lists_the_samples() {
    let services = Services { png: None, ..Default::default() };
    let app = LightcraftApp::new(lightcraft_engine::Session::with_demo(), services);
    let bar = crate::menubar::menu_bar(&app);
    let help = bar.iter().find(|(title, _)| title == "Help").expect("Help menu").1.iter();
    let mut items = Vec::new();
    for node in help {
        if let crate::menubar::MenuNode::Submenu { label, children } = node
            && label == "Sample IR Photos"
        {
            items.extend(children.iter().filter_map(|n| if let crate::menubar::MenuNode::Item { id, .. } = n { Some(id.clone()) } else { None }));
        }
    }
    let want: Vec<&str> = crate::samples::SAMPLES.iter().map(|s| s.id).chain(std::iter::once("sample.all")).collect();
    assert_eq!(items, want, "the submenu in table order plus All Samples");
}

/// Without the host's `fetch_sample` (the desktop, tests) the commands answer that the samples are
/// part of the web playground; with one, the right samples are asked for — All Samples asks for
/// all four.
#[test]
fn sample_commands_reach_the_fetch_service() {
    let services = Services { png: None, ..Default::default() };
    let mut app = LightcraftApp::new(lightcraft_engine::Session::with_demo(), services);
    let refused = app.run("sample.mt-hood", json!({})).map(|_| ()).is_err_and(|e| e.contains("web playground"));
    assert!(refused, "the desktop answer says the samples are web-only");

    let fetched: std::sync::Arc<std::sync::Mutex<Vec<&'static str>>> = std::sync::Arc::default();
    let sink = fetched.clone();
    let services = Services {
        png: None,
        fetch_sample: Some(Box::new(move |s| {
            sink.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push(s.file);
            Ok(())
        })),
        ..Default::default()
    };
    let mut app = LightcraftApp::new(lightcraft_engine::Session::with_demo(), services);
    app.run("sample.mt-hood", json!({})).expect("one sample");
    assert_eq!(*fetched.lock().unwrap_or_else(std::sync::PoisonError::into_inner), vec!["DSC03220.ARW"]);
    app.run("sample.all", json!({})).expect("all samples");
    let all = fetched.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
    assert_eq!(&all[1..], ["DSC03220.ARW", "DSC03584.ARW", "DSC03639.ARW", "DSC03341.ARW"], "All Samples asks for all four, in table order: {all:?}");
    // the menu reaches the same handler in a running UI
    let mut h = Headless::new(app, [900.0, 700.0], 1.0);
    let r = h.request("engine.execute", json!({"command": "sample.river-canyon"}), T);
    assert_eq!(r["ok"], true, "{r}");
    let _ = &mut h;
}
