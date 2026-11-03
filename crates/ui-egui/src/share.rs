//! Share This IR Edit (fork's IR playground): a prefilled GitHub issue for the
//! lightcraft-ir-recipes repo — the URL built here, the settings payload and the metadata-free
//! image from `lightcraft_engine::share`. English strings only (this branch runs with `tr()`
//! fallback).

use serde_json::Value;

use crate::state::Dialog;

use crate::LightcraftApp;

/// The issue form this opens (`share-edit.yml` in darthnut/lightcraft-ir-recipes).
pub const ISSUE_URL: &str = "https://github.com/darthnut/lightcraft-ir-recipes/issues/new?template=share-edit.yml";

/// The Filter dropdown's choices, as the issue form's plain-text field wants them.
pub const FILTERS: &[&str] = &["590 nm", "665 nm", "720 nm", "850 nm", "Full spectrum + filter", "Other / unsure"];

/// GitHub's practical URL ceiling for the `new issue` form; we keep a margin under it.
pub const URL_LIMIT: usize = 7000;

/// Percent-encode for a URL query value: everything but the unreserved set, as UTF-8 `%XX`.
fn encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// What the builder dropped from the settings payload to fit the URL cap.
#[derive(Debug, PartialEq)]
pub struct Dropped {
    pub curve: bool,
    pub mixer_and_grading: bool,
}

impl Dropped {
    fn none() -> Self {
        Dropped { curve: false, mixer_and_grading: false }
    }

    /// The sentence added to `notes` about what was dropped.
    fn note(&self) -> String {
        match (self.curve, self.mixer_and_grading) {
            (true, true) => "Dropped from the attached settings to fit the URL: the tone curve, the colour mixer and the colour grading.".into(),
            (true, false) => "Dropped from the attached settings to fit the URL: the tone curve.".into(),
            _ => String::new(),
        }
    }
}

/// The issue URL with every field filled: `title`, `filter`, `camera`, `lens`, `notes`, `settings`
/// (the issue form's field ids). `settings` is the `.lcpreset` payload; if the URL would pass
/// [`URL_LIMIT`], the payload's curve group goes first, then the mixer and grading groups, and
/// `notes` says what was dropped.
pub fn issue_url(filter: &str, notes: &str, camera: Option<&str>, lens: Option<&str>, settings: &str) -> (String, Dropped) {
    let mut dropped = Dropped::none();
    let settings_json: Value = serde_json::from_str(settings).unwrap_or(Value::Null);
    let strip = |mut v: Value, keys: &[&str]| -> Value {
        if let Some(o) = v
            .get_mut("presets")
            .and_then(Value::as_array_mut)
            .and_then(|a| a.first_mut())
            .and_then(|preset| preset.get_mut("settings"))
            .and_then(Value::as_object_mut)
        {
            for k in keys {
                o.remove(*k);
            }
        }
        v
    };
    let mut url = String::new();
    for pass in 0..3 {
        let payload = match pass {
            0 => settings_json.clone(),
            1 => {
                dropped.curve = true;
                strip(settings_json.clone(), &["curve"])
            }
            _ => {
                dropped.mixer_and_grading = true;
                strip(settings_json.clone(), &["curve", "mixer", "grading"])
            }
        };
        let settings = serde_json::to_string(&payload).unwrap_or_default();
        let notes = if dropped.note().is_empty() { notes.to_string() } else { format!("{} {}", notes, dropped.note()) };
        let title = match camera {
            Some(c) => format!("Shared edit: {c}, {filter}"),
            None => format!("Shared edit: IR photo, {filter}"),
        };
        url = format!(
            "{ISSUE_URL}&title={}&filter={}&camera={}&lens={}&notes={}&settings={}",
            encode(&title),
            encode(filter),
            encode(camera.unwrap_or("")),
            encode(lens.unwrap_or("")),
            encode(notes.trim()),
            encode(&settings),
        );
        if url.len() < URL_LIMIT || pass == 2 {
            return (url, dropped);
        }
    }
    (url, dropped)
}

/// What the Share This IR Edit dialog shows: the form, or the steps left once a press opened the
/// issue (the dialog stays open to say them).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ShareStep {
    /// Filter, notes and the include boxes; the image pair renders while it is shown.
    #[default]
    Compose,
    /// A press opened the issue: paste the image, press Create — and Open GitHub Again when the
    /// browser blocked the tab.
    AlmostDone,
}

/// The Share This IR Edit dialog's state beyond its form ([`Dialog::ShareIrEdit`]): the image pair
/// rendered while the dialog is open, so the Open on GitHub press does only instant things — a
/// browser lets a click alone write the clipboard and open a tab while it is still fresh. It lives
/// in [`crate::state::UiState`], not in the dialog, which is cloned per frame; it is reset when the
/// dialog opens and when it closes.
#[derive(Clone, Default)]
pub struct ShareState {
    /// The step the dialog is on.
    pub step: ShareStep,
    /// The rendered pair, once ready (an `Arc`: the UI state is cloned when it is saved).
    pub images: Option<std::sync::Arc<lightcraft_engine::share::ShareImages>>,
    /// The render starts on the next frame: the frame that arms it paints the dialog and its
    /// "Preparing your image…" line before the seconds-long render blocks the next one.
    pub render_due: bool,
    /// Why no image could be rendered (said in the dialog; the share goes on without the image).
    pub render_error: Option<String>,
    /// The last press found the browser had blocked the tab.
    pub blocked: bool,
    /// The last press could not put the image on the clipboard (the JPEG download is the fallback).
    pub clipboard_failed: bool,
    /// The URL the first press opened; Open GitHub Again opens it again.
    pub url: Option<String>,
    /// How many times the image pair was rendered (tests assert the press renders nothing).
    pub renders: u32,
}

