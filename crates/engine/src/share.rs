//! The playground's Share This IR Edit payload (fork's IR playground): the settings that make a
//! look as a compact `.lcpreset` string for the GitHub issue URL, and the metadata-free image pair
//! (a PNG for the clipboard, a JPEG as the download fallback) rendered through the ordinary export
//! path — no second renderer.

use lightcraft_catalog::PhotoId;
use lightcraft_develop::{DevelopSettings, SettingsGroup};
use serde_json::Value;

use crate::export::{ExportFormat, ExportOptions, MetadataPolicy, OutputSpace, Resize, ResizeMode, export_photo};
use crate::presets::{LCPRESET_FORMAT, PresetFile};

/// The settings groups a shared look is made of: white balance, Light, the tone curve, Color
/// (vibrance/saturation), the HSL mixer, colour grading, calibration — and the channel mixer,
/// materialized even when it is the identity (`extract_groups` uses `to_json_with_mixer`), so
/// importing a shared edit resets a mixed photo. Nothing photo-specific (no crop, masks, spots).
const SHARE_GROUPS: &[SettingsGroup] = &[
    SettingsGroup::WhiteBalance,
    SettingsGroup::Light,
    SettingsGroup::ToneCurve,
    SettingsGroup::Color,
    SettingsGroup::ColorMixer,
    SettingsGroup::ColorGrading,
    SettingsGroup::Calibration,
];

/// Drop the keys of `v` that hold exactly what `base` (the default settings, same groups) holds,
/// including objects that become empty: an absent field reads back as its default, so the payload
/// carries only the edit. Keys the default does not name are kept as they are.
fn prune_defaults(v: &mut Value, base: &Value) {
    if let (Value::Object(o), Value::Object(b)) = (v, base) {
        let keys: Vec<String> = o.keys().cloned().collect();
        for k in keys {
            // the channel mixer is kept whole, identity coefficients included: a preset that
            // cannot reset a mixed photo is exactly the hole `to_json_with_mixer` closes
            if k == "channel_mixer" {
                continue;
            }
            if let (Some(cv), Some(bv)) = (o.get_mut(&k), b.get(&k))
                && {
                    prune_defaults(cv, bv);
                    cv == bv || (cv.is_object() && cv.as_object().is_some_and(|m| m.is_empty()))
                }
            {
                o.remove(&k);
            }
        }
    }
}

/// The `.lcpreset` payload of Share This IR Edit: one preset named "Shared edit" over
/// [`SHARE_GROUPS`], default-valued fields pruned, compact (no pretty-printing — it goes into a
/// URL). Re-imports cleanly with `preset.import` / `parse_preset_file`.
pub fn ir_share_preset(d: &DevelopSettings) -> String {
    let mut look = lightcraft_develop::extract_groups(d, SHARE_GROUPS);
    prune_defaults(&mut look, &lightcraft_develop::extract_groups(&DevelopSettings::default(), SHARE_GROUPS));
    let file = PresetFile {
        format: LCPRESET_FORMAT.into(),
        version: 1,
        presets: vec![lightcraft_develop::Preset {
            id: "shared-edit".into(),
            name: "Shared edit".into(),
            group: "IR playground".into(),
            settings: look,
            favorite: false,
            builtin: false,
        }],
    };
    serde_json::to_string(&file).unwrap_or_default()
}

/// The shared-edit image: the current photo with its edit, sRGB, **no metadata at all** (no EXIF,
/// XMP, GPS, maker notes; the file name never enters the image), long edge 2048 px stepping down
/// (1600, 1200) until the PNG is under 9 MB (GitHub's image limit is 10 MB). The PNG is for the
/// clipboard (the only image type browsers reliably accept); the JPEG, quality 90, is the download
/// fallback named `lightcraft-ir-edit.jpg`.
pub struct ShareImages {
    /// For the clipboard (`image/png`, the only image type browsers reliably accept).
    pub png: Vec<u8>,
    /// The download fallback, `lightcraft-ir-edit.jpg`.
    pub jpeg: Vec<u8>,
    /// The long edge both were rendered at.
    pub long_edge: u32,
}

pub fn share_images(s: &mut crate::Session, id: PhotoId) -> Result<ShareImages, String> {
    images_under(s, id, &[2048, 1600, 1200], 9_000_000)
}

