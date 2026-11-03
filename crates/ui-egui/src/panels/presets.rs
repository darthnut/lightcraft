//! The Presets column (opens to the left of the Edit panel): grouped presets with hover preview
//! (the loupe shows the photo with the preset while the pointer rests on it; nothing is
//! committed), optional live thumbnails, amount slider, create/favourite, import/export of preset
//! files.

use std::collections::BTreeMap;
use std::sync::Arc;

use egui::{Align2, Color32, Rect, Sense, pos2, vec2};
use lightcraft_develop::{ControlSpec, Preset, Section, Track};
use lightcraft_engine::presets::PresetList;
use serde_json::json;

use crate::icons::{Icon, paint};
use crate::theme::Tokens;
use crate::widgets::{divider, icon_button, register, slider};
use crate::{HoverPreview, LightcraftApp};

/// Long edge of preset thumbnails (px).
const THUMB_EDGE: usize = 128;

/// The preset amount slider may replace only the undo step it created.
#[derive(Clone)]
struct AppliedPreset {
    id: String,
    amount: f64,
    selected: Vec<lightcraft_catalog::PhotoId>,
    active: Option<lightcraft_catalog::PhotoId>,
    commits: u64,
}

impl AppliedPreset {
    fn new(session: &lightcraft_engine::Session, id: String, amount: f64) -> Self {
        Self { id, amount, selected: session.selection.ids.clone(), active: session.active(), commits: session.commits() }
    }

    fn is_current(&self, session: &lightcraft_engine::Session) -> bool {
        self.selected == session.selection.ids && self.active == session.active() && !self.is_stale(session)
    }

    /// A later edit, undo or redo (on any photo) owns the undo step: the slider may never replace it.
    fn is_stale(&self, session: &lightcraft_engine::Session) -> bool {
        !(self.commits == session.commits()
            && session.interaction.is_none()
            && session.redo.is_empty()
            && session
                .undo
                .last()
                .is_some_and(|entry| session.presets.iter().any(|preset| preset.id == self.id && entry.label == format!("Preset: {}", preset.name))))
    }
}

/// The preset each photo was given, with its settings right after: the panel marks it while the
/// photo still has exactly those settings, so it is still marked on coming back to the photo
/// (issue #2392) and drops once the photo is edited further.
#[derive(Clone, Default)]
struct AppliedByPhoto(std::collections::HashMap<lightcraft_catalog::PhotoId, (String, Arc<lightcraft_develop::DevelopSettings>)>);

impl AppliedByPhoto {
    const ID: &'static str = "preset-applied-by-photo";

    /// Remember `preset` on the photos `preset.apply` just changed (the selection, else the active
    /// photo; an active photo outside the selection wasn't changed and isn't marked).
    fn remember(ctx: &egui::Context, session: &lightcraft_engine::Session, preset: &str) {
        let id = egui::Id::unique(Self::ID);
        let mut map: Self = ctx.data(|d| d.get_temp(id)).unwrap_or_default();
        for photo in session.selected() {
            if let Some(d) = session.develop_of(photo) {
                map.0.insert(photo, (preset.to_string(), d));
            }
        }
        ctx.data_mut(|d| d.insert_temp(id, map));
    }

    /// The preset to mark for `photo`: the one it was given, while its settings are unchanged since.
    fn of(ctx: &egui::Context, session: &lightcraft_engine::Session, photo: Option<lightcraft_catalog::PhotoId>) -> Option<String> {
        let photo = photo?;
        let map: Self = ctx.data(|d| d.get_temp(egui::Id::unique(Self::ID)))?;
        let (preset, then) = map.0.get(&photo)?;
        let now = session.develop_of(photo)?;
        (Arc::ptr_eq(then, &now) || **then == *now).then(|| preset.clone())
    }
}

