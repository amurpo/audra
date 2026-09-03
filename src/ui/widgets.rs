//! Small reusable widget builders, shared across views to keep the look
//! consistent without duplicating GTK plumbing.
use glib::clone;
use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, Button, Label, Orientation, ToggleButton};
use libadwaita as adw;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::i18n::gettext;
use crate::ui::icons::{self, Icon};

const VIEW_TAB_ICON_SIZE: i32 = 16;

/// One tab in [`view_switcher_bar`].
pub struct ViewTab {
    pub stack_name: &'static str,
    pub icon: Icon,
    pub label: String,
}

/// Header tabs for an [`adw::ViewStack`], with bundled Remix icons (no
/// `GtkIconTheme` — required for macOS and consistent with the rest of Audra).
pub fn view_switcher_bar(stack: &adw::ViewStack, tabs: &[ViewTab]) -> GtkBox {
    let bar = GtkBox::new(Orientation::Horizontal, 0);
    bar.add_css_class("linked");
    bar.add_css_class("audra-view-switcher");
    bar.set_halign(Align::Center);

    let current = stack
        .visible_child_name()
        .map(|n| n.to_string())
        .unwrap_or_default();

    let mut group_leader: Option<ToggleButton> = None;
    let mut btn_list = Vec::new();

    for tab in tabs {
        let btn = ToggleButton::new();
        if let Some(leader) = &group_leader {
            btn.set_group(Some(leader));
        } else {
            group_leader = Some(btn.clone());
        }
        btn.set_active(tab.stack_name == current);
        btn.add_css_class("flat");

        let row = GtkBox::new(Orientation::Horizontal, 6);
        row.set_valign(Align::Center);
        row.append(&icons::image(tab.icon, VIEW_TAB_ICON_SIZE));
        row.append(&Label::new(Some(&tab.label)));
        btn.set_child(Some(&row));

        let stack = stack.clone();
        let name = tab.stack_name;
        btn.connect_toggled(clone!(
            #[weak]
            stack,
            move |b| {
                if b.is_active() {
                    stack.set_visible_child_name(name);
                }
            }
        ));

        bar.append(&btn);
        btn_list.push(btn);
    }
    let buttons = Rc::new(btn_list);

    let tabs_static: Vec<(&'static str, usize)> = tabs
        .iter()
        .enumerate()
        .map(|(i, t)| (t.stack_name, i))
        .collect();
    stack.connect_visible_child_name_notify(clone!(
        #[weak]
        stack,
        #[weak]
        buttons,
        move |_| {
            let Some(name) = stack.visible_child_name() else {
                return;
            };
            let name = name.as_str();
            for (tab_name, idx) in &tabs_static {
                if *tab_name == name {
                    if let Some(btn) = buttons.get(*idx) {
                        btn.set_active(true);
                    }
                    break;
                }
            }
        }
    ));

    bar
}

// --- Content width -------------------------------------------------------
//
// Every content surface sits in an `adw::Clamp` so nothing spans the full
// width of a wide monitor. A clamp only understands pixels, so the "share of
// the window" rule below is applied by recomputing `maximum-size` whenever the
// toplevel is resized — see [`on_window_width`].
//
// Two families, because they want opposite things from a wide screen:
//   * grids  — more covers per row is a straight win, so they take the ratio.
//   * lists  — a song row pins the title left and the duration right, so past
//     a point the two stop reading as one row. They grow, then stop.

/// Share of the window width the content column may take on wide screens —
/// 70 %, i.e. 15 % of breathing room per side.
const FLUID_RATIO: f64 = 0.70;

/// The fixed width Audra used before the ratio existed, kept as a floor so a
/// narrow window never ends up with *less* room than it had. Below roughly
/// 1257 px of window the floor wins and the layout is the old one, pixel for
/// pixel.
const CONTENT_FLOOR: i32 = 880;

/// Where track lists stop growing (see the family note above).
const LIST_CEILING: i32 = 1150;

/// The clamp eases into its maximum from here instead of snapping to it.
const TIGHTENING: i32 = 640;

/// Horizontal breathing margin around the whole content area (the `view_stack`
/// margins in `main_window`), subtracted from the window width to get the space
/// a clamp can actually be handed.
const CONTENT_MARGINS: i32 = 24;

/// One album column: the 176 px cover plus the 44 px gutter that the old fixed
/// 880 px / 4-column grid happened to produce. Grid widths are quantized to a
/// whole number of these, which is what holds that gutter at 44 px on every
/// screen instead of letting the leftover width stretch it.
pub const ALBUM_PITCH: i32 = 220;

/// Upper bound on album columns, so a very wide screen can't thin the grid out
/// into a wall of covers.
const MAX_ALBUM_COLUMNS: i32 = 16;

/// How many album columns fit at this window width. Single source of truth
/// behind both the grid-family clamp width and the grid's pinned column count,
/// so the two can never disagree.
pub fn album_columns(window_width: i32) -> i32 {
    let target = ((f64::from(window_width) * FLUID_RATIO) as i32).max(CONTENT_FLOOR);
    // Never ask for more than the window can give: on a narrow window the floor
    // is wider than the space available, and pinning the column count to it
    // would push the covers past the edge.
    let usable = target.min(window_width - CONTENT_MARGINS);
    (usable / ALBUM_PITCH).clamp(1, MAX_ALBUM_COLUMNS)
}

/// Width shared by every grid-family surface (Albums, Artists, and all three
/// clamps of the artist-detail page) at this window width.
pub fn grid_content_width(window_width: i32) -> i32 {
    album_columns(window_width) * ALBUM_PITCH
}

/// Width shared by every list-family surface (Songs, album detail).
pub fn list_content_width(window_width: i32) -> i32 {
    let target = (f64::from(window_width) * FLUID_RATIO) as i32;
    // Bounded by the grid width as well as by the ceiling: walking from the
    // album grid into an album's track list must never make the content column
    // *wider*, and the grid's quantization would otherwise allow exactly that
    // in the stretch where the grid sits waiting for its next whole column.
    target
        .min(LIST_CEILING)
        .min(grid_content_width(window_width))
        .max(CONTENT_FLOOR)
}

/// `adw::Clamp` with Audra's base content-width parameters. Not used directly:
/// callers take [`fluid_grid_clamp`] or [`fluid_list_clamp`], which bind the
/// maximum to the window width. The starting values are the pre-ratio ones, so
/// a clamp that is never realized still looks right.
fn content_clamp() -> adw::Clamp {
    let c = adw::Clamp::new();
    c.set_maximum_size(CONTENT_FLOOR);
    c.set_tightening_threshold(TIGHTENING);
    c
}

/// Retarget a clamp. The threshold is kept strictly below the maximum: a
/// threshold at or above it makes the clamp hand the child the whole width it
/// was given, which would undo the quantization on small windows.
fn set_clamp_width(clamp: &adw::Clamp, width: i32) {
    if clamp.maximum_size() == width {
        return;
    }
    clamp.set_maximum_size(width);
    clamp.set_tightening_threshold((width - 1).clamp(1, TIGHTENING));
}

/// Run `apply` with the toplevel window's width: once when `widget` is
/// realized, then again on every resize.
///
/// The width comes from the `GdkSurface`, not from `GtkWindow:default-width` —
/// the latter keeps reporting the *restored* size while the window is maximized
/// or fullscreen, which is precisely the case this exists for. The handler
/// lives on the surface, which outlives the widget, so it is disconnected on
/// unrealize: detail pages are built and dropped over and over.
pub fn on_window_width<W: IsA<gtk4::Widget>>(widget: &W, apply: impl Fn(i32) + 'static) {
    let apply = Rc::new(apply);
    let handler: Rc<RefCell<Option<(gtk4::gdk::Surface, glib::SignalHandlerId)>>> =
        Rc::new(RefCell::new(None));

    widget.connect_realize(clone!(
        #[strong]
        apply,
        #[strong]
        handler,
        move |w| {
            let Some(surface) = w.native().and_then(|n| n.surface()) else {
                return;
            };
            apply(surface.width());
            let id = surface.connect_width_notify(clone!(
                #[strong]
                apply,
                move |s| apply(s.width())
            ));
            *handler.borrow_mut() = Some((surface, id));
        }
    ));

    widget.connect_unrealize(clone!(
        #[strong]
        handler,
        move |_| {
            if let Some((surface, id)) = handler.borrow_mut().take() {
                surface.disconnect(id);
            }
        }
    ));
}

/// Clamp for a grid-family surface: follows the window at [`FLUID_RATIO`],
/// quantized to whole album columns.
pub fn fluid_grid_clamp() -> adw::Clamp {
    let c = content_clamp();
    on_window_width(
        &c,
        clone!(
            #[weak]
            c,
            move |w| set_clamp_width(&c, grid_content_width(w))
        ),
    );
    c
}

/// Clamp for a list-family surface: follows the window at [`FLUID_RATIO`] up to
/// [`LIST_CEILING`], then holds.
pub fn fluid_list_clamp() -> adw::Clamp {
    let c = content_clamp();
    on_window_width(
        &c,
        clone!(
            #[weak]
            c,
            move |w| set_clamp_width(&c, list_content_width(w))
        ),
    );
    c
}

/// Pin an album `GridView` to the column count its clamp was sized for.
///
/// Left to itself the grid derives the count from the width it is handed, and
/// the leftover stretches every column — which is why the gutter between covers
/// used to shrink from 44 px to ~16 px as the window grew. Pinning it to the
/// same [`album_columns`] the clamp used makes each column exactly one
/// [`ALBUM_PITCH`], so the gutter is the same on a laptop and on an ultrawide.
pub fn bind_album_columns(grid: &gtk4::GridView) {
    on_window_width(
        grid,
        clone!(
            #[weak]
            grid,
            move |w| {
                let columns = album_columns(w) as u32;
                // Raise the ceiling before the floor, never the other way
                // round: GTK complains if min ever exceeds max mid-update.
                if grid.max_columns() < columns {
                    grid.set_max_columns(columns);
                }
                grid.set_min_columns(columns);
                grid.set_max_columns(columns);
            }
        ),
    );
}

/// Big section header used at the top of every "content" page (Songs,
/// album detail, future playlists, etc.).
///
/// **Vertical margins live on the parent container, not on the label.** This
/// is important because the same label is used both standalone (Songs) and
/// inside [`page_title_row`] next to a back button. If the label carried its
/// own top margin, the back button would render at the row's `y=0` while the
/// label sat 12 px lower — visibly misaligned. Pushing the margin to the
/// parent keeps both children sharing the same baseline.
pub fn section_header_label(text: &str) -> Label {
    let lbl = Label::new(Some(text));
    lbl.add_css_class("title-2");
    lbl.set_xalign(0.0);
    lbl.set_valign(Align::Center);
    lbl.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    lbl.set_margin_start(4);
    lbl.set_margin_end(4);
    lbl
}

/// `[← back] [title]` row used by every top-of-page header in the app.
///
/// When `navigable` is `true` the back button is functional: it walks up to
/// the parent `adw::NavigationView` at click time via `ancestor()` and calls
/// `pop()`, which works the same whether the page is pushed onto the
/// Albums nav or the Artists nav (no need to pass the nav through the call
/// chain).
///
/// When `navigable` is `false` (Songs view, or any future root page) the
/// same button is built but rendered invisible and removed from focus/input.
/// **The slot is preserved on purpose** so the row has identical height and
/// horizontal alignment as detail pages — without this, switching between
/// Songs and an album detail makes the title visibly jump.
pub fn page_title_row(text: &str, navigable: bool) -> GtkBox {
    // Vertical margins on the row (not the children) so the back button and
    // the title share the same baseline.
    let row = GtkBox::new(Orientation::Horizontal, 6);
    row.set_margin_top(12);
    row.set_margin_bottom(2);
    row.set_margin_start(4);
    row.set_margin_end(4);

    let btn_back = icons::flat_icon_button(Icon::ArrowLeft, 20, None);
    btn_back.add_css_class("flat");
    btn_back.add_css_class("circular");
    btn_back.set_valign(Align::Center);

    if navigable {
        btn_back.set_tooltip_text(Some(&gettext("Back")));
        btn_back.connect_clicked(|btn| {
            if let Some(ancestor) = btn.ancestor(adw::NavigationView::static_type()) {
                if let Ok(nav) = ancestor.downcast::<adw::NavigationView>() {
                    nav.pop();
                }
            }
        });
    } else {
        // Invisible spacer: same footprint, no interaction, not in tab order.
        btn_back.set_opacity(0.0);
        btn_back.set_sensitive(false);
        btn_back.set_can_target(false);
        btn_back.set_focusable(false);
    }

    let title = section_header_label(text);
    title.set_hexpand(true);
    // The row already provides 4 px of side padding; drop the label's own
    // side padding so the title hugs the back arrow.
    title.set_margin_start(0);
    title.set_margin_end(0);

    row.append(&btn_back);
    row.append(&title);
    row
}

/// Settings-popover "segmented" row: a caption label over a linked group of
/// mutually exclusive toggle buttons (ReplayGain, Dynamic color, Language…).
/// `options` pairs each button label with the value it represents; the row
/// marks `initial` active *without* firing `on_change`, then calls
/// `on_change(value)` whenever the user picks a different segment.
pub fn segmented_setting_row<T: Copy + PartialEq + 'static>(
    label: &str,
    options: &[(String, T)],
    initial: T,
    on_change: impl Fn(T) + 'static,
) -> GtkBox {
    let row = GtkBox::new(Orientation::Vertical, 4);
    row.set_margin_top(4);
    row.set_margin_bottom(4);
    row.set_margin_start(8);
    row.set_margin_end(8);

    let lbl = Label::new(Some(label));
    lbl.set_xalign(0.0);
    lbl.add_css_class("caption");
    lbl.add_css_class("dim-label");

    let seg = GtkBox::new(Orientation::Horizontal, 0);
    seg.add_css_class("linked");

    let on_change = Rc::new(on_change);
    let mut group_leader: Option<ToggleButton> = None;
    for (text, value) in options {
        let btn = ToggleButton::with_label(text);
        match &group_leader {
            Some(leader) => btn.set_group(Some(leader)),
            None => group_leader = Some(btn.clone()),
        }
        // Activate the initial segment before connecting the handler, so
        // building the row never fires on_change (a spurious fire here can
        // trigger heavy work like a full window rebuild for Language).
        if *value == initial {
            btn.set_active(true);
        }
        let value = *value;
        let cb = Rc::clone(&on_change);
        btn.connect_toggled(move |b| {
            if b.is_active() {
                cb(value);
            }
        });
        seg.append(&btn);
    }

    row.append(&lbl);
    row.append(&seg);
    row
}

