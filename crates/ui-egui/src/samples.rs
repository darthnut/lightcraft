//! The playground's sample IR photos (fork's IR playground): four Sony A6000 raws (590 nm) served
//! as `sample/<file>` **next to the site** — never part of the wasm module or the branch — and
//! fetched and imported on demand by the web host through the same store-then-import path a
//! dropped file takes. `SAMPLES` is the one table; the menu commands in
//! [`crate::menus::ui_commands`] restate it and a test keeps them equal.

/// One sample: its command id, the file served next to the site, and its menu name.
pub struct SamplePhoto {
    pub id: &'static str,
    pub file: &'static str,
    pub name: &'static str,
}

/// The samples, in menu order. The first is what a first visit fetches (one raw, ~25 MB: a
/// first-time visitor should not download all four).
pub const SAMPLES: &[SamplePhoto] = &[
    SamplePhoto { id: "sample.mt-hood", file: "DSC03220.ARW", name: "Mt. Hood" },
    SamplePhoto { id: "sample.forest-road", file: "DSC03584.ARW", name: "Forest Road" },
    SamplePhoto { id: "sample.river-canyon", file: "DSC03639.ARW", name: "River Canyon" },
    SamplePhoto { id: "sample.train-in-the-trees", file: "DSC03341.ARW", name: "Train in the Trees" },
];

/// The sample with command id `id`, if it is one.
pub fn sample(id: &str) -> Option<&'static SamplePhoto> {
    SAMPLES.iter().find(|s| s.id == id)
}
