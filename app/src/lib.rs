//! Edge Camera Discovery — Tauri backend: window, commands and events.

use std::sync::{Arc, Mutex};

use ecm_discovery_core::actions::Actions;
use ecm_discovery_core::scanner::{self, Event, Scanner};
use ecm_discovery_core::trust::{self, TrustStore};
use ecm_discovery_core::types::{
    self, Camera, CameraStatus, FdpInfo, IdentifyResult, KeyTrust, PrepareSetIpResult, SetIpRequest, SetIpResult,
};
use fdp::{NetMode, NetState, StaticAddress};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_opener::OpenerExt;

const DEMO_ONLY: &str = "Not available in demo mode.";

struct AppState {
    demo: bool,
    scanner: Scanner,
    actions: Actions,
}

fn demo_camera(id: &str, hostname: &str, ip: &str, online: bool) -> Camera {
    Camera {
        id: id.into(),
        hostname: hostname.into(),
        ip: ip.into(),
        port: 443,
        scheme: "https".into(),
        last_seen: types::now_ms() - if online { 0 } else { 60_000 },
        online,
        status: if online { CameraStatus::Online } else { CameraStatus::Offline },
        fdp: None,
    }
}

fn demo_cameras() -> Vec<Camera> {
    let mut new_camera = demo_camera("demo-3", "ecm-244260001238", "192.168.1.100", false);
    new_camera.status = CameraStatus::OtherSubnet;
    new_camera.fdp = Some(FdpInfo {
        serial: "244260001238".into(),
        mac: "00:1b:c5:0a:0b:0c".into(),
        vendor: "3dvisionlabs".into(),
        model: "C7".into(),
        fw_version: "3.1.0".into(),
        net: NetState {
            interface: "eth0".into(),
            mode: NetMode::Dhcp,
            address: Some("192.168.1.100/24".into()),
            gateway: Some("192.168.1.1".into()),
            dns: vec![],
            fallback: Some(StaticAddress { address: "192.168.1.100/24".into(), gateway: Some("192.168.1.1".into()) }),
        },
        caps: vec!["set_ip".into(), "identify".into()],
        set_ip: "enabled".into(),
        key_fp: "SHA256:vXOB6rCNLWJnXCfHnpmCFX5xYXoJWIZR2Ku6xQPTmSM".into(),
        key_trust: KeyTrust::New,
        versions: vec![2],
        version: Some(2),
        local_address: "169.254.33.7".into(),
        local_cidr: "169.254.33.7/16".into(),
    });
    vec![
        demo_camera("demo-1", "ecm-00A1B2C3", "192.168.1.101", true),
        demo_camera("demo-2", "ecm-00D4E5F6", "192.168.1.102", true),
        new_camera,
        demo_camera("demo-4", "ecm-00DEF012", "192.168.1.104", false),
    ]
}

#[tauri::command]
fn get_cameras(state: State<AppState>) -> Vec<Camera> {
    if state.demo { demo_cameras() } else { state.scanner.get_all() }
}

#[tauri::command]
fn refresh(state: State<AppState>) {
    if !state.demo {
        state.scanner.refresh();
    }
}

#[tauri::command]
fn open_camera(app: AppHandle, state: State<AppState>, id: String) {
    let Some(camera) = state.scanner.get_by_id(&id) else { return };
    let default_port = if camera.scheme == "https" { 443 } else { 80 };
    let port = if camera.port == default_port { String::new() } else { format!(":{}", camera.port) };
    let url = format!("{}://{}{port}", camera.scheme, camera.ip);
    if let Err(err) = app.opener().open_url(&url, None::<&str>) {
        eprintln!("Cannot open {url}: {err}");
    }
}

// Async commands that borrow state must return a Result; they never fail,
// errors are reported inside the result for the UI.

#[tauri::command]
async fn prepare_set_ip(state: State<'_, AppState>, id: String) -> Result<PrepareSetIpResult, ()> {
    if state.demo {
        return Ok(PrepareSetIpResult::error(DEMO_ONLY));
    }
    Ok(state.actions.prepare_set_ip(&id).await)
}

#[tauri::command]
fn trust_camera(state: State<AppState>, id: String) -> bool {
    !state.demo && state.actions.trust_camera(&id)
}