/// The Presets panel: the Favorites section of starred presets on top (issue #1999), then the
/// groups of built-in and user presets, each row applying its preset on click.
pub fn show(app: &mut LightcraftApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    egui::Frame::NONE.fill(t.chrome).stroke(egui::Stroke::new(1.0, t.divider)).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        let (hr, _) = ui.allocate_exact_size(vec2(ui.available_width(), 46.0), Sense::hover());
        ui.painter().text(pos2(hr.left() + 24.0, hr.center().y + 2.0), Align2::LEFT_CENTER, crate::i18n::tr("Presets"), t.semibold(15.0), t.text);
        let mut hdr = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(Rect::from_min_max(pos2(hr.right() - 80.0, hr.top()), hr.right_bottom()))
                .layout(egui::Layout::right_to_left(egui::Align::Center)),
        );
        if icon_button(
            &mut hdr,
            "presetCreate",
            Icon::Plus,
            vec2(28.0, 28.0),
            false,
            app.session.active().is_some(),
            crate::i18n::tr("Create Preset…"),
        )
        .clicked()
        {
            app.ui.dialog = Some(crate::state::Dialog::create_preset());
        }
        let more = icon_button(&mut hdr, "presetMore", Icon::More, vec2(28.0, 28.0), false, true, crate::i18n::tr("More preset options"));
        egui::Popup::menu(&more).show(|ui| {
            let r = ui.checkbox(&mut app.ui.preset_thumbs, crate::i18n::tr("Show Thumbnails"));
            register(ui, "presetMenu:thumbnails", r.rect);
            ui.separator();
            if ui.button(crate::i18n::tr("Import Presets…")).clicked() {
                let _ = app.run("file.importPresets", json!({}));
            }
            let any_user = app.session.presets.iter().any(|p| !p.builtin);
            if ui.add_enabled(any_user, egui::Button::new(crate::i18n::tr("Export User Presets…"))).clicked() {
                let _ = app.run("file.exportPresets", json!({}));
            }
        });
        divider(ui);
        // amount slider (applies to the last applied preset)
        let amt_id = egui::Id::unique("preset-amount");
        let last: Option<AppliedPreset> = ui.data(|d| d.get_temp(amt_id));
        if let Some(last) = last.clone().filter(|last| last.is_current(&app.session)) {
            let spec = ControlSpec {
                id: "presetAmount",
                label: "Amount",
                section: Section::Profile,
                min: 0.0,
                max: 200.0,
                default: 100.0,
                step: 1.0,
                decimals: 0,
                track: Track::Plain,
            };
            let out = slider(ui, &spec, last.amount, true, None);
            if let Some(v) = out.value
                && app.run("edit.undo", json!({})).is_ok()
            {
                // Only replace the most recent preset; never undo intervening edits.
                if app.run("preset.apply", json!({"id": &last.id, "amount": v})).is_ok() {
                    AppliedByPhoto::remember(ui.ctx(), &app.session, &last.id);
                    ui.data_mut(|d| d.insert_temp(amt_id, AppliedPreset::new(&app.session, last.id, v)));
                } else {
                    let _ = app.run("edit.redo", json!({}));
                    ui.data_mut(|d| d.remove::<AppliedPreset>(amt_id));
                }
            }
            divider(ui);
        } else if last.is_some_and(|last| last.is_stale(&app.session)) {
            // kept across a switch to another photo and back (the slider returns with it); gone once
            // a later edit, undo or redo owns the step
            ui.data_mut(|d| d.remove::<AppliedPreset>(amt_id));
        }
        // the fork's IR playground: the recipe workflow's three steps, above the groups
        start_card(app, ui);
        let active = app.session.active();
        let marked = AppliedByPhoto::of(ui.ctx(), &app.session, active);
        let current = active.and_then(|id| app.session.develop_of(id)).map(|d| (*d).clone());
        let thumbs = app.ui.preset_thumbs && active.is_some();
        let row_h = if thumbs { 54.0 } else { 26.0 };
        let groups = grouped(ui.ctx(), &app.session.presets);
        let scroll = egui::ScrollArea::vertical().id_salt("presets-scroll").auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            // Only the rows in view are laid out, drawn and given thumbnails: a Lightroom collection
            // imported whole is 10 000+ presets, and building every row each frame took seconds.
            let visible = ui.clip_rect().expand(row_h);
            for grp in groups.iter() {
                let (g, builtin, favorites) = (grp.name.clone(), grp.builtin, grp.favorites);
                // the Favorites section opens and closes apart from a user group named "Favorites"
                let open_id = egui::Id::unique(("preset-group", favorites, &g));
                // until the visitor toggles one: the Favorites section, the IR recipes and the
                // user's own groups open, the stock groups folded, so the playground's recipes are
                // the visible path
                let open: bool = ui.data(|d| d.get_temp(open_id)).unwrap_or(favorites || grp.ir || grp.user);
                let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::click());
                crate::widgets::announce(&resp, g.to_string());
                // outside the `presetGroup:` ids, which a user group of any name can take
                register(ui, if favorites { "presetFavorites".to_string() } else { format!("presetGroup:{g}") }, r);
                // the fork's IR playground: its recipe groups draw in the accent colour
                let header = if grp.ir { t.accent } else { t.text_label };
                paint(
                    ui.painter(),
                    Rect::from_center_size(pos2(r.left() + 22.0, r.center().y), vec2(16.0, 16.0)),
                    if open { Icon::ChevronDown } else { Icon::ChevronRight },
                    header,
                );
                ui.painter().text(
                    pos2(r.left() + 36.0, r.center().y),
                    Align2::LEFT_CENTER,
                    crate::i18n::builtin_label(&g, builtin),
                    t.semibold(13.0),
                    header,
                );
                if resp.clicked() {
                    ui.data_mut(|d| d.insert_temp(open_id, !open));
                }
                if !favorites {
                    crate::widgets::context_menu(&resp, |ui| {
                        if ui.button(crate::i18n::tr("Export Group…")).clicked() {
                            let _ = app.run("file.exportPresets", json!({"group": g}));
                        }
                    });
                }
                if !open {
                    continue;
                }
                let (first, end) = rows_in_view(ui.cursor().top(), row_h, grp.items.len(), visible);
                if first > 0 {
                    ui.allocate_space(vec2(ui.available_width(), first as f32 * row_h));
                }
                for &i in grp.items.get(first..end).unwrap_or_default() {
                    let Some(pr) = app.session.presets.get(i).cloned() else { continue };
                    let (pid, name, fav) = (pr.id.clone(), crate::i18n::builtin_label(&pr.name, pr.builtin).to_string(), pr.favorite);
                    let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), row_h), Sense::click());
                    crate::widgets::announce(&resp, name.clone());
                    // a starred preset is listed twice: in Favorites and in its group
                    register(ui, if favorites { format!("presetFavorite:{pid}") } else { format!("preset:{pid}") }, r);
                    let is_last = marked.as_deref() == Some(pid.as_str());
                    if is_last {
                        register(ui, "presetApplied", r);
                        ui.painter().rect_filled(r, 0.0, t.tool_active);
                    } else if resp.hovered() {
                        ui.painter().rect_filled(r, 0.0, t.hover.gamma_multiply(0.7));
                    }
                    let look = if thumbs || resp.hovered() { current.as_ref().map(|d| pr.apply(d, 1.0)) } else { None };
                    let mut text_x = r.left() + 40.0;
                    if thumbs && let (Some(id), Some(s)) = (active, look.as_ref()) {
                        let tr = Rect::from_min_size(pos2(r.left() + 36.0, r.top() + 5.0), vec2(66.0, row_h - 10.0));
                        ui.painter().rect_filled(tr, 2.0, t.canvas);
                        if let Some(job) = app.session.variant_job(id, s, THUMB_EDGE)
                            && let Some(tex) = app.renderer.variant(job)
                        {
                            ui.painter().image(tex.tex.id(), tr, cover_uv(tr, tex.size), Color32::WHITE);
                        }
                        text_x = tr.right() + 10.0;
                    }
                    ui.painter().text(pos2(text_x, r.center().y), Align2::LEFT_CENTER, &name, t.font(13.0), t.text_label);
                    if fav {
                        crate::rating::paint_star(
                            ui.painter(),
                            Rect::from_center_size(pos2(r.right() - 20.0, r.center().y), vec2(16.0, 16.0)),
                            true,
                            t.star,
                        );
                    }
                    // resting on a preset previews it in the loupe (no history entry)
                    if resp.hovered()
                        && let Some(s) = look
                    {
                        let label = crate::i18n::tr_format!("Preset: {name}", name = name);
                        app.hover_preview = Some(HoverPreview { label: label.clone(), settings: s });
                        if let Some(id) = active {
                            ui.data_mut(|d| d.insert_temp(egui::Id::unique("last-preset-hover"), (id, pr.clone(), label)));
                        }
                    }
                    if resp.clicked() && app.run("preset.apply", json!({"id": pid, "amount": 100})).is_ok() {
                        AppliedByPhoto::remember(ui.ctx(), &app.session, &pid);
                        ui.data_mut(|d| d.insert_temp(amt_id, AppliedPreset::new(&app.session, pid.clone(), 100.0)));
                        ui.data_mut(|d| d.remove::<(lightcraft_catalog::PhotoId, Preset, String)>(egui::Id::unique("last-preset-hover")));
                        app.toast(ui.ctx(), crate::i18n::tr_format!("Preset: {name}", name = name));
                    }
                    let (builtin, group) = (pr.builtin, pr.group.clone());
                    crate::widgets::context_menu(&resp, |ui| {
                        if ui.button(crate::i18n::tr(if fav { "Remove from Favorites" } else { "Add to Favorites" })).clicked() {
                            let _ = app.run("preset.favorite", json!({"id": pid}));
                        }
                        if builtin {
                            return;
                        }
                        ui.separator();
                        if ui
                            .add_enabled(app.session.active().is_some(), egui::Button::new(crate::i18n::tr("Update with Current Settings")))
                            .clicked()
                        {
                            match app.run("preset.update", json!({"id": pid})) {
                                Ok(_) => app.toast(ui.ctx(), crate::i18n::tr_format!("Updated “{name}”", name = name)),
                                Err(e) => app.toast(ui.ctx(), e),
                            }
                        }
                        if ui.button(crate::i18n::tr("Rename…")).clicked() {
                            crate::panels::dialogs::prompt(app, "Rename Preset", "Preset name", &name, "preset.rename", json!({"id": pid}), "name");
                        }
                        crate::widgets::menu_button(ui, crate::i18n::tr("Move to Group"), |ui| {
                            for g in groups.iter().filter(|x| x.user && x.name != group).map(|x| &x.name) {
                                if ui.button(g).clicked() {
                                    let _ = app.run("preset.move", json!({"id": pid, "group": g}));
                                }
                            }
                            if ui.button(crate::i18n::tr("New Group…")).clicked() {
                                crate::panels::dialogs::prompt(
                                    app,
                                    "Move Preset to New Group",
                                    "Group name",
                                    "",
                                    "preset.move",
                                    json!({"id": pid}),
                                    "group",
                                );
                            }
                        });
                        ui.separator();
                        if ui.button(crate::i18n::tr("Delete Preset")).clicked() {
                            let _ = app.run("preset.delete", json!({"id": pid}));
                        }
                    });
                }
                if end < grp.items.len() {
                    ui.allocate_space(vec2(ui.available_width(), (grp.items.len() - end) as f32 * row_h));
                }
            }
            ui.add_space(30.0);
        });
        let last_hover_id = egui::Id::unique("last-preset-hover");
        let pointer_in_scroll = ui.ctx().input(|i| i.pointer.hover_pos()).is_some_and(|pos| scroll.inner_rect.contains(pos));
        if pointer_in_scroll {
            if app.hover_preview.is_none()
                && let (Some(id), Some(curr)) = (active, current.as_ref())
                && let Some((saved_id, saved_preset, saved_label)) =
                    ui.data(|d| d.get_temp::<(lightcraft_catalog::PhotoId, Preset, String)>(last_hover_id))
                && saved_id == id
            {
                app.hover_preview = Some(HoverPreview { label: saved_label, settings: saved_preset.apply(curr, 1.0) });
            }
        } else {
            ui.data_mut(|d| d.remove::<(lightcraft_catalog::PhotoId, Preset, String)>(last_hover_id));
        }
    });
}

