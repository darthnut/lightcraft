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

/// Open the prefilled issue (the dialog's Open on GitHub): the image goes to the clipboard first —
/// a browser only allows a clipboard write right after a user gesture, and the new tab takes the
/// focus away — then the JPEG download (the fallback for browsers that refuse the clipboard),
/// then the tab. A clipboard refusal does not stop the issue from opening; it becomes the returned
/// note for the dialog to toast.
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

    // the image: PNG to the clipboard (while the press is still the active user gesture), JPEG as
    // a download either way; a clipboard refusal does not stop the issue opening
    let mut note = String::new();
    if image && app.services.write_clipboard.is_some() {
        match lightcraft_engine::share::share_images(&mut app.session, id) {
            Ok(images) => {
                if let Some(clipboard) = app.services.write_clipboard.as_mut()
                    && clipboard(&images.png).is_err()
                {
                    note = "Couldn't copy the image: drag lightcraft-ir-edit.jpg from your Downloads into the 'Your image' box.".into();
                }
                if let Some(write) = app.services.write.as_mut() {
                    let _ = write("lightcraft-ir-edit.jpg", &images.jpeg);
                }
            }
            Err(e) => note = format!("No image was made to share ({e})."),
        }
    }

    let opened = crate::links::open(app, &url);
    if !note.is_empty() {
        // the issue opened anyway: the note is the toast, not a failure
        let _ = opened;
        return Err(note);
    }
    opened
}

#[cfg(test)]
mod tests {
    #[test]
    fn probe() {
        let pad = "A".repeat(10_000);
        let payload = serde_json::json!({"presets": [{"settings": {"curve": {"pad": pad}, "light": {"exposure": 1}}}]}).to_string();
        let (url, dropped) = super::issue_url("590 nm", "", None, None, &payload);
        println!("PROBE len={} dropped={:?}", url.len(), dropped);
        let mut v: serde_json::Value = serde_json::from_str(&payload).unwrap();
        let before = v.to_string().len();
        let matched = v
            .get_mut("presets")
            .and_then(serde_json::Value::as_array_mut)
            .and_then(|a| a.first_mut())
            .and_then(serde_json::Value::as_object_mut)
            .is_some();
        if let Some(o) =
            v.get_mut("presets").and_then(serde_json::Value::as_array_mut).and_then(|a| a.first_mut()).and_then(serde_json::Value::as_object_mut)
        {
            o.remove("curve");
        }
        println!("STRIP matched={} {} -> {}", matched, before, v.to_string().len());
    }
}
