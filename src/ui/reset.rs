use adw::prelude::*;
use glib::clone;
use libadwaita as adw;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use crate::i18n::gettext;
use crate::library::{self, db::Database};
use crate::ui::albums_view::AlbumsView;
use crate::ui::artists_view::ArtistsView;
use crate::ui::library_view::LibraryView;
use crate::ui::main_window::{reload_all_views, start_scan};

/// Confirm and, on acceptance, wipe the scanned library and cover caches,
/// then rescan the configured folder. Music files, the selected folder and
/// the Last.fm session are never touched.
pub fn show_reset_dialog(
    window: &adw::ApplicationWindow,
    db: Arc<Mutex<Database>>,
    lib_view: Rc<RefCell<LibraryView>>,
    albums_view: Rc<AlbumsView>,
    artists_view: Rc<ArtistsView>,
    loading_box: gtk4::Box,
    spinner: gtk4::Spinner,
) {
    let dialog = adw::AlertDialog::new(
        Some(&gettext("Reset library?")),
        Some(&gettext(
            "This permanently deletes all scanned tracks and cached cover art. \
             Your music files, the selected folder and your Last.fm session are \
             not affected. The library is rescanned afterwards.",
        )),
    );
    dialog.add_response("cancel", &gettext("Cancel"));
    dialog.add_response("reset", &gettext("Reset"));
    dialog.set_response_appearance("reset", adw::ResponseAppearance::Destructive);
    dialog.set_default_response(Some("cancel"));
    dialog.set_close_response("cancel");
    dialog.connect_response(
        None,
        clone!(
            #[strong]
            db,
            #[strong]
            lib_view,
            #[strong]
            albums_view,
            #[strong]
            artists_view,
            #[strong]
            loading_box,
            #[strong]
            spinner,
            move |_, resp| {
                if resp != "reset" {
                    return;
                }
                {
                    let _ = db.lock().unwrap().clear_library();
                }
                library::metadata::clear_cover_cache();
                let folder = db.lock().unwrap().get_setting("music_folder");
                if let Some(folder) = folder {
                    start_scan(
                        folder,
                        Arc::clone(&db),
                        Rc::clone(&lib_view),
                        Rc::clone(&albums_view),
                        Rc::clone(&artists_view),
                        loading_box.clone(),
                        spinner.clone(),
                    );
                } else {
                    reload_all_views(&db, &lib_view, &albums_view, &artists_view);
                }
            }
        ),
    );
    dialog.present(Some(window));
}