/// A preset group as the panel lists it: indices into the session's presets.
struct Group {
    name: String,
    /// Holds a stock preset: a stock group (shown in the UI language), even when user presets
    /// share its name; a group of user presets keeps its name.
    builtin: bool,
    /// Holds a user preset (a Move to Group target).
    user: bool,
    /// One of the fork's IR playground recipe groups ("IR Recipes …"): listed right after
    /// Favorites, opened and highlighted in the accent colour.
    ir: bool,
    /// The Favorites section on top (issue #1999): the starred presets, each also in its own group.
    favorites: bool,
    items: Vec<usize>,
}

/// The presets by group (groups sorted by name, presets in list order), rebuilt only when the
/// list changed (its generation).
fn grouped(ctx: &egui::Context, presets: &PresetList) -> Arc<Vec<Group>> {
    let id = egui::Id::unique("presets-grouped");
    if let Some((generation, groups)) = ctx.data(|d| d.get_temp::<(u64, Arc<Vec<Group>>)>(id))
        && generation == presets.generation()
    {
        return groups;
    }
    let mut by_name: BTreeMap<&str, Group> = BTreeMap::new();
    for (i, p) in presets.iter().enumerate() {
        let g = by_name.entry(&p.group).or_insert_with(|| Group {
            name: p.group.clone(),
            builtin: false,
            user: false,
            ir: p.group.starts_with("IR Recipes"),
            favorites: false,
            items: Vec::new(),
        });
        g.builtin |= p.builtin;
        g.user |= !p.builtin;
        g.items.push(i);
    }
    // the fork's IR playground recipe groups lead, before the stock groups (a visitor clicks one
    // first); `BTreeMap` order keeps them 1 · Classic, 2 · Wild, 3 · Silly and the rest by name
    let (ir, stock): (Vec<Group>, Vec<Group>) = by_name.into_values().partition(|g| g.ir);
    // starred presets first, in their own section, as Profiles does
    let starred: Vec<usize> = presets.iter().enumerate().filter(|(_, p)| p.favorite).map(|(i, _)| i).collect();
    let favorites =
        (!starred.is_empty()).then(|| Group { name: "Favorites".into(), builtin: true, user: false, ir: false, favorites: true, items: starred });
    let groups = Arc::new(favorites.into_iter().chain(ir).chain(stock).collect::<Vec<_>>());
    ctx.data_mut(|d| d.insert_temp(id, (presets.generation(), groups.clone())));
    groups
}

