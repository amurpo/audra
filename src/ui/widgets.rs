//! Small reusable widget builders, shared across views to keep the look
//! consistent without duplicating GTK plumbing.
use glib::clone;
use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, Button, Label, Orientation, ToggleButton};
use libadwaita as adw;
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

/// `adw::Clamp` with Audra's standard content-width parameters. Single point
/// of truth for "how wide is the useful content column"; changing the
/// constants here propagates to every list, grid, action row and section
/// title across the app — no other code should set these values directly.
pub fn content_clamp() -> adw::Clamp {
    let c = adw::Clamp::new();
    c.set_maximum_size(880);
    c.set_tightening_threshold(640);
    c
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
