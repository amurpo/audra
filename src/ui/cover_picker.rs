//! Poweramp-style art picker: right-click an album card or an artist card to
//! open a context menu, then choose the image from candidate thumbnails
//! fetched from every source, or reset it back to the automatic pick.
//!
//! The dialog/menu scaffolding is shared; albums and artists only differ in
//! how the chosen bytes are applied to the widget, persisted, and what the
//! "reset to automatic" pipeline is — all injected as closures.

use adw::prelude::*;
use glib::clone;
use gtk4::gio;
use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, Button, ContentFit, FlowBox, Label, Orientation, Picture, Stack};
use libadwaita as adw;
use std::cell::Cell;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crate::i18n::gettext;
use crate::library::db::Database;
use crate::library::metadata::{self, CoverCandidate};
use crate::ui::albums_view::CARD_SIZE;
use crate::ui::artists_view::AVATAR_SIZE;
use crate::ui::image_utils::{pixels_to_texture, scale_to_pixels, ScaledPixels};

const THUMB: i32 = 168;

/// Apply chosen bytes to the target widget on the UI thread; `None` clears it.
type ApplyFn = Rc<dyn Fn(Option<&[u8]>)>;
/// Store the chosen bytes durably. Runs on the UI thread; quick.
type PersistFn = Arc<dyn Fn(&[u8]) + Send + Sync>;
/// Gather candidates off the UI thread for a given search term (the user can
/// refine it in the dialog, Plex-style, when the real name finds nothing).
type CandidatesFn = Arc<dyn Fn(&str) -> Vec<CoverCandidate> + Send + Sync>;

/// One thumbnail ready for the grid: scaled pixels for display plus the
/// original bytes to store verbatim if the user picks it.
type ScaledCandidate = (String, Vec<u8>, i32, bool, Vec<u8>);

fn apply_album_cover(stack: &Stack, picture: &Picture, data: Option<&[u8]>) {
    match data {
        Some(d) => {
            // Scale to CARD_SIZE — the same path image_loader uses. Handing
            // raw bytes to Texture::from_bytes preserved source resolution
            // but produced a texture whose natural-size grew the FlowBox
            // homogeneous cells, so every album card in the grid expanded
            // to match the picked one.
            if let Some((px, rs, alpha)) = scale_to_pixels(d, CARD_SIZE) {
                let tex = pixels_to_texture(px, rs, alpha, CARD_SIZE);
                picture.set_paintable(Some(&tex));
                stack.set_visible_child_name("art");
            }
        }
        None => stack.set_visible_child_name("placeholder"),
    }
}

fn apply_artist_photo(avatar: &adw::Avatar, data: Option<&[u8]>) {
    // Replicate start_photo_fetch exactly: scale on a worker thread, then
    // deliver the MemoryTexture from the GLib main loop — the same async
    // path that produces sharp results on app startup / restart.
    match data {
        Some(d) => {
            let bytes = d.to_vec();
            let avatar = avatar.clone();
            let result: Arc<Mutex<Option<ScaledPixels>>> = Arc::new(Mutex::new(None));
            let done = Arc::new(AtomicBool::new(false));
            let result_tx = Arc::clone(&result);
            let done_tx = Arc::clone(&done);
            std::thread::spawn(move || {
                if let Some(scaled) = scale_to_pixels(&bytes, AVATAR_SIZE) {
                    *result_tx.lock().unwrap() = Some(scaled);
                }
                done_tx.store(true, Ordering::Relaxed);
            });
            glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
                if !done.load(Ordering::Relaxed) {
                    return glib::ControlFlow::Continue;
                }
                if let Some((px, rs, alpha)) = result.lock().unwrap().take() {
                    let tex = pixels_to_texture(px, rs, alpha, AVATAR_SIZE);
                    avatar.set_custom_image(Some(&tex));
                }
                glib::ControlFlow::Break
            });
        }
        None => avatar.set_custom_image(None::<&gtk4::gdk::Texture>),
    }
}