/// The shared-edit image pair, forced under `limit` bytes by stepping `edges` down. The edges and
/// the limit are parameters only so the stepping is testable.
fn images_under(s: &mut crate::Session, id: PhotoId, edges: &[u32], limit: usize) -> Result<ShareImages, String> {
    let base = |format: ExportFormat, long_edge: u32| ExportOptions {
        format,
        quality: 90,
        resize: Some(Resize { mode: ResizeMode::LongEdge, value: long_edge as f32, height: 0, dont_enlarge: true }),
        metadata: MetadataPolicy::None,
        remove_location: true,
        color_space: OutputSpace::Srgb,
        watermark: None,
        naming: "lightcraft-ir-edit".into(),
        ..ExportOptions::default()
    };
    let mut last = Vec::new();
    for long_edge in edges {
        let png = export_photo(s, id, &base(ExportFormat::Png, *long_edge), 0).map(|e| e.bytes)?;
        if png.len() < limit {
            let jpeg = export_photo(s, id, &base(ExportFormat::Jpeg, *long_edge), 0).map(|e| e.bytes)?;
            return Ok(ShareImages { png, jpeg, long_edge: *long_edge });
        }
        last = png;
    }
    Err(format!("the shared image does not fit under {} bytes even at {} px ({} bytes)", limit, edges.last().unwrap_or(&0), last.len()))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn demo() -> crate::Session {
        crate::Session::with_demo()
    }
    use serde_json::json;

    /// The shared preset re-imports and reproduces the look: the mixer (materialized even as the
    /// identity, so importing resets a mixed photo), the white balance and the colour settings —
    /// and an untouched look prunes to an (almost) empty payload that still parses.
    #[test]
    fn ir_share_preset_round_trips_the_look() {
        let mut d = DevelopSettings::default();
        d.wb.mode = lightcraft_develop::WbMode::Custom;
        d.wb.temp = 2000.0;
        d.wb.tint = -8.0;
        d.light.exposure = 1.25;
        d.curve.master = vec![lightcraft_geom::Point::new(0.1, 0.22), lightcraft_geom::Point::new(0.9, 0.85)];
        d.color.vibrance = 20.0;
        d.mixer.red.sat = 15.0;
        d.grading.shadows.hue = 210.0;
        d.calibration.red_hue = 12.0;
        d.channel_mixer.red.red = 0.0;
        d.channel_mixer.red.blue = 100.0;
        let payload = ir_share_preset(&d);
        assert!(!payload.contains('\n'), "compact, no pretty-printing");
        let parsed = crate::presets::parse_preset_file("share.lcpreset", payload.as_bytes()).expect("parses");
        let preset = parsed.first().expect("one preset");
        assert_eq!(preset.name, "Shared edit");
        assert!(!preset.builtin);
        let out = preset.apply(&DevelopSettings::default(), 1.0);
        assert_eq!(out.channel_mixer, d.channel_mixer);
        assert_eq!(out.wb.temp, 2000.0);
        assert_eq!(out.wb.tint, -8.0);
        assert_eq!(out.light.exposure, 1.25);
        assert_eq!(out.curve.master, d.curve.master);
        assert_eq!(out.color.vibrance, 20.0);
        assert_eq!(out.mixer.red.sat, 15.0);
        assert_eq!(out.grading.shadows.hue, 210.0);
        assert_eq!(out.calibration.red_hue, 12.0);
        // nothing photo-specific rides along
        for key in ["crop", "masks", "spots", "geometry", "optics", "detail"] {
            assert!(preset.settings.get(key).is_none(), "no {key}");
        }

        // an identity mixer is still written (to_json_with_mixer): importing resets a mixed photo
        let plain = DevelopSettings::default();
        let payload = ir_share_preset(&plain);
        let parsed = crate::presets::parse_preset_file("share.lcpreset", payload.as_bytes()).expect("parses");
        let preset = parsed.first().expect("one preset");
        assert!(preset.settings.get("channel_mixer").is_some(), "the identity mixer is materialized");
        let mut mixed = DevelopSettings::default();
        mixed.channel_mixer.green.green = 40.0;
        assert_eq!(preset.apply(&mixed, 1.0).channel_mixer, plain.channel_mixer, "a mixed photo resets");

        // default-valued fields are pruned: the untouched look's payload carries nothing but the
        // materialized mixer
        let look = preset.settings.as_object().expect("object");
        let heavy: Vec<&String> =
            look.keys().filter(|k| **k != "channel_mixer").filter(|k| look.get(*k).unwrap_or(&Value::Null).to_string().len() > 40).collect();
        assert!(heavy.is_empty(), "nothing default-sized rides along: {heavy:?}");
        assert!(look.contains_key("channel_mixer"));

        // and the same photo-specific groups stay out of a preset that has them set
        let mut edited = DevelopSettings::default();
        edited.crop.geometry.angle = 5.0;
        edited.spots.push(Default::default());
        let payload = ir_share_preset(&edited);
        let parsed = crate::presets::parse_preset_file("share.lcpreset", payload.as_bytes()).expect("parses");
        for key in ["crop", "spots"] {
            assert!(parsed[0].settings.get(key).is_none(), "no {key}");
        }
    }

    /// The shared image renders through the ordinary export path: sRGB, ≤ the long edge (never
    /// upscaled), no metadata at all (no EXIF, no XMP, no embedded camera profile), and the size
    /// steps down when the PNG budget is tightened.
    #[test]
    fn share_images_are_srgb_small_and_metadata_free() {
        let mut s = demo();
        let id = s.active().unwrap();
        s.execute("develop.set", &json!({"control": "channelMixer.red.green", "value": -60.0})).unwrap();
        let img = share_images(&mut s, id).expect("renders");
        assert!(img.long_edge <= 2048);
        for (bytes, what) in [(&img.png, "png"), (&img.jpeg, "jpeg")] {
            let decoded = lightcraft_codecs::decode(bytes, Default::default()).expect(what);
            assert!(decoded.width.max(decoded.height) <= img.long_edge, "{what} is at most the long edge");
            assert!(!bytes.is_empty() && bytes.len() < 9_000_000, "{what} under the budget");
            // no metadata at all: the sRGB profile may embed, nothing else
            assert!(decoded.xmp.as_deref().unwrap_or_default().is_empty(), "{what} carries no XMP");
            assert!(!bytes.windows(4).any(|w| w == b"Exif"), "{what} carries no EXIF");
            assert!(!bytes.windows(4).any(|w| w == b"GPSI" || w == b"GPS\x00"), "{what} carries no GPS");
        }
        // the stepping: a budget the largest size misses (its own PNG, minus one byte) is met lower
        // down, and the answer says which edge it landed on
        let largest = images_under(&mut s, id, &[2048], usize::MAX).expect("the largest size");
        let at_largest = largest.png.len();
        let stepped = images_under(&mut s, id, &[2048, 1600, 1200], at_largest - 1).expect("a smaller edge fits");
        assert!(stepped.long_edge < 2048, "stepped down from 2048: {}", stepped.long_edge);
        assert!(stepped.png.len() < at_largest, "the stepped PNG is the smaller one");
    }
}
