// pi-oven desktop: a window onto a pi-oven server. The page itself is served by the server (so the app never goes stale
// and there is no cross-origin traffic); this shell only remembers which server, and offers a small launcher page to
// choose one. The server's pages are given no access to the app (see capabilities/default.json: the launcher only).

use std::{fs, path::PathBuf};
use tauri::{AppHandle, Manager, Url, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
#[cfg(desktop)]
use tauri::webview::NewWindowResponse;

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
/// Errors are message keys; the launcher page (`ui/index.html`) words them in its language.
fn parse_server(raw: &str) -> Result<Url, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err("err.empty".into());
    }
    let full = if raw.contains("://") { raw.to_string() } else { format!("http://{raw}") };
    let url = Url::parse(&full).map_err(|_| "err.format".to_string())?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err("err.scheme".into());
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

/// Windows open at 1280x720 (16:9) and can be resized. There is no menu bar: it made the web view miss its size on some
/// Linux desktops (blank until a resize, clipped on resize), and what it offered lives elsewhere now.
#[cfg(desktop)]
const DEFAULT_SIZE: (f64, f64) = (1280.0, 720.0);

/// Gives the server's page a way back to the launcher without any access to the app: it only sets a function on `window`.
/// A phone steps back in history (the launcher is the page before); a computer navigates to a made-up address that the
/// window intercepts (see `show_server`) and answers by opening the launcher.
#[cfg(desktop)]
const BACK_SCRIPT: &str = "window.__piOvenBack = () => { location.href = 'pi-oven-launcher://back'; };";
#[cfg(mobile)]
const BACK_SCRIPT: &str = "window.__piOvenBack = () => history.back();";

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
    let b = WebviewWindowBuilder::new(app, "main", WebviewUrl::External(url))
        .title("pi-oven")
        .initialization_script(BACK_SCRIPT);
    #[cfg(desktop)]
    let handle = app.clone();
    #[cfg(desktop)]
    let b = b
        .on_navigation(move |u| {
            if u.scheme() != "pi-oven-launcher" {
                return true;
            }
            // Open the launcher first so that closing this window never leaves the app without one.
            if show_launcher(&handle).is_ok() {
                if let Some(w) = handle.get_webview_window("main") {
                    let _ = w.close();
                }
            }
            false
        })
        .inner_size(DEFAULT_SIZE.0, DEFAULT_SIZE.1)
        .min_inner_size(640.0, 360.0)
        .center()
        // The server's page opens links with window.open (Pin tab addresses, for one): those go to the system browser.
        .on_new_window(|url, _| {
            if matches!(url.scheme(), "http" | "https") {
                let _ = open::that(url.as_str());
            }
            NewWindowResponse::Deny
        });
    let w = b.build()?;
    // Ask for the keyboard explicitly: a window opened by the app itself does not always get it from the desktop.
    #[cfg(desktop)]
    let _ = w.set_focus();
    let _ = &w;
    Ok(())
}

