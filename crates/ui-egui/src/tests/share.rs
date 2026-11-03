//! Tests of Share This IR Edit (the fork's IR playground): the issue URL's encoding, field ids and
//! length cap with its fallbacks, and the dialog end to end — open it, confirm it, and see the URL,
//! the clipboard PNG and the JPEG download it produces.

use std::time::Duration;

use serde_json::json;

use crate::headless::Headless;
use crate::share::{self, FILTERS, ISSUE_URL, URL_LIMIT};
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

/// The dialog, end to end: opened from the command, confirmed, it renders the edited photo through
/// the export path, puts the PNG on the clipboard, downloads the JPEG, and opens the prefilled
/// issue (here: captured by a fake host). Without the host's clipboard the image checkbox was
/// hidden and no image is made.
#[test]
fn share_dialog_opens_the_issue_with_image_and_settings() {
    let opened: std::sync::Arc<std::sync::Mutex<Vec<String>>> = std::sync::Arc::default();
    let clipboard: std::sync::Arc<std::sync::Mutex<Vec<usize>>> = std::sync::Arc::default();
    let written: std::sync::Arc<std::sync::Mutex<Vec<(String, usize)>>> = std::sync::Arc::default();
    let (o, c, w) = (opened.clone(), clipboard.clone(), written.clone());
    let services = Services {
        png: None,
        open_url: Some(Box::new(move |url: &str| {
            o.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push(url.to_string());
            Ok(())
        })),
        write_clipboard: Some(Box::new(move |png: &[u8]| {
            c.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push(png.len());
            Ok(())
        })),
        write: Some(Box::new(move |path: &str, bytes: &[u8]| {
            w.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push((path.to_string(), bytes.len()));
            Ok(())
        })),
        ..Default::default()
    };
    let mut h = Headless::new(LightcraftApp::new(lightcraft_engine::Session::with_demo(), services), [1000.0, 800.0], 1.0);
    h.request("ui.set", json!({"view": "detail", "right": "edit", "openSections": ["channelMixer"]}), T);
    h.request("engine.execute", json!({"command": "develop.set", "params": {"control": "channelMixer.red.blue", "value": 100.0}}), T);
    let r = h.request("engine.execute", json!({"command": "dialog.shareIrEdit"}), T);
    assert_eq!(r["ok"], true, "{r}");
    assert!(matches!(h.app.ui.dialog, Some(Dialog::ShareIrEdit { .. })), "the dialog is open");

    let r = h.request("ui.dialog.confirm", json!({}), T);
    assert_eq!(r["ok"], true, "{r}");
    assert!(h.app.ui.dialog.is_none(), "confirming closed it");
    let opened = opened.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let url = opened.first().expect("the issue opened");
    assert!(url.starts_with(ISSUE_URL), "{url}");
    assert!(decode(&query_of(url, "settings")).contains("\"channel_mixer\""), "the payload carries the mixer");
    assert_eq!(opened.len(), 1, "one tab");
    let clipboard = clipboard.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    assert_eq!(clipboard.len(), 1, "one clipboard write");
    assert!(clipboard[0] > 1000, "a real PNG: {}", clipboard[0]);
    let written = written.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    assert_eq!(written.len(), 1, "one download");
    assert_eq!(written[0].0, "lightcraft-ir-edit.jpg", "the fallback's name");
    assert!(written[0].1 > 1000, "a real JPEG: {}", written[0].1);
}

/// Without the host's clipboard (the desktop build) the dialog hides its image checkbox and no
/// image is made — the settings-only issue still opens.
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
    let r = h.request("ui.dialog.confirm", json!({}), T);
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(opened.lock().unwrap_or_else(std::sync::PoisonError::into_inner).len(), 1, "the issue opened");
    assert!(written.lock().unwrap_or_else(std::sync::PoisonError::into_inner).is_empty(), "no image was made");
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
    let _ = FILTERS;
}

/// The value of query key `k` in `url`, still encoded.
fn query_of(url: &str, k: &str) -> String {
    url.split(&format!("&{k}=")).nth(1).unwrap_or("").split('&').next().unwrap_or("").to_string()
}
