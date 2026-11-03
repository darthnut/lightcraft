//! The fork's IR playground guidance: the start-here card above the preset groups, the IR recipe
//! groups' place and open state, the share buttons, and a brand-new library's first-visit layout.

use serde_json::json;

use crate::headless::Headless;
use crate::{LightcraftApp, Services};

const T: std::time::Duration = std::time::Duration::from_secs(10);

/// The demo app with the Presets column open and laid out.
fn app() -> Headless {
    let app = LightcraftApp::new(lightcraft_engine::Session::with_demo(), Services { png: None, ..Default::default() });
    let mut h = Headless::new(app, [1400.0, 1000.0], 1.0);
    h.app.run("panel.presets", json!({})).unwrap();
    for _ in 0..5 {
        h.step();
    }
    h
}

/// Where the widget registered as `id` was drawn in the last frame, if it was.
fn rect(h: &Headless, id: &str) -> Option<egui::Rect> {
    h.app.widgets.iter().find(|(w, _)| w == id).map(|(_, r)| *r)
}

/// The group headers from the top of the panel down, without the `presetGroup:` prefix.
fn headers(h: &Headless) -> Vec<String> {
    let mut v: Vec<(String, egui::Rect)> = h
        .app
        .widgets
        .iter()
        .filter(|(w, _)| w.starts_with("presetGroup:"))
        .map(|(w, r)| (w.strip_prefix("presetGroup:").unwrap().to_string(), *r))
        .collect();
    v.sort_by(|a, b| a.1.top().total_cmp(&b.1.top()));
    v.into_iter().map(|(w, _)| w).collect()
}

/// A click on a widget by automation id, plus the steps that let it land.
fn click(h: &mut Headless, id: &str) -> serde_json::Value {
    let r = h.request("ui.clickWidget", json!({"id": id}), T);
    h.step();
    h.step();
    r
}

/// The IR recipe groups come right after Favorites (before it exists: first of all), before the
/// stock groups by name; until the visitor toggles one, the IR groups and the user's own groups
/// are open and the stock groups are folded.
#[test]
fn ir_recipe_groups_lead_and_stock_groups_start_folded() {
    let mut h = app();
    let groups = headers(&h);
    assert_eq!(groups.len(), 13, "three IR groups and the ten stock ones: {groups:?}");
    assert_eq!(groups[0], "IR Recipes 1 · Classic", "the recipes lead: {groups:?}");
    assert_eq!(groups[1], "IR Recipes 2 · Wild");
    assert_eq!(groups[2], "IR Recipes 3 · Silly");
    assert!(groups[3..].windows(2).all(|w| w[0] <= w[1]), "then the rest by name: {:?}", &groups[3..]);
    let row = |h: &Headless, id: &str| rect(h, id).is_some();
    // the IR groups start open (their recipes are listed)…
    assert!(row(&h, "preset:ir-recipe-classic-swap"), "the first IR group is open");
    assert!(row(&h, "preset:ir-recipe-mars"), "the last IR group is open");
    // …and the stock groups start folded (none of their presets are listed)
    assert!(!h.app.widgets.iter().any(|(w, _)| w.starts_with("preset:lc.")), "no stock group's presets are listed");
    // a click folds an IR group, and it stays folded
    assert_eq!(click(&mut h, "presetGroup:IR Recipes 2 · Wild")["ok"], true);
    assert!(!row(&h, "preset:ir-recipe-aerochrome"), "the click folded the IR group");
    // a click opens a stock group, and it stays open
    assert_eq!(click(&mut h, "presetGroup:Color")["ok"], true);
    assert!(row(&h, "preset:lc.warm-glow"), "the click opened the Color group");
    h.step();
    h.step();
    assert!(row(&h, "preset:lc.warm-glow"), "and it stays open");
    // a user's own group defaults to open, like the IR groups
    let made = h.app.run("preset.create", json!({"name": "Mine", "group": "My Looks"})).unwrap();
    h.step();
    h.step();
    let made = format!("preset:{}", made["id"].as_str().unwrap_or_default());
    assert!(h.app.widgets.iter().any(|(w, _)| *w == made), "the user group is open");
}

/// Starring a preset puts the Favorites section first, above the IR recipes.
#[test]
fn favorites_stay_above_the_ir_recipes() {
    let mut h = app();
    let id = h.app.session.presets.first().map(|p| p.id.clone()).unwrap();
    h.app.run("preset.favorite", json!({"id": id})).unwrap();
    h.step();
    h.step();
    let groups = headers(&h);
    assert_eq!(groups[0], "IR Recipes 1 · Classic", "the group rows keep their order: {groups:?}");
    let favorites = rect(&h, "presetFavorites").expect("the section is there");
    assert!(
        h.app.widgets.iter().filter(|(w, _)| w.starts_with("presetGroup:")).all(|(_, r)| favorites.top() <= r.top()),
        "the Favorites header is above every group"
    );
}