impl std::fmt::Debug for ShareState {
    /// The rendered bytes are a megabyte pair: say they are there, not what they are.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ShareState")
            .field("step", &self.step)
            .field("images", &self.images.as_ref().map(|i| i.png.len()))
            .field("render_due", &self.render_due)
            .field("render_error", &self.render_error)
            .field("blocked", &self.blocked)
            .field("clipboard_failed", &self.clipboard_failed)
            .field("url", &self.url.is_some())
            .field("renders", &self.renders)
            .finish()
    }
}

/// Render the shared-edit image pair for the active photo into the dialog's state. This is the slow
/// part (seconds on a big raw, through the export path), so it runs while the dialog is open —
/// [`crate::dialogs`]'s body arms it a frame after the dialog first shows.
pub(crate) fn render_image(app: &mut LightcraftApp) {
    let rendered = match app.session.active() {
        Some(id) => lightcraft_engine::share::share_images(&mut app.session, id),
        None => Err("no active photo".into()),
    };
    app.ui.share.renders += 1;
    match rendered {
        Ok(images) => app.ui.share.images = Some(std::sync::Arc::new(images)),
        Err(e) => app.ui.share.render_error = Some(e),
    }
}

/// Open the prefilled issue: the dialog's Open on GitHub, and its Open GitHub Again. The image pair
/// was rendered while the dialog was open ([`ShareState`]), so this does only instant things, in the
/// order a browser needs: the clipboard write first, while the click is still the active gesture,
/// then the tab (it takes the focus away), then the JPEG download (the fallback for a clipboard the
/// browser refuses). Nothing here toasts or closes: the dialog moves to its "Almost done" step,
/// which says what the press found — a blocked tab, a refused clipboard — because it stays open.
pub fn open_on_github(app: &mut LightcraftApp, dlg: &Dialog) -> Result<Value, String> {
    let Some(id) = app.session.active() else {
        return Err("no active photo".into());
    };
    // the dialog is already taken off `app.ui` when this runs (the confirm path holds it): take
    // the fields from it, not from the app
    let Dialog::ShareIrEdit { filter, notes, camera, image } = dlg else {
        return Err("no Share This IR Edit dialog".into());
    };
    let (filter, notes, camera, image) = (*filter, notes.clone(), *camera, *image);

    // The issue URL, built once (the dialog is modal: nothing it is built from can change) and
    // reused by Open GitHub Again, so "again" opens the very same URL.
    let url = match &app.ui.share.url {
        Some(url) => url.clone(),
        None => {
            let d = app.session.develop_of(id).unwrap_or_default();
            let settings = lightcraft_engine::share::ir_share_preset(&d);

            // camera = make + model as the app already reads it, lens = the lens model
            let meta = app.session.catalog.photo(id).map(|p| (p.meta.camera.clone(), p.meta.lens.clone()));
            let (camera_string, lens_string) = if camera {
                match meta {
                    Some((c, l)) => {
                        let c = c.trim().to_string();
                        let l = l.trim().to_string();
                        ((!c.is_empty()).then_some(c), (!l.is_empty()).then_some(l))
                    }
                    None => (None, None),
                }
            } else {
                (None, None)
            };
            let filter = FILTERS[filter.min(FILTERS.len() - 1)];
            let (url, _dropped) = issue_url(filter, &notes, camera_string.as_deref(), lens_string.as_deref(), &settings);
            app.ui.share.url = Some(url.clone());
            url
        }
    };

    // what this press found, for the step the dialog moves to
    app.ui.share.blocked = false;
    app.ui.share.clipboard_failed = false;
    // The pair renders while the dialog is open; a press through the control channel can arrive
    // before a frame has (the button waits), so render it now rather than share without the image
    // the visitor asked to include.
    if image && app.services.write_clipboard.is_some() && app.ui.share.images.is_none() && app.ui.share.render_error.is_none() {
        render_image(app);
    }
    // 1. the clipboard write, while the click is still the active user gesture
    let clipboard_failed = image
        && match (app.services.write_clipboard.as_mut(), app.ui.share.images.as_ref()) {
            (Some(clipboard), Some(images)) => clipboard(&images.png).is_err(),
            _ => false,
        };
    app.ui.share.clipboard_failed = clipboard_failed;
    // 2. the tab, before the download takes the pointer's attention; a blocked one is a state the
    // dialog reports (its Open GitHub Again opens it from a fresh click), not a failure
    if let Err(e) = crate::links::open(app, &url) {
        if e.contains(crate::links::BLOCKED_TAB) {
            app.ui.share.blocked = true;
        } else {
            return Err(e);
        }
    }
    // 3. the JPEG download, the fallback for a browser that refuses the clipboard — once, on the
    // first press (Again would only put another copy in Downloads)
    if app.ui.share.step == ShareStep::Compose
        && let Some(write) = app.services.write.as_mut()
        && let Some(images) = app.ui.share.images.as_ref()
    {
        let _ = write("lightcraft-ir-edit.jpg", &images.jpeg);
    }
    app.ui.share.step = ShareStep::AlmostDone;
    Ok(serde_json::json!({ "url": url }))
}