/// One wrapped line of the start-here card: plain and semibold runs in the panel's small fonts,
/// the strong parts standing in for the mock's bolding.
fn card_job(t: &Tokens, w: f32, parts: &[(&str, bool)]) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob { wrap: egui::text::TextWrapping::wrap_at_width(w), ..Default::default() };
    for (text, strong) in parts {
        job.append(
            crate::i18n::tr(text),
            0.0,
            egui::TextFormat { font_id: if *strong { t.semibold(11.5) } else { t.font(11.5) }, color: t.text_label, ..Default::default() },
        );
    }
    job
}

/// The fork's IR playground start-here card, always shown above the preset groups: the recipe
/// workflow in three steps, the share button and where more photos come from. Static highlights
/// only (an accent tint and border): a pulsing glow would repaint every frame on the CPU renderer,
/// and the card must stay compact — it may not push the IR groups below the fold of a 1080p window
/// with the panel docked.
fn start_card(app: &mut LightcraftApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let enabled = app.session.active().is_some();
    ui.add_space(8.0);
    let card =
        egui::Frame::NONE.fill(t.accent.gamma_multiply(0.10)).inner_margin(egui::Margin { left: 16, right: 10, top: 8, bottom: 6 }).show(ui, |ui| {
            ui.spacing_mut().item_spacing = vec2(2.0, 4.0);
            let w = ui.available_width();
            crate::widgets::announce_region(ui, egui::accesskit::Role::Note, crate::i18n::tr("IR playground: start here"));
            ui.add(egui::Label::new(egui::RichText::new(crate::i18n::tr("IR playground: start here")).font(t.semibold(13.0)).color(t.text)));
            // two ways in: the mixer itself (what the playground tests) or a recipe built on it
            ui.add(egui::Label::new(card_job(
                &t,
                w,
                &[
                    ("1. Press ", false),
                    ("Swap Red and Blue", true),
                    (" in ", false),
                    ("Channel Mixer", true),
                    (", or click a recipe under ", false),
                    ("IR Recipes", true),
                    (".", false),
                ],
            )));
            ui.add(egui::Label::new(card_job(&t, w, &[("2. Then pick: in ", false), ("Channel Mixer", true), (", click the picker", false)])));
            // the Channel Mixer's own eyedropper (`Icon::Picker`), leading the sentence it picks up
            ui.horizontal_wrapped(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
                paint(ui.painter(), r, Icon::Picker, t.accent);
                ui.add(egui::Label::new(
                    egui::RichText::new(crate::i18n::tr("then something white or grey in the photo (or what a recipe's (pick …) names)."))
                        .font(t.font(11.5))
                        .color(t.text_label),
                ));
            });
            ui.horizontal_wrapped(|ui| {
                ui.add(egui::Label::new(egui::RichText::new(crate::i18n::tr("3. Like it?")).font(t.font(11.5)).color(t.text_label)));
                if crate::widgets::accent_button(ui, "shareIrEditCard", "Share This IR Edit…", enabled).clicked()
                    && let Err(e) = app.run("dialog.shareIrEdit", json!({}))
                {
                    app.toast(ui.ctx(), e);
                }
                ui.add(egui::Label::new(
                    egui::RichText::new(crate::i18n::tr("sends it to the recipe collection.")).font(t.font(11.5)).color(t.text_label),
                ));
            });
            ui.add(egui::Label::new(
                // `→` and not the menu triangle: that has no glyph outside craft-fonts (i18n tests)
                egui::RichText::new(crate::i18n::tr("More photos: Help → Sample IR Photos. Your own photos: drag them in, or File → Import Photos…"))
                    .font(t.font(11.0))
                    .color(t.text_dim),
            ));
        });
    // the accent left border, over the card's fill
    let r = card.response.rect;
    ui.painter().rect_filled(Rect::from_min_max(pos2(r.left(), r.top()), pos2(r.left() + 3.0, r.bottom())), 0.0, t.accent);
    register(ui, "presetStartCard", r);
    ui.add_space(4.0);
}