/// Like [`segmented_setting_row`], but the change is *deferred* so it can be
/// confirmed first. When the user picks a different segment, `on_request` runs
/// with the new value plus two callbacks: `commit` records the new value as the
/// active one, and `revert` snaps the selection back to the last committed value
/// without re-firing. Use it for changes heavy enough to warrant a prompt —
/// e.g. Language, which tears down and rebuilds the whole window. The caller
/// shows the dialog and calls `commit` on accept or `revert` on cancel.
pub fn segmented_setting_row_confirm<T: Copy + PartialEq + 'static>(
    label: &str,
    options: &[(String, T)],
    initial: T,
    on_request: impl Fn(T, Rc<dyn Fn()>, Rc<dyn Fn()>) + 'static,
) -> GtkBox {
    let row = GtkBox::new(Orientation::Vertical, 4);
    row.set_margin_top(4);
    row.set_margin_bottom(4);
    row.set_margin_start(8);
    row.set_margin_end(8);

    let lbl = Label::new(Some(label));
    lbl.set_xalign(0.0);
    lbl.add_css_class("caption");
    lbl.add_css_class("dim-label");

    let seg = GtkBox::new(Orientation::Horizontal, 0);
    seg.add_css_class("linked");

    let on_request = Rc::new(on_request);
    // The currently applied value, the one `revert` returns to.
    let committed = Rc::new(Cell::new(initial));
    // Set while we programmatically move the selection back, so the resulting
    // `toggled` fire on the committed segment is swallowed instead of treated
    // as a fresh user request (which would re-open the dialog forever).
    let suppress = Rc::new(Cell::new(false));
    // value -> button, so `revert` can re-activate the committed segment.
    let buttons: Rc<RefCell<Vec<(T, ToggleButton)>>> = Rc::new(RefCell::new(Vec::new()));

    let mut group_leader: Option<ToggleButton> = None;
    for (text, value) in options {
        let btn = ToggleButton::with_label(text);
        match &group_leader {
            Some(leader) => btn.set_group(Some(leader)),
            None => group_leader = Some(btn.clone()),
        }
        if *value == initial {
            btn.set_active(true);
        }
        buttons.borrow_mut().push((*value, btn.clone()));
        let value = *value;
        let on_request = Rc::clone(&on_request);
        let committed = Rc::clone(&committed);
        let suppress = Rc::clone(&suppress);
        let buttons = Rc::clone(&buttons);
        btn.connect_toggled(move |b| {
            if !b.is_active() {
                return;
            }
            if suppress.get() {
                suppress.set(false);
                return;
            }
            if value == committed.get() {
                return;
            }
            let commit: Rc<dyn Fn()> = {
                let committed = Rc::clone(&committed);
                Rc::new(move || committed.set(value))
            };
            let revert: Rc<dyn Fn()> = {
                let committed = Rc::clone(&committed);
                let suppress = Rc::clone(&suppress);
                let buttons = Rc::clone(&buttons);
                Rc::new(move || {
                    let target = committed.get();
                    if let Some((_, b)) = buttons.borrow().iter().find(|(v, _)| *v == target) {
                        suppress.set(true);
                        b.set_active(true);
                    }
                })
            };
            on_request(value, commit, revert);
        });
        seg.append(&btn);
    }

    row.append(&lbl);
    row.append(&seg);
    row
}

