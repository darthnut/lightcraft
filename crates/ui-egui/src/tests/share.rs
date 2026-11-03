//! Tests of Share This IR Edit (the fork's IR playground): the issue URL's encoding, field ids and
//! length cap with its fallbacks, and the dialog end to end — the image renders while the dialog is
//! open (not on the press, which a browser would treat as a pop-up by then), the press does only
//! instant things in the order a browser needs, and the dialog stays open on an "Almost done" step
//! that says what to do on GitHub and what to do about a blocked tab or a refused clipboard.

use std::time::Duration;

use serde_json::json;

use crate::headless::Headless;
use crate::share::{self, FILTERS, ISSUE_URL, ShareStep, URL_LIMIT};
use crate::state::Dialog;
use crate::{LightcraftApp, Services};

const T: Duration = Duration::from_secs(30);

/// Percent-decode a query value back to text (test-side only).
fn decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(v) = u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or(""), 16)
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

/// The URL names the issue form's field ids, encodes everything, and stays under the cap.
#[test]
fn issue_url_encodes_and_names_the_fields() {
    let settings = r#"{"format":"lightcraft.preset","version":1,"presets":[{"id":"shared-edit","name":"Shared edit","group":"IR playground","settings":{"channel_mixer":{"red":{"red":0,"blue":100}}}}]}"#;
    let (url, dropped) = share::issue_url("590 nm", "a & b = c?#", Some("Sony A6000"), Some("E 16-50mm"), settings);
    assert_eq!(dropped, share::Dropped { curve: false, mixer_and_grading: false }, "nothing dropped");
    assert!(url.len() < URL_LIMIT, "{}", url.len());
    assert!(url.starts_with(ISSUE_URL), "the issue form template is named: {url}");
    let q = &url[ISSUE_URL.len()..];
    for field in ["&title=", "&filter=", "&camera=", "&lens=", "&notes=", "&settings="] {
        assert!(q.contains(field), "the form's field is filled: {q}");
    }
    let value = |k: &str| decode(q.split(&format!("&{k}=")).nth(1).unwrap_or("").split('&').next().unwrap_or(""));
    assert_eq!(value("title"), "Shared edit: Sony A6000, 590 nm");
    assert_eq!(value("filter"), "590 nm");
    assert_eq!(value("camera"), "Sony A6000");
    assert_eq!(value("lens"), "E 16-50mm");
    assert_eq!(value("notes"), "a & b = c?#", "the tricky characters round-trip");
    assert!(value("settings").starts_with(r#"{"format":"lightcraft.preset"#), "the payload is JSON");
    // no camera: the title falls back and the fields stay empty
    let (url, _) = share::issue_url("Other / unsure", "", None, None, settings);
    let q = &url[ISSUE_URL.len()..];
    let value = |k: &str| decode(q.split(&format!("&{k}=")).nth(1).unwrap_or("").split('&').next().unwrap_or(""));
    assert_eq!(value("title"), "Shared edit: IR photo, Other / unsure");
    assert_eq!(value("camera"), "");
    assert_eq!(value("lens"), "");
}

/// Past the cap the payload sheds the tone curve first, then the colour mixer and grading, and the
/// notes say what went; a payload that cannot fit sheds both and still answers.
#[test]
fn issue_url_caps_the_payload_and_says_what_it_dropped() {
    let big = || format!("\"{}\"", "A".repeat(10_000));
    // the curve alone pushes past the cap; dropping it leaves a small payload
    let curve_and_more = json!({"presets": [{"settings": {"curve": {"pad": big()}, "light": {"exposure": 1}}}]}).to_string();
    let (url, dropped) = share::issue_url("590 nm", "", None, None, &curve_and_more);
    assert!(url.len() < URL_LIMIT, "{}", url.len());
    assert_eq!(dropped, share::Dropped { curve: true, mixer_and_grading: false }, "the curve went");
    assert!(decode(&query_of(&url, "notes")).contains("tone curve"), "the note says so");
    assert!(query_of(&url, "settings").contains("light"), "the rest stayed");

    // curve + mixer + grading: dropping the curve is not enough, so the mixer and grading follow
    let both =
        json!({"presets": [{"settings": {"curve": {"pad": big()}, "mixer": {"pad": big()}, "grading": {"pad": big()}, "light": {"exposure": 1}}}]})
            .to_string();
    let (url, dropped) = share::issue_url("590 nm", "", None, None, &both);
    assert_eq!(dropped, share::Dropped { curve: true, mixer_and_grading: true }, "curve, mixer and grading went");
    assert!(decode(&query_of(&url, "notes")).contains("colour mixer"), "the note says so");

    // a payload that cannot fit even then (the fat group is one nothing drops) is returned as the
    // last pass, still over the cap
    let never =
        json!({"presets": [{"settings": {"curve": {"pad": big()}, "mixer": {"pad": big()}, "grading": {"pad": big()}, "light": {"pad": big()}}}]})
            .to_string();
    let (url, dropped) = share::issue_url("590 nm", "", None, None, &never);
    assert_eq!(dropped, share::Dropped { curve: true, mixer_and_grading: true });
    assert!(url.len() >= URL_LIMIT, "over the cap, but answered: {}", url.len());
}

/// What the fake host services recorded, in the order the app called them.
#[derive(Default)]
struct Recorder {
    order: std::sync::Arc<std::sync::Mutex<Vec<&'static str>>>,
    urls: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    clipboard_sizes: std::sync::Arc<std::sync::Mutex<Vec<usize>>>,
    written: std::sync::Arc<std::sync::Mutex<Vec<(String, usize)>>>,
}

/// Lock one of the recorder's lists whatever it holds.
fn locked<T>(v: &std::sync::Arc<std::sync::Mutex<T>>) -> std::sync::MutexGuard<'_, T> {
    v.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The fake services of a host that can open tabs, write the clipboard and download files. Each
/// call is recorded in `rec`, in order; `blocked` makes the tab fail as a browser does when it
/// blocks a pop-up, `no_clipboard` makes the clipboard write fail.
fn recording_services(rec: &Recorder, blocked: bool, no_clipboard: bool) -> Services {
    let (order, urls, clipboard, written) = (rec.order.clone(), rec.urls.clone(), rec.clipboard_sizes.clone(), rec.written.clone());
    let order_for_clipboard = order.clone();
    let order_for_write = order.clone();
    Services {
        png: None,
        open_url: Some(Box::new(move |url: &str| {
            locked(&order).push("open");
            locked(&urls).push(url.to_string());
            if blocked { Err(crate::links::BLOCKED_TAB.to_string()) } else { Ok(()) }
        })),
        write_clipboard: Some(Box::new(move |png: &[u8]| {
            locked(&order_for_clipboard).push("clipboard");
            locked(&clipboard).push(png.len());
            if no_clipboard { Err("no clipboard".into()) } else { Ok(()) }
        })),
        write: Some(Box::new(move |path: &str, bytes: &[u8]| {
            locked(&order_for_write).push("write");
            locked(&written).push((path.to_string(), bytes.len()));
            Ok(())
        })),
        ..Default::default()
    }
}

impl Recorder {
    fn order(&self) -> Vec<&'static str> {
        self.order.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone()
    }
    fn urls(&self) -> Vec<String> {
        self.urls.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone()
    }
    fn written(&self) -> Vec<(String, usize)> {
        self.written.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone()
    }
}

/// The demo app with the dialog's services installed, on a photo with a channel-mixer edit.
fn sharing_app(services: Services) -> Headless {
    let mut h = Headless::new(LightcraftApp::new(lightcraft_engine::Session::with_demo(), services), [1000.0, 800.0], 1.0);
    h.request("ui.set", json!({"view": "detail", "right": "edit", "openSections": ["channelMixer"]}), T);
    let r = h.request("engine.execute", json!({"command": "develop.set", "params": {"control": "channelMixer.red.blue", "value": 100.0}}), T);
    assert_eq!(r["ok"], true, "{r}");
    h
}

/// The dialog, end to end: the image renders while the dialog is open — not on the press, which by
/// then has lost the user's activation and would be a pop-up — and the press does only instant
/// things, in the order a browser needs: the clipboard write, the tab, the JPEG download. The
/// dialog then stays open on its "Almost done" step, which says what to do on GitHub.
#[test]
fn share_dialog_opens_the_issue_with_image_and_settings() {
    let rec = Recorder::default();
    let mut h = sharing_app(recording_services(&rec, false, false));
    let r = h.request("engine.execute", json!({"command": "dialog.shareIrEdit"}), T);
    assert_eq!(r["ok"], true, "{r}");
    assert!(matches!(h.app.ui.dialog, Some(Dialog::ShareIrEdit { .. })), "the dialog is open");
    h.settle(T); // the sizing pass, the first painted frame, then the render
    assert_eq!(h.app.ui.share.renders, 1, "the image rendered while the dialog was open");
    assert!(h.app.ui.share.images.is_some(), "the rendered pair is kept for the press");

    let r = h.request("ui.dialog.confirm", json!({}), T);
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(h.app.ui.share.renders, 1, "the press renders nothing: the pair was ready before it");
    assert_eq!(rec.order(), vec!["clipboard", "open", "write"], "the clipboard, then the tab, then the download");
    let url = rec.urls().first().expect("the issue opened").clone();
    assert!(url.starts_with(ISSUE_URL), "{url}");
    assert!(decode(&query_of(&url, "settings")).contains("\"channel_mixer\""), "the payload carries the mixer");
    let clipboard = rec.clipboard_sizes.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
    assert_eq!(clipboard.len(), 1, "one clipboard write");
    assert!(clipboard[0] > 1000, "a real PNG: {}", clipboard[0]);
    let written = rec.written();
    assert_eq!(written.len(), 1, "one download");
    assert_eq!(written[0].0, "lightcraft-ir-edit.jpg", "the fallback's name");
    assert!(written[0].1 > 1000, "a real JPEG: {}", written[0].1);

    // the dialog stays open on the Almost done step and says what to do there
    assert!(matches!(h.app.ui.dialog, Some(Dialog::ShareIrEdit { .. })), "the dialog stays open");
    assert_eq!(h.app.ui.share.step, ShareStep::AlmostDone);
    let text = h.painted_text().join("\n");
    assert!(text.contains("Almost done: finish on GitHub."), "{text}");
    assert!(text.contains("press Ctrl+V (⌘V on a Mac)"), "the paste step is said: {text}");
    assert!(!text.contains("Your browser blocked"), "nothing was blocked: {text}");
}

/// Open GitHub Again — the Almost done step's way back for a visitor whose browser closed or
/// blocked the tab — opens the same URL from its fresh click and writes the clipboard again, but
/// puts no second copy of the download in Downloads; Done closes the dialog.
#[test]
fn open_github_again_opens_the_same_url_and_done_closes() {
    let rec = Recorder::default();
    let mut h = sharing_app(recording_services(&rec, false, false));
    h.request("engine.execute", json!({"command": "dialog.shareIrEdit"}), T);
    h.settle(T); // the pair is prepared before the press
    let r = h.request("ui.dialog.confirm", json!({}), T);
    assert_eq!(r["ok"], true, "{r}");

    // Open GitHub Again: the same URL from this fresh click, the clipboard written again, and no
    // second copy of the download
    let r = h.request("ui.dialog.confirm", json!({}), T);
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(rec.order(), vec!["clipboard", "open", "write", "clipboard", "open"], "Again copies and opens again");
    let urls = rec.urls();
    assert_eq!(urls.len(), 2, "the tab was asked for twice");
    assert_eq!(urls[0], urls[1], "the same URL again");
    assert_eq!(rec.written().len(), 1, "Again puts no second copy in Downloads");
    assert_eq!(h.app.ui.share.renders, 1, "Again renders nothing either");
    assert_eq!(h.app.ui.share.step, ShareStep::AlmostDone, "the dialog stays on the step");

    // Done closes it
    let r = h.request("ui.dialog.cancel", json!({}), T);
    assert_eq!(r["ok"], true, "{r}");
    assert!(h.app.ui.dialog.is_none(), "Done closed the dialog");
}

/// The press is held off while the pair renders — the whole point of preparing it ahead is that the
/// press has nothing slow left to do — and unticking the image lets it go at once. The render waits
/// for the dialog's first *painted* frame (a newly shown window's first pass only sizes it), so
/// the visitor watches "Preparing your image…" while it works, not a blank screen.
#[test]
fn the_press_waits_while_the_image_prepares() {
    let rec = Recorder::default();
    let mut h = sharing_app(recording_services(&rec, false, false));
    h.app.run("dialog.shareIrEdit", json!({})).expect("the dialog opens");
    h.step(); // the sizing pass: the window is measured, nothing is on screen yet
    assert_eq!(h.app.ui.share.renders, 0, "the sizing pass renders nothing");
    h.step(); // the dialog paints; the render is armed for the next frame
    let dlg = h.app.ui.dialog.clone().expect("the dialog is open");
    assert_eq!(h.app.ui.share.renders, 0, "the first painted frame renders nothing");
    assert!(h.painted_text().iter().any(|t| t.contains("Preparing your image")), "the dialog says it is preparing");
    assert!(!crate::dialogs::can_confirm(&h.app, &dlg, false), "Open on GitHub is held off while preparing");
    // unticking the image needs no render at all: the press goes at once
    if let Some(Dialog::ShareIrEdit { image, .. }) = &mut h.app.ui.dialog {
        *image = false;
    }
    let dlg = h.app.ui.dialog.clone().unwrap();
    assert!(crate::dialogs::can_confirm(&h.app, &dlg, false), "unticking the image enables it at once");
    if let Some(Dialog::ShareIrEdit { image, .. }) = &mut h.app.ui.dialog {
        *image = true;
    }
    let dlg = h.app.ui.dialog.clone().unwrap();
    assert!(!crate::dialogs::can_confirm(&h.app, &dlg, false), "ticking it back waits for the pair");
    h.step(); // the render runs
    assert_eq!(h.app.ui.share.renders, 1, "the pair rendered while the dialog was open");
    let dlg = h.app.ui.dialog.clone().unwrap();
    assert!(crate::dialogs::can_confirm(&h.app, &dlg, false), "ready: the press goes");
    assert!(rec.order().is_empty(), "nothing was opened or written before any press");
    h.step(); // the frame the render finished painted its own last state; this one shows the ready one
    assert!(h.painted_text().iter().any(|t| t.contains("Your image will be copied")), "the hint replaces the preparing line");
}

/// A tab the browser blocked is a state the dialog reports in red — the press opened the clipboard
/// and the download anyway, and Open GitHub Again is the way out — not a failure.
#[test]
fn a_blocked_tab_is_reported_in_the_dialog() {
    let rec = Recorder::default();
    let mut h = sharing_app(recording_services(&rec, true, false));
    h.request("engine.execute", json!({"command": "dialog.shareIrEdit"}), T);
    h.settle(T); // the pair is prepared before the press
    let r = h.request("ui.dialog.confirm", json!({}), T);
    assert_eq!(r["ok"], true, "{r}");
    assert!(matches!(h.app.ui.dialog, Some(Dialog::ShareIrEdit { .. })), "the dialog stays open to explain");
    assert_eq!(rec.order(), vec!["clipboard", "open", "write"], "the clipboard and the fallback download happened anyway");
    let text = h.painted_text().join("\n");
    assert!(text.contains("Almost done: finish on GitHub."), "{text}");
    assert!(text.contains("Your browser blocked the GitHub tab. Press Open GitHub Again, or allow pop-ups for this site."), "{text}");
    assert!(text.contains("No GitHub tab? Your browser may have blocked it."), "{text}");
    // Open GitHub Again is the confirm, on the same URL
    let r = h.request("ui.dialog.confirm", json!({}), T);
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(rec.urls().len(), 2, "the tab was asked for again");
    assert_eq!(h.app.ui.share.step, ShareStep::AlmostDone, "the dialog stays on the step");
}

/// A clipboard the browser refuses is said in the dialog: the JPEG download (which the press made
/// anyway) is dragged into GitHub's "Your image" box instead.
#[test]
fn a_refused_clipboard_points_at_the_download() {
    let rec = Recorder::default();
    let mut h = sharing_app(recording_services(&rec, false, true));
    h.request("engine.execute", json!({"command": "dialog.shareIrEdit"}), T);
    h.settle(T); // the pair is prepared before the press
    let r = h.request("ui.dialog.confirm", json!({}), T);
    assert_eq!(r["ok"], true, "{r}");
    assert!(matches!(h.app.ui.dialog, Some(Dialog::ShareIrEdit { .. })), "the dialog stays open to explain");
    assert_eq!(rec.order(), vec!["clipboard", "open", "write"], "the failed write, the tab, then the fallback download");
    let text = h.painted_text().join("\n");
    assert!(text.contains("Couldn't copy the image: drag lightcraft-ir-edit.jpg from your Downloads into the 'Your image' box instead."), "{text}");
    assert!(!text.contains("press Ctrl+V"), "the paste step is not offered without a copy: {text}");
    let written = rec.written();
    assert_eq!(written.len(), 1, "the fallback was downloaded");
    assert_eq!(written[0].0, "lightcraft-ir-edit.jpg");
}

/// Without the host's clipboard (the desktop build) the dialog hides its image checkbox and no
/// image is made — the settings-only issue still opens, and the dialog still says what is left on
/// GitHub.
#[test]
fn share_dialog_without_a_clipboard_opens_without_an_image() {
    let opened: std::sync::Arc<std::sync::Mutex<Vec<String>>> = std::sync::Arc::default();
    let written: std::sync::Arc<std::sync::Mutex<Vec<(String, usize)>>> = std::sync::Arc::default();
    let (o, w) = (opened.clone(), written.clone());
    let services = Services {
        png: None,
        open_url: Some(Box::new(move |url: &str| {
            o.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push(url.to_string());
            Ok(())
        })),
        write: Some(Box::new(move |path: &str, bytes: &[u8]| {
            w.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push((path.to_string(), bytes.len()));
            Ok(())
        })),
        ..Default::default()
    };
    let mut h = Headless::new(LightcraftApp::new(lightcraft_engine::Session::with_demo(), services), [1000.0, 800.0], 1.0);
    let r = h.request("engine.execute", json!({"command": "dialog.shareIrEdit"}), T);
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(h.app.ui.share.renders, 0, "without a clipboard, nothing renders");
    let r = h.request("ui.dialog.confirm", json!({}), T);
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(opened.lock().unwrap_or_else(std::sync::PoisonError::into_inner).len(), 1, "the issue opened");
    assert!(written.lock().unwrap_or_else(std::sync::PoisonError::into_inner).is_empty(), "no image was made");
    assert!(matches!(h.app.ui.dialog, Some(Dialog::ShareIrEdit { .. })), "the dialog stays open");
    let text = h.painted_text().join("\n");
    assert!(text.contains("Almost done: finish on GitHub."), "{text}");
    assert!(text.contains("In the GitHub tab: press Create at the bottom."), "no image step is offered: {text}");
    let r = h.request("ui.dialog.cancel", json!({}), T);
    assert_eq!(r["ok"], true, "{r}");
    assert!(h.app.ui.dialog.is_none(), "Done closed the dialog");
    // and with no open_url at all (tests): the dialog says it cannot open links
    let mut h = Headless::new(
        LightcraftApp::new(lightcraft_engine::Session::with_demo(), Services { png: None, ..Default::default() }),
        [1000.0, 800.0],
        1.0,
    );
    h.request("engine.execute", json!({"command": "dialog.shareIrEdit"}), T);
    let r = h.request("ui.dialog.confirm", json!({}), T);
    assert_eq!(r["ok"], false, "{r}");
    assert!(r["error"].as_str().unwrap_or_default().contains("open links"), "{r}");
    assert!(h.app.ui.dialog.is_none(), "a dialog that cannot open links closes");
    let _ = FILTERS;
}

/// The value of query key `k` in `url`, still encoded.
fn query_of(url: &str, k: &str) -> String {
    url.split(&format!("&{k}=")).nth(1).unwrap_or("").split('&').next().unwrap_or("").to_string()
}