/// A brand-new library's opening layout (the fork's IR playground, a web first visit): the Presets
/// column open and the Channel Mixer section expanded, so the visitor lands on the start-here
/// card, the IR recipes and the mixer. The rest of the layout is left alone.
pub fn first_visit_layout(ui: &mut crate::state::UiState) {
    ui.presets = true;
    if !ui.section_open("channelMixer") {
        ui.open_sections.push("channelMixer".into());
    }
}

/// The presets as the rows of a picker (a combo box's list), in list order with a header row
/// wherever the group changes. Only the rows in view are built (10 000+ presets made an open
/// picker take seconds a frame). Returns the id of the preset clicked.
pub fn picker_rows(ui: &mut egui::Ui, salt: &str, presets: &PresetList, selected: Option<&str>) -> Option<String> {
    let id = egui::Id::unique("preset-picker-rows");
    let rows = match ui.ctx().data(|d| d.get_temp::<(u64, Arc<Vec<PickRow>>)>(id)) {
        Some((generation, rows)) if generation == presets.generation() => rows,
        _ => {
            let mut rows = Vec::with_capacity(presets.len() + 16);
            let mut group: Option<&str> = None;
            for (i, p) in presets.iter().enumerate() {
                if group != Some(p.group.as_str()) {
                    group = Some(&p.group);
                    rows.push(PickRow::Header(crate::i18n::builtin_label(&p.group, p.builtin).to_string()));
                }
                rows.push(PickRow::Preset(i));
            }
            let rows = Arc::new(rows);
            ui.ctx().data_mut(|d| d.insert_temp(id, (presets.generation(), rows.clone())));
            rows
        }
    };
    let row_h = ui.spacing().interact_size.y;
    let mut out = None;
    egui::ScrollArea::vertical().id_salt(("preset-picker", salt)).max_height(300.0).auto_shrink([false, true]).show_rows(
        ui,
        row_h,
        rows.len(),
        |ui, range| {
            for row in rows.get(range).unwrap_or_default() {
                let size = vec2(ui.available_width(), row_h);
                match row {
                    PickRow::Header(g) => {
                        ui.add_sized(size, egui::Label::new(egui::RichText::new(g).size(10.5).weak()));
                    }
                    PickRow::Preset(i) => {
                        let Some(p) = presets.get(*i) else { continue };
                        let label = crate::i18n::builtin_label(&p.name, p.builtin);
                        if ui.add_sized(size, egui::Button::selectable(selected == Some(p.id.as_str()), label)).clicked() {
                            out = Some(p.id.clone());
                        }
                    }
                }
            }
        },
    );
    out
}