/// Add a right-click gesture to an album card opening the cover menu.
pub fn install_album_cover_gesture(
    child: &gtk4::FlowBoxChild,
    db: Arc<Mutex<Database>>,
    artist: String,
    album: String,
    track_path: String,
    stack: Stack,
    picture: Picture,
) {
    let apply: ApplyFn = Rc::new(move |d| apply_album_cover(&stack, &picture, d));

    let persist: PersistFn = {
        let (db, a, al) = (Arc::clone(&db), artist.clone(), album.clone());
        Arc::new(move |b: &[u8]| {
            let _ = db.lock().unwrap().set_cover(&a, &al, b);
        })
    };

    // The search term overrides the album title only; the embedded-art
    // candidate comes from the file and is independent of the query.
    let candidates: CandidatesFn = {
        let (a, tp) = (artist.clone(), track_path.clone());
        Arc::new(move |query: &str| {
            let mut v = Vec::new();
            if let Some(d) = crate::library::art::read_cover_art(&tp) {
                v.push(CoverCandidate {
                    source: gettext("Embedded in file"),
                    data: d,
                });
            }
            v.extend(metadata::fetch_album_cover_candidates(&a, query));
            v
        })
    };

    let default_query = album;

    install_gesture(
        child,
        gettext("Choose cover"),
        default_query,
        apply,
        persist,
        candidates,
    );
}

/// Add a right-click gesture to an artist card opening the photo menu.
pub fn install_artist_photo_gesture(
    child: &gtk4::FlowBoxChild,
    artist: String,
    avatar: adw::Avatar,
) {
    let apply: ApplyFn = Rc::new(move |d| apply_artist_photo(&avatar, d));

    let persist: PersistFn = {
        let a = artist.clone();
        Arc::new(move |b: &[u8]| metadata::set_artist_photo(&a, b))
    };

    // The search term is the artist name to query; the user can refine it.
    let candidates: CandidatesFn =
        Arc::new(move |query: &str| metadata::fetch_artist_photo_candidates(query));

    let default_query = artist;

    install_gesture(
        child,
        gettext("Choose photo"),
        default_query,
        apply,
        persist,
        candidates,
    );
}

fn install_gesture(
    child: &gtk4::FlowBoxChild,
    title: String,
    default_query: String,
    apply: ApplyFn,
    persist: PersistFn,
    candidates: CandidatesFn,
) {
    let gesture = gtk4::GestureClick::new();
    gesture.set_button(gtk4::gdk::BUTTON_SECONDARY);
    let child_w = child.clone();
    gesture.connect_pressed(move |g, _, x, y| {
        g.set_state(gtk4::EventSequenceState::Claimed);
        show_menu(
            &child_w,
            x,
            y,
            title.clone(),
            default_query.clone(),
            Rc::clone(&apply),
            Arc::clone(&persist),
            Arc::clone(&candidates),
        );
    });
    child.add_controller(gesture);
}