/// The window for choosing a server.
fn show_launcher(app: &AppHandle) -> tauri::Result<()> {
    if let Some(w) = app.get_webview_window("launcher") {
        return w.set_focus();
    }
    let b = WebviewWindowBuilder::new(app, "launcher", WebviewUrl::App("index.html".into())).title("pi-oven");
    #[cfg(desktop)]
    let b = b.inner_size(DEFAULT_SIZE.0, DEFAULT_SIZE.1).min_inner_size(480.0, 360.0).center();
    let w = b.build()?;
    #[cfg(desktop)]
    let _ = w.set_focus();
    let _ = &w;
    Ok(())
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

/// The AppImage brings its own GTK and WebKit, and with them its own list of input-method modules (no fcitx in it) and,
/// in older builds, its own libwayland. What it is started with is settled here, before GTK reads the environment.
/// A value the person set themselves always wins. Nothing changes outside the AppImage (the native build uses the
/// system's own GTK, modules and drivers).
#[cfg(target_os = "linux")]
mod linux_env {
    use std::{env, path::Path};

    pub struct Plan {
        pub backend: Option<&'static str>,
        pub im: Option<Option<&'static str>>,
        pub xmodifiers: Option<&'static str>,
        pub no_dmabuf: bool,
    }

    pub struct Seen<'a> {
        pub wayland_session: bool,
        /// The AppImage still carries libwayland: native Wayland would load it next to the system's Mesa and can crash.
        pub bundled_wayland: bool,
        pub backend: Option<&'a str>,
        pub im: Option<&'a str>,
        pub xmodifiers: Option<&'a str>,
        /// The IM module the person asked for is one the AppImage has.
        pub im_bundled: bool,
        pub nvidia: bool,
        pub dmabuf_set: bool,
    }

    pub fn plan(s: &Seen) -> Plan {
        // Native Wayland where it is safe: sharp on scaled screens, the GPU path WebKit expects, and the compositor's own
        // text input. X11 (XWayland) otherwise, with Wayland left as a fallback only when the session is Wayland.
        let backend = match s.backend {
            Some(_) => None,
            None if s.wayland_session && !s.bundled_wayland => Some("wayland,x11"),
            None => Some("x11"),
        };
        let on_wayland = match s.backend {
            Some(b) => b.starts_with("wayland"),
            None => s.wayland_session && !s.bundled_wayland,
        };
        // fcitx (fcitx5 included) has no module in the AppImage. Asking for it leaves GTK on its plain fallback: no
        // Korean, or text that is typed but never drawn. On Wayland the compositor carries the input method
        // (text-input), which fcitx5 and ibus both speak; on X11 fcitx5 answers XIM.
        let fcitx = s.im.is_some_and(|m| m.starts_with("fcitx")) || s.xmodifiers.is_some_and(|x| x.contains("fcitx"));
        let (im, xmodifiers) = if fcitx {
            if on_wayland {
                (Some(Some("wayland")), None)
            } else {
                (Some(Some("xim")), s.xmodifiers.is_none().then_some("@im=fcitx"))
            }
        } else if s.im.is_some_and(|m| !m.is_empty()) && !s.im_bundled {
            // another module the AppImage does not have: let GTK choose (Wayland's text input, or its default)
            (Some(on_wayland.then_some("wayland")), None)
        } else {
            (None, None)
        };
        // NVIDIA's driver is where WebKit's DMA-BUF renderer leaves a black window (after minimising, or from the start).
        // Elsewhere it stays on: it is the fast path.
        let no_dmabuf = s.nvidia && !s.dmabuf_set;
        Plan { backend, im, xmodifiers, no_dmabuf }
    }

    pub fn apply() {
        let Some(appdir) = env::var_os("APPDIR").filter(|_| env::var_os("APPIMAGE").is_some()) else { return };
        let appdir = Path::new(&appdir);
        let get = |k: &str| env::var(k).ok().filter(|v| !v.is_empty());
        let (backend, im, xmod) = (get("GDK_BACKEND"), get("GTK_IM_MODULE"), get("XMODIFIERS"));
        let im_bundled = im.as_deref().is_some_and(|m| {
            m.split(':').all(|m| appdir.join(format!("usr/lib/gtk-3.0/3.0.0/immodules/im-{m}.so")).exists())
        });
        let seen = Seen {
            // only a named Wayland display counts (a session type alone, as under sudo or ssh, has nothing to connect to)
            wayland_session: get("WAYLAND_DISPLAY").is_some(),
            bundled_wayland: appdir.join("usr/lib/libwayland-client.so.0").exists(),
            backend: backend.as_deref(),
            im: im.as_deref(),
            xmodifiers: xmod.as_deref(),
            im_bundled,
            nvidia: Path::new("/proc/driver/nvidia/version").exists(),
            dmabuf_set: env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_some(),
        };
        let p = plan(&seen);
        if let Some(b) = p.backend {
            env::set_var("GDK_BACKEND", b);
        }
        match p.im {
            Some(Some(m)) => env::set_var("GTK_IM_MODULE", m),
            Some(None) => env::remove_var("GTK_IM_MODULE"),
            None => {}
        }
        if let Some(x) = p.xmodifiers {
            env::set_var("XMODIFIERS", x);
        }
        if p.no_dmabuf {
            env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        }
        if env::var_os("PI_OVEN_DEBUG_ENV").is_some() {
            eprintln!(
                "pi-oven env: GDK_BACKEND={:?} GTK_IM_MODULE={:?} XMODIFIERS={:?} WEBKIT_DISABLE_DMABUF_RENDERER={:?}",
                env::var("GDK_BACKEND").ok(),
                env::var("GTK_IM_MODULE").ok(),
                env::var("XMODIFIERS").ok(),
                env::var("WEBKIT_DISABLE_DMABUF_RENDERER").ok()
            );
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn seen() -> Seen<'static> {
            Seen { wayland_session: true, bundled_wayland: false, backend: None, im: None, xmodifiers: None, im_bundled: false, nvidia: false, dmabuf_set: false }
        }

        #[test]
        fn arch_wayland_with_fcitx5_goes_native_and_types_through_the_compositor() {
            let p = plan(&Seen { im: Some("fcitx"), xmodifiers: Some("@im=fcitx"), ..seen() });
            assert_eq!(p.backend, Some("wayland,x11"));
            assert_eq!(p.im, Some(Some("wayland")));
            assert!(!p.no_dmabuf, "the GPU path stays on away from NVIDIA");
        }

        #[test]
        fn an_old_appimage_with_its_own_libwayland_stays_on_x11_and_uses_xim_for_fcitx() {
            let p = plan(&Seen { bundled_wayland: true, im: Some("fcitx5"), ..seen() });
            assert_eq!(p.backend, Some("x11"));
            assert_eq!(p.im, Some(Some("xim")));
            assert_eq!(p.xmodifiers, Some("@im=fcitx"));
        }

        #[test]
        fn what_the_person_set_wins() {
            let p = plan(&Seen { backend: Some("x11"), im: Some("ibus"), im_bundled: true, dmabuf_set: true, nvidia: true, ..seen() });
            assert_eq!(p.backend, None);
            assert!(p.im.is_none() && p.xmodifiers.is_none());
            assert!(!p.no_dmabuf);
        }

        #[test]
        fn nvidia_turns_off_only_the_dmabuf_renderer() {
            assert!(plan(&Seen { nvidia: true, ..seen() }).no_dmabuf);
        }

        #[test]
        fn a_module_the_appimage_lacks_is_dropped() {
            assert_eq!(plan(&Seen { im: Some("uim"), ..seen() }).im, Some(Some("wayland")));
            assert_eq!(plan(&Seen { wayland_session: false, im: Some("uim"), ..seen() }).im, Some(None));
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[cfg(target_os = "linux")]
    linux_env::apply();
    let builder = tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![get_server, connect, auto_connect])
        .setup(|app| {
            let h = app.handle();

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

    builder
        .run(tauri::generate_context!())
        .expect("pi-oven desktop failed to start");
}