#[derive(Clone)]
enum PickRow {
    /// A group's name (in the UI language for stock groups).
    Header(String),
    /// An index into the session's presets.
    Preset(usize),
}

/// The rows `first..end` of a block of `n` rows `row_h` high starting at `top` that meet
/// `visible`.
fn rows_in_view(top: f32, row_h: f32, n: usize, visible: Rect) -> (usize, usize) {
    if row_h <= 0.0 || !top.is_finite() {
        return (0, n);
    }
    // `as usize` saturates (negative and NaN give 0)
    let first = (((visible.top() - top) / row_h).floor() as usize).min(n);
    let end = (((visible.bottom() - top) / row_h).ceil() as usize).clamp(first, n);
    (first, end)
}

/// The part of a `size` image that covers `r` (centre crop), as texture coordinates.
pub fn cover_uv(r: Rect, size: [usize; 2]) -> Rect {
    let a = size[0] as f32 / size[1].max(1) as f32;
    let ra = r.width() / r.height().max(1.0);
    if a > ra {
        let f = ra / a;
        Rect::from_min_max(pos2((1.0 - f) / 2.0, 0.0), pos2((1.0 + f) / 2.0, 1.0))
    } else {
        let f = a / ra;
        Rect::from_min_max(pos2(0.0, (1.0 - f) / 2.0), pos2(1.0, (1.0 + f) / 2.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn amount_slider_does_not_undo_a_later_edit() {
        let mut session = lightcraft_engine::Session::with_demo();
        let pid = "lc.golden-hour";
        session.execute("preset.apply", &json!({"id": pid})).unwrap();
        let state = AppliedPreset::new(&session, pid.into(), 100.0);
        assert!(state.is_current(&session));

        session.execute("develop.set", &json!({"values": {"light.exposure": 0.75}})).unwrap();
        assert!(!state.is_current(&session), "a newer edit owns the undo step");
    }

    #[test]
    fn amount_slider_does_not_undo_on_another_photo() {
        let mut session = lightcraft_engine::Session::with_demo();
        let pid = "lc.golden-hour";
        session.execute("preset.apply", &json!({"id": pid})).unwrap();
        let state = AppliedPreset::new(&session, pid.into(), 100.0);
        let active = session.active();
        let other = session.visible().iter().copied().find(|id| Some(*id) != active).unwrap();
        session.execute("library.select", &json!({"ids": [other.0]})).unwrap();
        assert!(!state.is_current(&session), "the preset belongs to the earlier selection");
    }
}