/// The start-here card is always at the top of the Presets panel, above the groups, with its share
/// button opening the dialog on a photo and taking no clicks without one.
#[test]
fn the_start_here_card_shares_the_edit() {
    let mut h = app();
    let card = rect(&h, "presetStartCard").expect("the card is drawn");
    let first_header =
        h.app.widgets.iter().filter(|(w, _)| w.starts_with("presetGroup:")).map(|(_, r)| *r).min_by(|a, b| a.top().total_cmp(&b.top())).unwrap();
    assert!(card.bottom() <= first_header.top(), "the card sits above the groups: {card:?} / {first_header:?}");
    let button = rect(&h, "button:shareIrEditCard").expect("the card's share button");
    assert!(card.contains_rect(button), "the button is in the card");
    assert!(card.width() < 300.0, "the card leaves the panel its width: {card:?}");
    // on the demo photo it opens the share dialog
    assert_eq!(click(&mut h, "button:shareIrEditCard")["ok"], true);
    assert!(matches!(h.app.ui.dialog, Some(crate::state::Dialog::ShareIrEdit { .. })), "the dialog is open");
    // with no active photo the button is inert (the command is still in the Photo menu)
    h.app.ui.dialog = None;
    h.app.session.selection.ids.clear();
    h.app.session.selection.active = None;
    h.step();
    h.step();
    assert!(rect(&h, "button:shareIrEditCard").is_some(), "the card stays without a photo");
    let _ = click(&mut h, "button:shareIrEditCard");
    assert!(h.app.ui.dialog.is_none(), "a disabled share button opens nothing");
}

/// The card stays compact: on a 1080p window with the panel docked the IR recipes still start
/// above the fold (the whole point of the card is to lead down to them), and the demo photo's
/// grid keeps its rows in view too.
#[test]
fn the_card_leaves_the_ir_recipes_above_the_fold_at_1080p() {
    let app = LightcraftApp::new(lightcraft_engine::Session::with_demo(), Services { png: None, ..Default::default() });
    let mut h = Headless::new(app, [1920.0, 1080.0], 1.0);
    h.app.run("panel.presets", json!({})).unwrap();
    for _ in 0..5 {
        h.step();
    }
    let card = rect(&h, "presetStartCard").expect("the card is drawn");
    assert!(card.height() < 350.0, "the card stays compact: {card:?}");
    let first = rect(&h, "presetGroup:IR Recipes 1 · Classic").expect("the first recipe group");
    assert!(first.bottom() < 1080.0 - 40.0, "the first recipe group is on screen: {first:?}");
    assert!(rect(&h, "preset:ir-recipe-classic-swap").is_some(), "its first recipes are listed");
    assert!(rect(&h, "presetGroup:IR Recipes 2 · Wild").is_some(), "and the next group is too");
}

/// A brand-new library's first-visit layout opens the Presets column and the Channel Mixer, and
/// leaves the rest of the layout alone (a second call adds nothing).
#[test]
fn first_visit_layout_opens_the_presets_column_and_the_mixer() {
    let mut ui = crate::state::UiState { presets: false, ..Default::default() };
    crate::panels::presets::first_visit_layout(&mut ui);
    assert!(ui.presets, "the Presets column is open");
    assert!(ui.section_open("channelMixer"), "the Channel Mixer is expanded");
    assert!(ui.section_open("light"), "the sections the layout started with are left alone");
    ui.presets = false;
    crate::panels::presets::first_visit_layout(&mut ui);
    assert_eq!(ui.open_sections.iter().filter(|s| *s == "channelMixer").count(), 1, "no duplicate entry");
}

/// The top bar's Share IR Edit button sits beside the right icons without running into the search
/// field or its filter icon — at 1280 wide in both menu layouts — and a window too narrow for
/// that drops it instead.
#[test]
fn the_top_bar_share_button_fits_beside_the_icons() {
    for (width, native_menu) in [1280.0f32, 1600.0].into_iter().flat_map(|w| [(w, false), (w, true)]) {
        let mut a = LightcraftApp::new(lightcraft_engine::Session::with_demo(), Services { png: None, ..Default::default() });
        a.native_menu = native_menu;
        a.integrated_titlebar = native_menu;
        let mut h = Headless::new(a, [width, 800.0], 1.0);
        h.step();
        h.step();
        let the = |h: &Headless, id: &str| rect(h, id).unwrap_or_else(|| panic!("{id} at {width} (native menu {native_menu})"));
        let (button, filter, appearance) = (the(&h, "button:shareIrEditTop"), the(&h, "icon:filter"), the(&h, "icon:appearance"));
        assert!(!button.intersects(filter), "{width} (native menu {native_menu}): {button:?} meets the filter icon {filter:?}");
        assert!(!button.intersects(appearance), "{width} (native menu {native_menu}): {button:?} meets the appearance icon {appearance:?}");
        assert!(button.right() < appearance.left(), "the button is the leftmost of the right group");
    }
    // narrow: the button gives up its place rather than pushing into the field
    let a = LightcraftApp::new(lightcraft_engine::Session::with_demo(), Services { png: None, ..Default::default() });
    let mut h = Headless::new(a, [900.0, 800.0], 1.0);
    h.step();
    h.step();
    assert!(rect(&h, "button:shareIrEditTop").is_none(), "no button at 900 wide");
    let filter = rect(&h, "icon:filter").unwrap();
    let appearance = rect(&h, "icon:appearance").unwrap();
    assert!(!filter.intersects(appearance), "the bar is laid out as before");
}
