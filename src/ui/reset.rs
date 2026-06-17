use adw::prelude::*;
use glib::clone;
use libadwaita as adw;

use crate::i18n::gettext;
use crate::library;
use crate::ui::main_window::{reload_all_views, start_scan, Views};

/// Confirm and, on acceptance, reset the library. The user picks the scope:
///
/// * **Library only** — wipe scanned tracks and rescan, keeping cached art so
///   it re-shows the moment the rescan repopulates each album.
/// * **Cover art only** — wipe the cached cover BLOBs and the on-disk art cache
///   and re-fetch them, leaving the scanned tracks untouched.
/// * **Everything** — both of the above.
///
/// Music files, the selected folder and the Last.fm session are never touched.
/// The media-controls (MPRIS) thumbnail cache is intentionally left alone: it
/// is repopulated on its own as tracks play, so wiping it here is pointless.
pub fn show_reset_dialog(
    window: &adw::ApplicationWindow,
    views: Views,
    loading_box: gtk4::Box,
    spinner: gtk4::Spinner,
    toast_overlay: adw::ToastOverlay,
) {
    let dialog = adw::AlertDialog::new(
        Some(&gettext("Reset library?")),
        Some(&gettext(
            "Choose what to clear and rebuild. Your music files, the selected \
             folder and your Last.fm session are never affected.\n\n\
             • Library only: re-reads tracks from disk, keeps downloaded art.\n\
             • Cover art only: re-fetches all artwork, keeps the track list.\n\
             • Everything: both.",
        )),
    );
    dialog.add_response("cancel", &gettext("Cancel"));
    dialog.add_response("library", &gettext("Library only"));
    dialog.add_response("covers", &gettext("Cover art only"));
    dialog.add_response("all", &gettext("Everything"));
    dialog.set_response_appearance("all", adw::ResponseAppearance::Destructive);
    dialog.set_default_response(Some("cancel"));
    dialog.set_close_response("cancel");
    dialog.connect_response(
        None,
        clone!(
            #[strong]
            views,
            #[strong]
            loading_box,
            #[strong]
            spinner,
            #[strong]
            toast_overlay,
            move |_, resp| {
                // Helper: repopulate tracks by rescanning the configured folder,
                // or just reload the views when no folder is set yet.
                let rescan = |views: &Views| {
                    let folder = views.db.lock().unwrap().music_folder();
                    if let Some(folder) = folder {
                        start_scan(folder, views.clone(), loading_box.clone(), spinner.clone());
                    } else {
                        reload_all_views(views);
                    }
                };
                // Confirm the action is done. For Library/Everything a rescan
                // spinner takes over; the toast still tells the user the reset
                // itself went through.
                let toast = |msg: &str| toast_overlay.add_toast(adw::Toast::new(msg));
                match resp {
                    "library" => {
                        let _ = views.db.lock().unwrap().clear_tracks();
                        rescan(&views);
                        toast(&gettext("Library reset"));
                    }
                    "covers" => {
                        let _ = views.db.lock().unwrap().clear_album_covers();
                        library::metadata::clear_cover_cache();
                        // No rescan: the track list is unchanged. Reloading the
                        // views re-runs the cover fetch against the now-empty
                        // cache, re-deriving embedded art and re-downloading.
                        reload_all_views(&views);
                        toast(&gettext("Cover art reset"));
                    }
                    "all" => {
                        let _ = views.db.lock().unwrap().clear_library();
                        library::metadata::clear_cover_cache();
                        rescan(&views);
                        toast(&gettext("Library and cover art reset"));
                    }
                    _ => {}
                }
            }
        ),
    );
    dialog.present(Some(window));
}