/// "Play all" action button: themed accent (follows the system color), with
/// a play glyph next to the label. No `pill` so the corners are the default
/// Adwaita radius — compact, recognisable, single definition used by Songs,
/// Album detail and Artist detail headers.
pub fn play_all_button(label: &str) -> Button {
    let btn = Button::new();
    btn.add_css_class("suggested-action");
    // Marker class so the dynamic-tint Full mode can override its
    // background with @card_shade_color — otherwise the button takes the
    // tinted accent color and disappears into the tinted window bg.
    btn.add_css_class("audra-play-all");

    let row = GtkBox::new(Orientation::Horizontal, 8);
    row.set_valign(Align::Center);

    let icon = icons::image(Icon::Play, 16);
    let lbl = Label::new(Some(label));

    row.append(&icon);
    row.append(&lbl);
    btn.set_child(Some(&row));
    btn
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The narrowest a cell may get before the 176 px cover no longer fits:
    /// cover + the 2 px CSS margin each side (see `audra-album-grid > child`).
    const CELL_MINIMUM: i32 = 180;

    /// Space a clamp can actually be handed at a given window width.
    fn available(window: i32) -> i32 {
        window - CONTENT_MARGINS
    }

    #[test]
    fn narrow_windows_keep_the_pre_ratio_layout() {
        // Below the crossover the floor wins, so nothing changes for the
        // windows Audra used to be designed around.
        for window in [1024, 1152, 1257] {
            assert_eq!(grid_content_width(window), 880, "window {window}");
            assert_eq!(album_columns(window), 4, "window {window}");
        }
    }

    #[test]
    fn wide_windows_grow_with_the_window() {
        assert_eq!(album_columns(1920), 6);
        assert_eq!(grid_content_width(1920), 1320);
        assert_eq!(album_columns(2560), 8);
        assert_eq!(grid_content_width(2560), 1760);
        assert_eq!(album_columns(3440), 10);
        assert_eq!(grid_content_width(3440), 2200);
    }

    #[test]
    fn very_wide_screens_stop_at_the_column_cap() {
        assert_eq!(album_columns(5120), MAX_ALBUM_COLUMNS);
        assert_eq!(album_columns(7680), MAX_ALBUM_COLUMNS);
    }

    /// The point of quantizing the width: every column is exactly one pitch, so
    /// the gutter between covers is the same on a laptop and on an ultrawide.
    #[test]
    fn every_column_is_exactly_one_pitch() {
        for window in (400..=7680).step_by(7) {
            let width = grid_content_width(window);
            let columns = album_columns(window);
            assert_eq!(width, columns * ALBUM_PITCH, "window {window}");
        }
    }

    /// A pinned column count must never ask for more room than the window can
    /// give, or the covers spill past the edge and a scrollbar appears.
    #[test]
    fn columns_always_fit_the_window() {
        for window in (400..=7680).step_by(7) {
            let needed = album_columns(window) * CELL_MINIMUM;
            assert!(
                needed <= available(window).max(CELL_MINIMUM),
                "window {window}: {needed} px of columns in {} px",
                available(window)
            );
        }
    }

    #[test]
    fn content_width_never_shrinks_as_the_window_grows() {
        let mut previous_grid = 0;
        let mut previous_list = 0;
        for window in (200..=7680).step_by(3) {
            let grid = grid_content_width(window);
            let list = list_content_width(window);
            assert!(grid >= previous_grid, "grid shrank at {window}");
            assert!(list >= previous_list, "list shrank at {window}");
            previous_grid = grid;
            previous_list = list;
        }
    }

    #[test]
    fn lists_grow_then_stop() {
        assert_eq!(list_content_width(800), CONTENT_FLOOR);
        assert_eq!(list_content_width(1280), CONTENT_FLOOR);
        // Between the floor and the ceiling a list follows the grid it shares
        // the window with, one whole album column at a time.
        assert_eq!(list_content_width(1643), 1100);
        // Past the ceiling a song row would put its title and its duration a
        // screen apart, so it holds.
        assert_eq!(list_content_width(1920), LIST_CEILING);
        assert_eq!(list_content_width(2560), LIST_CEILING);
        assert_eq!(list_content_width(3440), LIST_CEILING);
    }

    #[test]
    fn grids_are_never_narrower_than_lists() {
        // Both families share the floor, and the grid quantization must not
        // round a grid below the list it sits next to in the same window.
        for window in (1200..=7680).step_by(11) {
            assert!(
                grid_content_width(window) >= list_content_width(window),
                "window {window}"
            );
        }
    }
}
