use adw::prelude::*;
use glib::clone;
use gtk4::prelude::*;
use gtk4::Button;
use libadwaita as adw;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use crate::i18n::gettext;
use crate::library::db::Database;
use crate::scrobbler::LastFmClient;

fn open_url(url: &str) -> std::io::Result<()> {
    // Only ever hand the OS handler an http/https URL. Anything else (file://,
    // javascript:, a flag-looking string) could be coerced by `start`/`open`
    // into doing something other than opening a browser tab.
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "refusing to open non-http URL",
        ));
    }
    #[cfg(target_os = "windows")]
    {
        // `start` treats the first quoted argument as the window title, so we
        // pass an empty title explicitly to keep the URL in the URL slot.
        std::process::Command::new("cmd")
            .args(["/c", "start", "", url])
            .spawn()?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open").arg(url).spawn()?;
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        std::process::Command::new("xdg-open").arg(url).spawn()?;
    }
    Ok(())
}

pub fn show_lastfm_dialog(
    parent: &adw::ApplicationWindow,
    db: Arc<Mutex<Database>>,
    lastfm: Arc<Mutex<Option<LastFmClient>>>,
) {
    let win = adw::Window::builder()
        .title(gettext("Last.fm Account"))
        .transient_for(parent)
        .modal(true)
        .default_width(460)
        .default_height(440)
        .resizable(false)
        .build();

    let header = adw::HeaderBar::new();
    let stack = gtk4::Stack::new();
    stack.set_transition_type(gtk4::StackTransitionType::SlideLeftRight);
    stack.set_transition_duration(300);

    // Página 1: autorizar
    let auth_box = gtk4::Box::new(gtk4::Orientation::Vertical, 12);
    auth_box.set_valign(gtk4::Align::Center);
    auth_box.set_vexpand(true);
    auth_box.set_margin_top(24);
    auth_box.set_margin_bottom(24);
    auth_box.set_margin_start(24);
    auth_box.set_margin_end(24);

    let auth_status = adw::StatusPage::new();
    auth_status.set_icon_name(Some("avatar-default-symbolic"));
    auth_status.set_title(&gettext("Connect to Last.fm"));
    auth_status.set_description(Some(&gettext(
        "Authorize Audra on your Last.fm account to track your listens.",
    )));
    auth_status.set_vexpand(true);

    let auth_error_label = gtk4::Label::new(None);
    auth_error_label.set_wrap(true);
    auth_error_label.add_css_class("lastfm-err");
    auth_error_label.set_halign(gtk4::Align::Center);

    let btn_authorize = Button::with_label(&gettext("Authorize on Last.fm"));
    btn_authorize.add_css_class("suggested-action");
    btn_authorize.set_halign(gtk4::Align::Center);

    auth_box.append(&auth_status);
    auth_box.append(&auth_error_label);
    auth_box.append(&btn_authorize);

    // Página 2: esperando confirmación
    let wait_box = gtk4::Box::new(gtk4::Orientation::Vertical, 12);
    wait_box.set_valign(gtk4::Align::Center);
    wait_box.set_vexpand(true);
    wait_box.set_margin_top(24);
    wait_box.set_margin_bottom(24);
    wait_box.set_margin_start(24);
    wait_box.set_margin_end(24);

    let wait_status = adw::StatusPage::new();
    wait_status.set_icon_name(Some("network-transmit-receive-symbolic"));
    wait_status.set_title(&gettext("Waiting for authorization"));
    wait_status.set_description(Some(&gettext(
        "Complete authorization in the browser and then click «I already authorized».",
    )));
    wait_status.set_vexpand(true);

    let wait_error_label = gtk4::Label::new(None);
    wait_error_label.set_wrap(true);
    wait_error_label.add_css_class("lastfm-err");
    wait_error_label.set_halign(gtk4::Align::Center);

    let wait_btn_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    wait_btn_row.set_halign(gtk4::Align::Center);

    let btn_confirmed = Button::with_label(&gettext("I already authorized"));
    btn_confirmed.add_css_class("suggested-action");
    let btn_cancel_wait = Button::with_label(&gettext("Cancel"));

    wait_btn_row.append(&btn_confirmed);
    wait_btn_row.append(&btn_cancel_wait);
    wait_box.append(&wait_status);
    wait_box.append(&wait_error_label);
    wait_box.append(&wait_btn_row);

    // Página 3: conectado
    let ok_box = gtk4::Box::new(gtk4::Orientation::Vertical, 24);
    ok_box.set_valign(gtk4::Align::Center);
    ok_box.set_vexpand(true);
    ok_box.set_margin_top(32);
    ok_box.set_margin_bottom(32);
    ok_box.set_margin_start(24);
    ok_box.set_margin_end(24);

    let ok_status = adw::StatusPage::new();
    ok_status.set_icon_name(Some("emblem-ok-symbolic"));
    ok_status.set_title(&gettext("Connected to Last.fm"));
    ok_status.set_vexpand(true);

    let ok_btn_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    ok_btn_row.set_halign(gtk4::Align::Center);

    let btn_change = Button::with_label(&gettext("Change account"));
    let btn_forget = Button::with_label(&gettext("Disconnect"));
    btn_forget.add_css_class("destructive-action");

    ok_btn_row.append(&btn_change);
    ok_btn_row.append(&btn_forget);
    ok_box.append(&ok_status);
    ok_box.append(&ok_btn_row);

    stack.add_named(&auth_box, Some("authorize"));
    stack.add_named(&wait_box, Some("waiting"));
    stack.add_named(&ok_box, Some("connected"));

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&stack));
    win.set_content(Some(&toolbar));

    {
        let db_g = db.lock().unwrap();
        let username_val = db_g.get_setting("lastfm_username").unwrap_or_default();
        let connected = db_g
            .get_setting("lastfm_session_key")
            .map(|k| !k.is_empty())
            .unwrap_or(false);
        if connected && !username_val.is_empty() {
            ok_status.set_description(Some(&username_val));
            stack.set_visible_child_name("connected");
        } else {
            stack.set_visible_child_name("authorize");
        }
    }

    let pending_token: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));

    btn_authorize.connect_clicked(clone!(
        #[weak]
        auth_error_label,
        #[weak]
        stack,
        #[weak]
        btn_authorize,
        #[strong]
        pending_token,
        move |_| {
            if !LastFmClient::is_configured() {
                auth_error_label.set_text(&gettext("The proxy URL is not configured."));
                return;
            }
            btn_authorize.set_sensitive(false);
            auth_error_label.set_text("");

            let (tx, rx) = std::sync::mpsc::channel::<Result<(String, String), String>>();
            std::thread::spawn(move || match LastFmClient::get_auth_token() {
                Ok(r) => {
                    let _ = tx.send(Ok((r.token, r.auth_url)));
                }
                Err(e) => {
                    let _ = tx.send(Err(e.to_string()));
                }
            });

            let pending_c = Rc::clone(&pending_token);
            glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
                use std::sync::mpsc::TryRecvError;
                match rx.try_recv() {
                    Ok(Ok((token, auth_url))) => {
                        *pending_c.borrow_mut() = Some(token);
                        let _ = open_url(&auth_url);
                        stack.set_visible_child_name("waiting");
                        btn_authorize.set_sensitive(true);
                        glib::ControlFlow::Break
                    }
                    Ok(Err(e)) => {
                        auth_error_label.set_text(&format!("{}: {}", gettext("Error"), e));
                        btn_authorize.set_sensitive(true);
                        glib::ControlFlow::Break
                    }
                    Err(TryRecvError::Empty) => glib::ControlFlow::Continue,
                    Err(TryRecvError::Disconnected) => {
                        btn_authorize.set_sensitive(true);
                        glib::ControlFlow::Break
                    }
                }
            });
        }
    ));

    btn_confirmed.connect_clicked(clone!(
        #[weak]
        wait_error_label,
        #[weak]
        stack,
        #[weak]
        ok_status,
        #[weak]
        btn_confirmed,
        #[strong]
        pending_token,
        #[strong]
        db,
        #[strong]
        lastfm,
        move |_| {
            let token = match pending_token.borrow().clone() {
                Some(t) => t,
                None => {
                    wait_error_label.set_text(&gettext(
                        "No pending token. Please authorize again.",
                    ));
                    return;
                }
            };
            btn_confirmed.set_sensitive(false);
            wait_error_label.set_text("");

            let (tx, rx) = std::sync::mpsc::channel::<Result<(String, String), String>>();
            let db2 = Arc::clone(&db);
            let lastfm2 = Arc::clone(&lastfm);
            std::thread::spawn(move || match LastFmClient::get_session(&token) {
                Ok(r) => {
                    {
                        let db_g = db2.lock().unwrap();
                        let _ = db_g.set_setting("lastfm_session_key", &r.session_key);
                        let _ = db_g.set_setting("lastfm_username", &r.username);
                    }
                    let new_client = LastFmClient::new().with_session(&r.session_key);
                    *lastfm2.lock().unwrap() = Some(new_client);
                    let _ = tx.send(Ok((r.session_key, r.username)));
                }
                Err(e) => {
                    let _ = tx.send(Err(e.to_string()));
                }
            });

            glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
                use std::sync::mpsc::TryRecvError;
                match rx.try_recv() {
                    Ok(Ok((_sk, username))) => {
                        ok_status.set_description(Some(&username));
                        stack.set_visible_child_name("connected");
                        btn_confirmed.set_sensitive(true);
                        glib::ControlFlow::Break
                    }
                    Ok(Err(e)) => {
                        wait_error_label.set_text(&format!("{}: {}", gettext("Error"), e));
                        btn_confirmed.set_sensitive(true);
                        glib::ControlFlow::Break
                    }
                    Err(TryRecvError::Empty) => glib::ControlFlow::Continue,
                    Err(TryRecvError::Disconnected) => {
                        btn_confirmed.set_sensitive(true);
                        glib::ControlFlow::Break
                    }
                }
            });
        }
    ));

    btn_cancel_wait.connect_clicked(clone!(
        #[weak]
        stack,
        #[strong]
        pending_token,
        move |_| {
            *pending_token.borrow_mut() = None;
            stack.set_visible_child_name("authorize");
        }
    ));

    btn_change.connect_clicked(clone!(
        #[weak]
        stack,
        move |_| {
            stack.set_visible_child_name("authorize");
        }
    ));

    btn_forget.connect_clicked(clone!(
        #[weak]
        stack,
        #[strong]
        db,
        #[strong]
        lastfm,
        move |_| {
            {
                let db_g = db.lock().unwrap();
                let _ = db_g.delete_setting("lastfm_session_key");
                let _ = db_g.delete_setting("lastfm_username");
            }
            *lastfm.lock().unwrap() = None;
            stack.set_visible_child_name("authorize");
        }
    ));

    win.present();
}