#[allow(clippy::too_many_arguments)]
fn show_menu(
    parent: &gtk4::FlowBoxChild,
    x: f64,
    y: f64,
    title: String,
    default_query: String,
    apply: ApplyFn,
    persist: PersistFn,
    candidates: CandidatesFn,
) {
    let pop = gtk4::Popover::new();
    pop.set_parent(parent);
    pop.set_pointing_to(Some(&gtk4::gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
    pop.connect_closed(|p| p.unparent());

    let vbox = GtkBox::new(Orientation::Vertical, 0);
    let btn_change = Button::with_label(&gettext("Change image…"));
    btn_change.add_css_class("flat");
    btn_change.set_halign(Align::Fill);
    let btn_custom = Button::with_label(&gettext("Use custom image…"));
    btn_custom.add_css_class("flat");
    btn_custom.set_halign(Align::Fill);
    let btn_remove = Button::with_label(&gettext("Remove image"));
    btn_remove.add_css_class("flat");
    btn_remove.set_halign(Align::Fill);
    vbox.append(&btn_change);
    vbox.append(&btn_custom);
    vbox.append(&btn_remove);
    pop.set_child(Some(&vbox));

    btn_change.connect_clicked(clone!(
        #[strong]
        apply,
        #[strong]
        persist,
        #[strong]
        candidates,
        #[weak]
        pop,
        #[strong]
        parent,
        #[strong]
        title,
        #[strong]
        default_query,
        move |_| {
            pop.popdown();
            open_picker(
                &parent,
                title.clone(),
                default_query.clone(),
                Rc::clone(&apply),
                Arc::clone(&persist),
                Arc::clone(&candidates),
            );
        }
    ));

    btn_custom.connect_clicked(clone!(
        #[strong]
        apply,
        #[strong]
        persist,
        #[weak]
        pop,
        #[strong]
        parent,
        move |_| {
            pop.popdown();
            pick_custom_image(&parent, Rc::clone(&apply), Arc::clone(&persist));
        }
    ));

    btn_remove.connect_clicked(clone!(
        #[strong]
        apply,
        #[strong]
        persist,
        #[weak]
        pop,
        move |_| {
            pop.popdown();
            // An empty blob is the "user removed this on purpose" marker:
            // the placeholder shows now and the automatic fetch skips it,
            // so the image does not silently come back on the next scan.
            persist(&[]);
            apply(None);
        }
    ));

    pop.popup();
}

fn show_load_error(window: Option<&gtk4::Window>) {
    let d = adw::AlertDialog::new(Some(&gettext("Could not load that image.")), None);
    d.add_response("ok", &gettext("OK"));
    d.present(window);
}

/// Let the user pick any image from disk and use it as the art. Reuses the
/// same `persist`/`apply` strategies, so it is just another byte source.
fn pick_custom_image(parent: &gtk4::FlowBoxChild, apply: ApplyFn, persist: PersistFn) {
    let window = parent.root().and_downcast::<gtk4::Window>();

    let filter = gtk4::FileFilter::new();
    filter.add_pixbuf_formats();
    filter.set_name(Some(&gettext("Images")));
    let filters = gio::ListStore::new::<gtk4::FileFilter>();
    filters.append(&filter);

    let dialog = gtk4::FileDialog::new();
    dialog.set_title(&gettext("Use custom image…"));
    dialog.set_filters(Some(&filters));

    let win_err = window.clone();
    dialog.open(window.as_ref(), gio::Cancellable::NONE, move |res| {
        let Ok(file) = res else {
            return; // user cancelled
        };
        let Some(path) = file.path() else {
            show_load_error(win_err.as_ref());
            return;
        };
        let Ok(bytes) = std::fs::read(&path) else {
            show_load_error(win_err.as_ref());
            return;
        };
        // Validate it actually decodes as an image before storing it.
        if scale_to_pixels(&bytes, THUMB).is_none() {
            show_load_error(win_err.as_ref());
            return;
        }
        persist(&bytes);
        apply(Some(&bytes));
    });
}

fn open_picker(
    parent: &gtk4::FlowBoxChild,
    title: String,
    default_query: String,
    apply: ApplyFn,
    persist: PersistFn,
    candidates: CandidatesFn,
) {
    let _ = parent;
    let dialog = adw::Dialog::new();
    dialog.set_title(&title);
    dialog.set_content_width(560);
    dialog.set_content_height(540);

    let header = adw::HeaderBar::new();
    let search = gtk4::SearchEntry::new();
    search.set_placeholder_text(Some(&gettext("Search…")));
    search.set_hexpand(true);
    search.set_width_request(260);
    search.set_text(&default_query);
    header.set_title_widget(Some(&search));

    let spinner = gtk4::Spinner::new();
    spinner.set_margin_top(24);
    spinner.set_margin_bottom(24);

    let grid = FlowBox::new();
    grid.set_selection_mode(gtk4::SelectionMode::None);
    grid.set_homogeneous(true);
    grid.set_column_spacing(12);
    grid.set_row_spacing(12);
    grid.set_margin_top(12);
    grid.set_margin_bottom(12);
    grid.set_margin_start(12);
    grid.set_margin_end(12);
    grid.set_min_children_per_line(2);
    grid.set_max_children_per_line(3);

    let scroll = gtk4::ScrolledWindow::new();
    scroll.set_vexpand(true);
    scroll.set_child(Some(&grid));

    let body = GtkBox::new(Orientation::Vertical, 0);
    body.append(&spinner);
    body.append(&scroll);

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&body));
    dialog.set_child(Some(&toolbar));

    // A monotonically increasing generation: every search bumps it. The poll
    // loop of an older search sees the mismatch and discards its results, so a
    // slow previous fetch can't pollute the grid when the user refines the
    // term (Plex-style). Search runs only on Enter to spare the rate-limited
    // APIs (MusicBrainz is 1 req/s).
    let generation = Rc::new(Cell::new(0u64));

    let run_search: Rc<dyn Fn(String)> = Rc::new(clone!(
        #[strong]
        apply,
        #[strong]
        persist,
        #[strong]
        candidates,
        #[strong]
        generation,
        #[weak]
        grid,
        #[weak]
        spinner,
        #[weak]
        dialog,
        move |query: String| {
            let gen = generation.get().wrapping_add(1);
            generation.set(gen);

            while let Some(c) = grid.first_child() {
                grid.remove(&c);
            }
            spinner.set_visible(true);
            spinner.start();

            // Collect candidates off the UI thread, scaling each to a
            // thumbnail so the poll loop only has to build textures.
            let queue: Arc<Mutex<Vec<ScaledCandidate>>> = Arc::new(Mutex::new(Vec::new()));
            let finished = Arc::new(AtomicBool::new(false));
            {
                let queue = Arc::clone(&queue);
                let finished = Arc::clone(&finished);
                let candidates = Arc::clone(&candidates);
                std::thread::spawn(move || {
                    for c in candidates(&query) {
                        if let Some((px, rs, alpha)) = scale_to_pixels(&c.data, THUMB) {
                            queue
                                .lock()
                                .unwrap()
                                .push((c.source, px, rs, alpha, c.data));
                        }
                    }
                    finished.store(true, Ordering::Relaxed);
                });
            }

            let any_added = Arc::new(AtomicBool::new(false));
            glib::timeout_add_local(
                std::time::Duration::from_millis(250),
                clone!(
                    #[strong]
                    apply,
                    #[strong]
                    persist,
                    #[strong]
                    generation,
                    #[weak]
                    grid,
                    #[weak]
                    spinner,
                    #[weak]
                    dialog,
                    #[strong]
                    any_added,
                    #[upgrade_or]
                    glib::ControlFlow::Break,
                    move || {
                        // A newer search superseded this one: stop and discard.
                        if generation.get() != gen {
                            return glib::ControlFlow::Break;
                        }
                        for (source, px, rs, alpha, raw) in queue.lock().unwrap().drain(..) {
                            any_added.store(true, Ordering::Relaxed);
                            let tex = pixels_to_texture(px, rs, alpha, THUMB);
                            let pic = Picture::new();
                            pic.set_paintable(Some(&tex));
                            pic.set_content_fit(ContentFit::Cover);
                            pic.set_size_request(THUMB, THUMB);
                            pic.set_overflow(gtk4::Overflow::Hidden);

                            let caption = Label::new(Some(&source));
                            caption.add_css_class("caption");
                            caption.add_css_class("dim-label");
                            caption.set_ellipsize(gtk4::pango::EllipsizeMode::End);

                            let cell = GtkBox::new(Orientation::Vertical, 4);
                            cell.append(&pic);
                            cell.append(&caption);

                            let btn = Button::new();
                            btn.add_css_class("flat");
                            // Keep the hover/active background tight to the
                            // thumbnail; otherwise the homogeneous FlowBox
                            // stretches each cell (very visible with only 2
                            // candidates) and the highlight runs wide.
                            btn.set_halign(Align::Center);
                            btn.set_hexpand(false);
                            btn.set_child(Some(&cell));
                            btn.connect_clicked(clone!(
                                #[strong]
                                apply,
                                #[strong]
                                persist,
                                #[weak]
                                dialog,
                                move |_| {
                                    persist(&raw);
                                    apply(Some(&raw));
                                    dialog.close();
                                }
                            ));
                            grid.insert(&btn, -1);
                        }

                        if finished.load(Ordering::Relaxed) {
                            spinner.stop();
                            spinner.set_visible(false);
                            if !any_added.load(Ordering::Relaxed) {
                                let empty = Label::new(Some(&gettext("No images found.")));
                                empty.add_css_class("dim-label");
                                empty.set_margin_top(24);
                                grid.insert(&empty, -1);
                            }
                            glib::ControlFlow::Break
                        } else {
                            glib::ControlFlow::Continue
                        }
                    }
                ),
            );
        }
    ));

    search.connect_activate(clone!(
        #[strong]
        run_search,
        move |e| run_search(e.text().to_string())
    ));

    run_search(default_query);
    dialog.present(Some(parent));
}
