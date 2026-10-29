// pi-oven desktop: a window onto a pi-oven server. The page itself is served by the server (so the app never goes stale
// and there is no cross-origin traffic); this shell only remembers which server, and offers a small launcher page to
// choose one. The server's pages are given no access to the app (see capabilities/default.json: the launcher only).

use std::{fs, path::PathBuf};
use tauri::{AppHandle, Manager, Url, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
#[cfg(desktop)]
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem, Submenu},
    webview::NewWindowResponse,
};

fn config_file(app: &AppHandle) -> Option<PathBuf> {
    Some(app.path().app_config_dir().ok()?.join("server.txt"))
}

fn saved_server(app: &AppHandle) -> Option<String> {
    let s = fs::read_to_string(config_file(app)?).ok()?;
    let s = s.lines().next()?.trim().to_string();
    (!s.is_empty()).then_some(s)
}

/// `--server URL` or `PI_OVEN_SERVER` beat the saved one.
fn given_server() -> Option<String> {
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        if a == "--server" {
            return args.next();
        }
        if let Some(v) = a.strip_prefix("--server=") {
            return Some(v.to_string());
        }
    }
    std::env::var("PI_OVEN_SERVER").ok().filter(|s| !s.trim().is_empty())
}

/// What the person typed → a URL we are willing to open: http(s) only; a bare `host:port` means http.
fn parse_server(raw: &str) -> Result<Url, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err("서버 주소를 입력하세요.".into());
    }
    let full = if raw.contains("://") { raw.to_string() } else { format!("http://{raw}") };
    let url = Url::parse(&full).map_err(|_| "주소 형식이 올바르지 않습니다. 예: localhost:4141".to_string())?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err("http:// 또는 https:// 주소만 쓸 수 있습니다.".into());
    }
    Ok(url)
}

/// A phone starts by connecting to the saved server by itself (the launcher is what you get back with the back button).
#[tauri::command]
fn auto_connect() -> bool {
    cfg!(mobile)
}

#[tauri::command]
fn get_server(app: AppHandle) -> Option<String> {
    given_server().or_else(|| saved_server(&app))
}

/// The window onto the server. Its page is the server's, so it is given no access to the app.
fn show_server(app: &AppHandle, url: Url) -> tauri::Result<()> {
    // A phone has one window: the launcher itself goes to the server.
    #[cfg(mobile)]
    if let Some(w) = app.get_webview_window("launcher") {
        return w.navigate(url);
    }
    if let Some(w) = app.get_webview_window("main") {
        w.navigate(url)?;
        return w.set_focus();
    }
    let b = WebviewWindowBuilder::new(app, "main", WebviewUrl::External(url)).title("pi-oven");
    #[cfg(desktop)]
    let b = b
        .inner_size(1360.0, 860.0)
        .min_inner_size(420.0, 520.0)
        // The server's page opens links with window.open (Pin tab addresses, for one): those go to the system browser.
        .on_new_window(|url, _| {
            if matches!(url.scheme(), "http" | "https") {
                let _ = open::that(url.as_str());
            }
            NewWindowResponse::Deny
        });
    b.build().map(|_| ())
}

/// The small window for choosing a server.
fn show_launcher(app: &AppHandle) -> tauri::Result<()> {
    if let Some(w) = app.get_webview_window("launcher") {
        return w.set_focus();
    }
    let b = WebviewWindowBuilder::new(app, "launcher", WebviewUrl::App("index.html".into())).title("pi-oven 서버");
    #[cfg(desktop)]
    let b = b.inner_size(520.0, 460.0).resizable(false);
    b.build().map(|_| ())
}

/// Remember the server (the file may hold a token, so only its owner can read it) and open it.
#[tauri::command]
fn connect(app: AppHandle, window: WebviewWindow, url: String) -> Result<(), String> {
    let mut parsed = parse_server(&url)?;
    // Connecting again to the same server without typing the token keeps the one saved with it.
    if parsed.fragment().is_none() {
        if let Some(old) = saved_server(&app).and_then(|v| parse_server(&v).ok()) {
            if old.origin() == parsed.origin() {
                parsed.set_fragment(old.fragment());
            }
        }
    }
    if let Some(path) = config_file(&app) {
        if let Some(dir) = path.parent() {
            let _ = fs::create_dir_all(dir);
        }
        if fs::write(&path, parsed.as_str()).is_ok() {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
            }
        }
    }
    show_server(&app, parsed).map_err(|e| e.to_string())?;
    #[cfg(desktop)]
    if window.label() == "launcher" {
        let _ = window.close();
    }
    #[cfg(mobile)]
    let _ = window;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![get_server, connect, auto_connect])
        .setup(|app| {
            let h = app.handle();
            #[cfg(desktop)]
            {
            let change = MenuItem::with_id(h, "server", "서버 바꾸기…", true, Some("CmdOrCtrl+L"))?;
            let reload = MenuItem::with_id(h, "reload", "새로고침", true, Some("CmdOrCtrl+R"))?;
            let menu = Menu::with_items(
                h,
                &[
                    &Submenu::with_items(h, "pi-oven", true, &[&change, &reload, &PredefinedMenuItem::separator(h)?, &PredefinedMenuItem::quit(h, Some("종료"))?])?,
                    &Submenu::with_items(
                        h,
                        "편집",
                        true,
                        &[
                            &PredefinedMenuItem::undo(h, Some("실행 취소"))?,
                            &PredefinedMenuItem::redo(h, Some("다시 실행"))?,
                            &PredefinedMenuItem::separator(h)?,
                            &PredefinedMenuItem::cut(h, Some("잘라내기"))?,
                            &PredefinedMenuItem::copy(h, Some("복사"))?,
                            &PredefinedMenuItem::paste(h, Some("붙여넣기"))?,
                            &PredefinedMenuItem::select_all(h, Some("모두 선택"))?,
                        ],
                    )?,
                ],
            )?;
            app.set_menu(menu)?;
            }

            #[cfg(desktop)]
            match given_server().or_else(|| saved_server(h)).and_then(|v| parse_server(&v).ok()) {
                Some(u) => show_server(h, u)?,
                None => show_launcher(h)?,
            }
            // A phone has no menu to come back to the launcher with, so it always starts there (the saved address is filled in).
            #[cfg(mobile)]
            show_launcher(h)?;
            Ok(())
        });

    #[cfg(desktop)]
    let builder = builder
        .on_menu_event(|app, ev| {
            match ev.id().as_ref() {
                "server" => {
                    let _ = show_launcher(app);
                }
                "reload" => {
                    if let Some(win) = app.get_webview_window("main") {
                        let _ = win.eval("location.reload()");
                    }
                }
                _ => {}
            }
        });

    builder
        .run(tauri::generate_context!())
        .expect("pi-oven desktop failed to start");
}