#[tauri::command]
async fn set_ip(state: State<'_, AppState>, req: SetIpRequest) -> Result<SetIpResult, ()> {
    if state.demo {
        return Ok(SetIpResult::new("error", DEMO_ONLY));
    }
    Ok(state.actions.set_ip(req).await)
}

#[tauri::command]
async fn identify(state: State<'_, AppState>, id: String) -> Result<IdentifyResult, ()> {
    if state.demo {
        return Ok(IdentifyResult::failed(DEMO_ONLY));
    }
    Ok(state.actions.identify(&id).await)
}

/// macOS requires a menu bar: app menu (About, Hide, Quit) plus Edit, without
/// which copy/paste shortcuts do not work in text fields.
#[cfg(target_os = "macos")]
fn macos_menu(handle: &AppHandle) -> tauri::Result<tauri::menu::Menu<tauri::Wry>> {
    use tauri::menu::{Menu, PredefinedMenuItem as P, Submenu};
    let app_menu = Submenu::with_items(
        handle,
        "Edge Camera Discovery",
        true,
        &[
            &P::about(handle, None, None)?,
            &P::separator(handle)?,
            &P::hide(handle, None)?,
            &P::hide_others(handle, None)?,
            &P::show_all(handle, None)?,
            &P::separator(handle)?,
            &P::quit(handle, None)?,
        ],
    )?;
    let edit_menu = Submenu::with_items(
        handle,
        "Edit",
        true,
        &[
            &P::undo(handle, None)?,
            &P::redo(handle, None)?,
            &P::separator(handle)?,
            &P::cut(handle, None)?,
            &P::copy(handle, None)?,
            &P::paste(handle, None)?,
            &P::select_all(handle, None)?,
        ],
    )?;
    Menu::with_items(handle, &[&app_menu, &edit_menu])
}

/// GTK3 (and so WebKitGTK in Tauri 2) only knows integer scales on Wayland: at
/// a desktop scale of e.g. 1.1 it renders at 2 and the compositor shrinks the
/// image, which makes thin text grainy. Via XWayland, WebKit renders at the
/// fractional scale itself (KDE's default "apply scaling themselves").
/// `ECM_NATIVE_WAYLAND=1` keeps native Wayland.
#[cfg(target_os = "linux")]
fn prefer_xwayland() {
    let xwayland = std::env::var_os("WAYLAND_DISPLAY").is_some() && std::env::var_os("DISPLAY").is_some();
    if xwayland && std::env::var("ECM_NATIVE_WAYLAND").is_err() {
        // SAFETY: called first thing in `run`, before any other thread exists
        unsafe { std::env::set_var("GDK_BACKEND", "x11") };
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[cfg(target_os = "linux")]
    prefer_xwayland();

    let demo = std::env::args().any(|a| a == "--demo") || std::env::var("ECM_DEMO").is_ok_and(|v| v == "1");

    let builder = tauri::Builder::default().plugin(tauri_plugin_opener::init());
    #[cfg(target_os = "macos")]
    let builder = builder.menu(macos_menu);

    builder
        .setup(move |app| {
            let trust_file = trust::default_file().ok_or("no config directory")?;
            let trust = Arc::new(Mutex::new(TrustStore::load(trust_file)));

            let handle = app.handle().clone();
            let emit: scanner::Emit = Arc::new(move |event| {
                let result = match event {
                    Event::Found(camera) => handle.emit("cameras:found", camera),
                    Event::Updated(camera) => handle.emit("cameras:updated", camera),
                    Event::Removed(id) => handle.emit("cameras:removed", id),
                };
                if let Err(err) = result {
                    eprintln!("Cannot send event to the window: {err}");
                }
            });

            let scanner = Scanner::new(trust.clone(), emit, tauri::async_runtime::handle().inner().clone());
            if demo {
                println!("[DEMO] Running with fake cameras — no network scanning");
            } else {
                scanner.start();
            }
            let actions = Actions::new(scanner.clone(), trust);
            app.manage(AppState { demo, scanner, actions });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_cameras,
            refresh,
            open_camera,
            prepare_set_ip,
            trust_camera,
            set_ip,
            identify
        ])
        .run(tauri::generate_context!())
        .expect("error while running Edge Camera Discovery");
}
