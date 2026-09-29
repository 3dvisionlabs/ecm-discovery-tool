//! ecm-discovery-cli — Edge Camera Discovery on the command line: list
//! cameras, open the web interface, identify and Change IP. Uses the same
//! discovery code (lib/) and the same trusted camera keys as
//! the app.

use std::io::{BufRead, IsTerminal, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use clap::{ArgGroup, Args, Parser, Subcommand};
use ecm_discovery_core::actions::Actions;
use ecm_discovery_core::messages::version_message;
use ecm_discovery_core::scanner::{Emit, Event, Scanner};
use ecm_discovery_core::trust::{self, TrustStore};
use ecm_discovery_core::types::{Camera, CameraStatus, FdpInfo, KeyTrust, PrepareSetIpResult, SetIpRequest};
use fdp::{NetConfig, NetMode, StaticAddress};
use serde_json::json;
use tokio::sync::mpsc;
use tokio::time::{Instant, sleep, sleep_until};
use zeroize::Zeroize;

/// Upper limit for one scan (FDP 3 s, mDNS 5 s, TCP checks 3 s each)
const SCAN_TIMEOUT: Duration = Duration::from_secs(15);
/// After Change IP: when to look for the camera, and for how long (like the app)
const REDISCOVERY_DELAY: Duration = Duration::from_secs(3);
const AWAIT_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Parser)]
#[command(
    name = "ecm-discovery-cli",
    version,
    about = "Find 3dvisionlabs cameras on the local network, open their web interface, identify them and change \
             their IP address.",
    after_help = "CAMERA can be a serial number, hostname, IP address or MAC address.\n\
                  Camera keys are trusted in the same file as the Edge Camera Discovery app."
)]
struct Cli {
    /// Print machine-readable JSON on stdout
    #[arg(long, global = true)]
    json: bool,

    /// Trusted camera keys (default: the file of the Edge Camera Discovery app)
    #[arg(long, global = true, value_name = "FILE", hide = true)]
    trust_file: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Scan once and list the cameras found
    List,
    /// Keep scanning and print every change, like the app window
    Watch,
    /// Open the web interface of a camera in the browser
    Open {
        camera: String,
        /// Only print the URL
        #[arg(long)]
        print: bool,
    },
    /// Let the camera LED flash so you can find it
    Identify { camera: String },
    /// Change the network settings of a camera ("Change IP")
    SetIp(SetIpArgs),
}

#[derive(Args)]
#[command(group(ArgGroup::new("mode").required(true).args(["static_address", "dhcp", "factory_default", "suggested"])))]
struct SetIpArgs {
    camera: String,

    /// Static address with prefix length, e.g. 192.168.1.50/24
    #[arg(long = "static", value_name = "ADDRESS/PREFIX")]
    static_address: Option<String>,
    /// Gateway for --static
    #[arg(long, requires = "static_address")]
    gateway: Option<String>,

    /// Get the address via DHCP
    #[arg(long)]
    dhcp: bool,
    /// Static address for --dhcp when no DHCP server answers
    #[arg(long, requires = "dhcp", value_name = "ADDRESS/PREFIX")]
    fallback: Option<String>,
    /// Gateway for --fallback
    #[arg(long, requires = "fallback")]
    fallback_gateway: Option<String>,

    /// DNS servers for --static or --dhcp (repeat or separate with commas)
    #[arg(long, value_delimiter = ',', conflicts_with_all = ["factory_default", "suggested"])]
    dns: Vec<String>,

    /// Reset to the camera's factory network settings (usually DHCP with a fallback address)
    #[arg(long = "default")]
    factory_default: bool,
    /// Use the settings suggested for this PC's network
    #[arg(long)]
    suggested: bool,

    /// User name for the camera's web interface
    #[arg(long, default_value = "admin")]
    user: String,
    /// Read the password from the first line of stdin instead of asking
    #[arg(long)]
    password_stdin: bool,
    /// Trust the camera key with this fingerprint without asking (new or changed key)
    #[arg(long, value_name = "SHA256:...")]
    fingerprint: Option<String>,
    /// Apply without asking for confirmation
    #[arg(short, long)]
    yes: bool,
    /// Do not wait for the camera to answer with its new settings
    #[arg(long)]
    no_wait: bool,
}

type Result<T, E = String> = std::result::Result<T, E>;

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli).await {
        Ok(code) => code,
        Err(message) => {
            eprintln!("Error: {message}");
            ExitCode::FAILURE
        }
    }
}

async fn run(cli: Cli) -> Result<ExitCode> {
    let trust_file = cli.trust_file.or_else(trust::default_file).ok_or("No configuration directory found.")?;
    let trust = Arc::new(Mutex::new(TrustStore::load(trust_file)));

    let (tx, rx) = mpsc::unbounded_channel();
    let emit: Emit = Arc::new(move |event| {
        let _ = tx.send(event);
    });
    let scanner = Scanner::new(trust.clone(), emit, tokio::runtime::Handle::current());
    let actions = Actions::new(scanner.clone(), trust);
    let json = cli.json;

    match cli.command {
        Command::List => list(&scanner, json).await,
        Command::Watch => watch(&scanner, rx, json).await,
        Command::Open { camera, print } => open_camera(&scanner, &camera, print, json).await,
        Command::Identify { camera } => identify(&scanner, &actions, &camera, json).await,
        Command::SetIp(args) => set_ip(&scanner, &actions, args, json).await,
    }
}

// --- Finding cameras ---

/// Run one scan and wait until it is complete.
async fn scan(scanner: &Scanner) {
    scanner.refresh();
    let _ = tokio::time::timeout(SCAN_TIMEOUT, scanner.settled()).await;
}

fn address(camera: &Camera) -> &str {
    let address = camera.fdp.as_ref().and_then(|f| f.net.address.as_deref()).unwrap_or(&camera.ip);
    if address.is_empty() { "no address" } else { address }
}

fn matches(camera: &Camera, selector: &str) -> bool {
    let name = selector.to_lowercase();
    let name = name.strip_suffix(".local").unwrap_or(&name);
    camera.id == selector
        || camera.hostname.to_lowercase() == name
        || camera.ip == selector
        || camera.fdp.as_ref().is_some_and(|f| {
            f.serial == selector
                || f.mac.eq_ignore_ascii_case(selector)
                || f.net.address.as_deref().map(fdp::strip_prefix) == Some(selector)
        })
}

/// Scan until the camera is found. An FDP camera is unique by serial number,
/// so it is returned right away; a match on an mDNS-only entry waits for the
/// complete scan, because FDP may still report the same camera.
async fn find_camera(scanner: &Scanner, selector: &str) -> Result<Camera> {
    eprintln!("Searching for {selector}...");
    scanner.refresh();
    let deadline = Instant::now() + SCAN_TIMEOUT;
    let settled = scanner.settled();
    tokio::pin!(settled);
    let mut done = false;
    loop {
        let found: Vec<Camera> = scanner.get_all().into_iter().filter(|c| matches(c, selector)).collect();
        if let [camera] = found.as_slice()
            && (done || camera.fdp.is_some())
        {
            return Ok(camera.clone());
        }
        if done {
            return Err(match found.len() {
                0 => format!("No camera \"{selector}\" found."),
                _ => format!(
                    "\"{selector}\" matches several cameras: {}. Use the serial number or IP address.",
                    found.iter().map(|c| format!("{} ({})", c.hostname, address(c))).collect::<Vec<_>>().join(", ")
                ),
            });
        }
        tokio::select! {
            _ = &mut settled => done = true,
            _ = sleep_until(deadline) => done = true,
            _ = sleep(Duration::from_millis(100)) => {}
        }
    }
}

// --- list / watch ---

fn status_label(camera: &Camera) -> &'static str {
    match camera.status {
        CameraStatus::Online => "online",
        CameraStatus::OtherSubnet => "not-reachable",
        CameraStatus::Unreachable => "no-web-ui",
        CameraStatus::Offline => "offline",
    }
}

fn notes(camera: &Camera) -> String {
    let Some(info) = &camera.fdp else { return "older firmware (mDNS only)".into() };
    let mut notes = Vec::new();
    if info.key_trust == KeyTrust::Mismatch {
        notes.push("KEY CHANGED".to_string());
    }
    if info.version.is_none() {
        notes.push("incompatible protocol version".into());
    }
    if camera.status == CameraStatus::OtherSubnet {
        notes.push(format!("outside this PC's network {}", info.local_cidr));
    }
    notes.join(", ")
}

fn print_table(cameras: &[Camera]) {
    let dash = |s: &str| if s.is_empty() { "-".to_string() } else { s.to_string() };
    let mut rows = vec![["STATUS", "NAME", "ADDRESS", "SERIAL", "MODEL", "FIRMWARE", "NOTES"].map(String::from)];
    for c in cameras {
        let info = c.fdp.as_ref();
        rows.push([
            status_label(c).into(),
            c.hostname.clone(),
            address(c).into(),
            dash(info.map_or("", |i| &i.serial)),
            dash(info.map_or("", |i| &i.model)),
            dash(info.map_or("", |i| &i.fw_version)),
            notes(c),
        ]);
    }
    let widths: Vec<usize> =
        (0..rows[0].len()).map(|i| rows.iter().map(|r| r[i].chars().count()).max().unwrap_or(0)).collect();
    for row in rows {
        let line: Vec<String> = row.iter().zip(&widths).map(|(cell, w)| format!("{cell:<w$}")).collect();
        println!("{}", line.join("  ").trim_end());
    }
}

fn sorted(mut cameras: Vec<Camera>) -> Vec<Camera> {
    cameras.sort_by(|a, b| a.hostname.cmp(&b.hostname).then_with(|| a.id.cmp(&b.id)));
    cameras
}

async fn list(scanner: &Scanner, json: bool) -> Result<ExitCode> {
    eprintln!("Scanning...");
    scan(scanner).await;
    let cameras = sorted(scanner.get_all());
    if json {
        println!("{}", serde_json::to_string_pretty(&cameras).unwrap_or_default());
        return Ok(ExitCode::SUCCESS);
    }
    if cameras.is_empty() {
        eprintln!("No cameras found.");
        return Ok(ExitCode::SUCCESS);
    }
    print_table(&cameras);
    if cameras.iter().any(|c| c.status == CameraStatus::OtherSubnet) {
        eprintln!(
            "\nCameras outside this PC's network: move them with `ecm-discovery-cli set-ip <SERIAL> --suggested`."
        );
    }
    Ok(ExitCode::SUCCESS)
}

async fn watch(scanner: &Scanner, mut rx: mpsc::UnboundedReceiver<Event>, json: bool) -> Result<ExitCode> {
    eprintln!("Scanning continuously, press Ctrl+C to stop.");
    scanner.start();
    while let Some(event) = rx.recv().await {
        let (kind, camera) = match event {
            Event::Found(c) => ("found", c),
            Event::Updated(c) => ("updated", c),
            Event::Removed(id) => {
                // A legacy mDNS entry that turned out to be an FDP camera
                if json {
                    println!("{}", json!({ "event": "removed", "id": id }));
                }
                continue;
            }
        };
        if json {
            // "event" first, so the lines are easy to filter
            println!(r#"{{"event":"{kind}","camera":{}}}"#, serde_json::to_string(&camera).unwrap_or_default());
        } else {
            let notes = notes(&camera);
            let notes = if notes.is_empty() { String::new() } else { format!("  ({notes})") };
            println!("{kind:<8} {:<14} {:<24} {}{notes}", status_label(&camera), camera.hostname, address(&camera));
        }
    }
    Ok(ExitCode::SUCCESS)
}

// --- open / identify ---

fn web_url(camera: &Camera) -> String {
    let default_port = if camera.scheme == "https" { 443 } else { 80 };
    let port = if camera.port == default_port { String::new() } else { format!(":{}", camera.port) };
    format!("{}://{}{port}", camera.scheme, camera.ip)
}

async fn open_camera(scanner: &Scanner, selector: &str, print: bool, json: bool) -> Result<ExitCode> {
    let camera = find_camera(scanner, selector).await?;
    let url = web_url(&camera);
    if json {
        println!("{}", json!({ "url": url, "online": camera.online }));
    } else if print {
        println!("{url}");
    }
    if print || json {
        return Ok(ExitCode::SUCCESS);
    }
    if !camera.online {
        let hint = if camera.fdp.is_some() { " Use `set-ip` to move it into this PC's network." } else { "" };
        return Err(format!(
            "The web interface of {} ({}) does not respond from this PC.{hint}",
            camera.hostname,
            address(&camera)
        ));
    }
    open::that_detached(&url).map_err(|err| format!("Cannot open {url}: {err}"))?;
    println!("Opened {url}");
    Ok(ExitCode::SUCCESS)
}

async fn identify(scanner: &Scanner, actions: &Actions, selector: &str, json: bool) -> Result<ExitCode> {
    let camera = find_camera(scanner, selector).await?;
    if camera.fdp.is_none() {
        return Err(format!("{} has an older firmware that does not support identify.", camera.hostname));
    }
    let result = actions.identify(&camera.id).await;
    if json {
        println!("{}", serde_json::to_string_pretty(&result).unwrap_or_default());
    } else if result.ok {
        println!("{}: {}", camera.hostname, result.message);
    } else {
        eprintln!("{}: {}", camera.hostname, result.message);
    }
    Ok(if result.ok { ExitCode::SUCCESS } else { ExitCode::FAILURE })
}

// --- set-ip ---

/// Why Change IP is not possible for this camera (same reasons as in the app).
fn set_ip_unavailable(camera: &Camera) -> Option<String> {
    const USE_WEB_UI: &str = "You can change the network settings in the camera's web interface instead.";
    let Some(info) = &camera.fdp else {
        return Some(format!("Changing the IP address from this tool needs a newer camera firmware. {USE_WEB_UI}"));
    };
    if !info.caps.iter().any(|c| c == "set_ip") {
        return Some(format!("This camera does not support changing the IP address from this tool. {USE_WEB_UI}"));
    }
    if info.version.is_none() {
        return Some(version_message(&info.versions));
    }
    if info.set_ip == "disabled" {
        return Some("Changing the IP address from this tool is disabled in the camera's web interface.".into());
    }
    None
}

fn describe(config: &NetConfig) -> String {
    let dns = if config.dns.is_empty() { String::new() } else { format!(", DNS {}", config.dns.join(", ")) };
    match config.mode {
        NetMode::Static => {
            let gateway = config.gateway.as_ref().map(|g| format!(", gateway {g}")).unwrap_or_default();
            format!("static {}{gateway}{dns}", config.address.as_deref().unwrap_or("?"))
        }
        NetMode::Dhcp => {
            let fallback = config.fallback.as_ref().map(|f| format!(", fallback {}", f.address)).unwrap_or_default();
            format!("DHCP{fallback}{dns}")
        }
        NetMode::FactoryDefault => "factory default network settings (usually DHCP with a fallback address)".into(),
    }
}

fn build_config(args: &SetIpArgs, info: &FdpInfo, suggestion: Option<NetConfig>) -> Result<NetConfig> {
    let config = |mode| NetConfig { mode, address: None, gateway: None, dns: args.dns.clone(), fallback: None };
    if args.factory_default {
        return Ok(NetConfig { dns: vec![], ..config(NetMode::FactoryDefault) });
    }
    if args.suggested {
        let s = suggestion
            .filter(|s| s.mode != NetMode::Static || s.address.is_some())
            .ok_or("There is no suggestion for this camera. Pass --static or --dhcp.")?;
        // Like the app: keep the camera's fallback unless the suggestion sets one, and its DNS servers
        return Ok(NetConfig {
            fallback: s.fallback.clone().or(info.net.fallback.clone()),
            dns: info.net.dns.clone(),
            ..s
        });
    }
    if let Some(address) = &args.static_address {
        return Ok(NetConfig {
            address: Some(address.clone()),
            gateway: args.gateway.clone(),
            ..config(NetMode::Static)
        });
    }
    let fallback =
        args.fallback.as_ref().map(|a| StaticAddress { address: a.clone(), gateway: args.fallback_gateway.clone() });
    Ok(NetConfig { fallback, ..config(NetMode::Dhcp) })
}

fn ask(question: &str) -> Result<bool> {
    eprint!("{question} [y/N] ");
    let _ = std::io::stderr().flush();
    let mut answer = String::new();
    std::io::stdin().lock().read_line(&mut answer).map_err(|e| e.to_string())?;
    Ok(matches!(answer.trim().to_lowercase().as_str(), "y" | "yes"))
}

/// Trust on first use (spec §8.5): the key must be confirmed before credentials are sent.
fn confirm_trust(camera: &Camera, prepared: &PrepareSetIpResult, given: Option<&str>, interactive: bool) -> Result<()> {
    let info = camera.fdp.as_ref().expect("FDP camera");
    let fp = prepared.fingerprint.as_deref().unwrap_or_default();
    eprintln!("\n{} · S/N {} · {}", camera.hostname, info.serial, info.mac);
    if prepared.trust == Some(KeyTrust::Mismatch) {
        eprintln!(
            "WARNING: The key of this camera differs from the one you trusted before. This is expected after a \
             factory reset of the camera. Otherwise, another device on the network may be pretending to be this \
             camera. Only continue if you know that the camera was reset.\n  Previously trusted: {}\n  Now:                {fp}\n",
            prepared.pinned_fingerprint.as_deref().unwrap_or("?")
        );
    } else {
        eprintln!(
            "This PC has not changed settings on this camera before. Its key is remembered, and you are warned if \
             it ever changes.\n  Key fingerprint: {fp}\n"
        );
    }
    if let Some(given) = given {
        return if given == fp {
            Ok(())
        } else {
            Err("The camera key does not match --fingerprint. Nothing was sent.".into())
        };
    }
    if !interactive {
        return Err(if prepared.trust == Some(KeyTrust::Mismatch) {
            "The camera key changed. Only if you know that the camera was reset, pass the new fingerprint with \
             --fingerprint."
        } else {
            "The camera key is not trusted yet. Check the fingerprint and pass it with --fingerprint."
        }
        .into());
    }
    let question = if prepared.trust == Some(KeyTrust::Mismatch) { "Trust the new key?" } else { "Trust this camera?" };
    if ask(question)? { Ok(()) } else { Err("The camera key was not confirmed. Nothing was sent.".into()) }
}

fn read_password(args: &SetIpArgs, camera: &Camera) -> Result<String> {
    if args.password_stdin {
        let mut line = String::new();
        std::io::stdin().lock().read_line(&mut line).map_err(|e| e.to_string())?;
        let password = line.trim_end_matches(['\r', '\n']).to_string();
        line.zeroize();
        return Ok(password);
    }
    if !std::io::stdin().is_terminal() {
        return Err("No terminal to ask for the password. Use --password-stdin.".into());
    }
    rpassword::prompt_password(format!("Password for {} on {}: ", args.user, camera.hostname))
        .map_err(|e| e.to_string())
}

async fn set_ip(scanner: &Scanner, actions: &Actions, args: SetIpArgs, json: bool) -> Result<ExitCode> {
    let camera = find_camera(scanner, &args.camera).await?;
    if let Some(reason) = set_ip_unavailable(&camera) {
        return Err(reason);
    }
    let info = camera.fdp.clone().expect("FDP camera");
    let interactive = std::io::stdin().is_terminal() && !args.password_stdin;

    eprintln!("Contacting {} ({})...", camera.hostname, address(&camera));
    let prepared = actions.prepare_set_ip(&camera.id).await;
    if !prepared.ok {
        return Err(prepared.error.unwrap_or_else(|| "The camera could not be contacted.".into()));
    }
    let fp = prepared.fingerprint.clone().unwrap_or_default();
    if prepared.trust == Some(KeyTrust::Match) {
        if args.fingerprint.as_ref().is_some_and(|given| *given != fp) {
            return Err("The camera key does not match --fingerprint. Nothing was sent.".into());
        }
    } else {
        confirm_trust(&camera, &prepared, args.fingerprint.as_deref(), interactive)?;
        if !actions.trust_camera(&camera.id) {
            return Err("The camera key could not be stored. Please try again.".into());
        }
    }

    let config = build_config(&args, &info, prepared.suggestion.clone())?;
    fdp::validate_config(&config).map_err(|err| format!("The network settings are invalid: {err}"))?;
    if config.mode == NetMode::Static && config.address.as_deref().map(fdp::strip_prefix) == Some(&info.local_address) {
        return Err("This is the address of your PC. Choose a different one.".into());
    }

    eprintln!("Current address: {}", info.net.address.as_deref().unwrap_or("none"));
    eprintln!("New settings:    {}", describe(&config));
    if !args.yes {
        if !interactive {
            return Err("Not applied. Pass --yes to apply without asking.".into());
        }
        if !ask("Apply?")? {
            return Err("Cancelled. Nothing was sent.".into());
        }
    }

    let password = read_password(&args, &camera)?;
    let result = actions
        .set_ip(SetIpRequest { id: camera.id.clone(), config: config.clone(), user: args.user.clone(), password })
        .await;
    let ok = result.status == "ok";
    if json {
        println!("{}", serde_json::to_string_pretty(&result).unwrap_or_default());
    }
    if !ok {
        if !json {
            eprintln!("{}", result.message.as_deref().unwrap_or(&result.status));
        }
        return Ok(ExitCode::FAILURE);
    }

    let new_address = result.net.as_ref().and_then(|n| n.address.clone()).or(config.address.clone());
    let applied = match &new_address {
        Some(a) => format!("IP configuration applied ({a})."),
        None => "IP configuration applied.".into(),
    };
    if json || args.no_wait {
        if !json {
            println!("{applied}");
        }
        return Ok(ExitCode::SUCCESS);
    }
    println!("{applied} Searching for the camera...");
    await_camera(scanner, &camera.id, new_address.as_deref().map(fdp::strip_prefix)).await;
    Ok(ExitCode::SUCCESS)
}

/// After Change IP: report once the camera answers with its new settings (like the app).
async fn await_camera(scanner: &Scanner, id: &str, address: Option<&str>) {
    let deadline = Instant::now() + AWAIT_TIMEOUT;
    sleep(REDISCOVERY_DELAY).await;
    let mut last = String::new();
    while Instant::now() < deadline {
        scan(scanner).await;
        if let Some(camera) = scanner.get_by_id(id) {
            let now = camera.fdp.as_ref().and_then(|f| f.net.address.as_deref()).map(fdp::strip_prefix);
            if address.is_none() || now == address {
                let now = now.unwrap_or("no address");
                if camera.online {
                    println!("Done. The camera is reachable at {}.", web_url(&camera));
                    return;
                }
                let text = match camera.status {
                    CameraStatus::OtherSubnet => format!(
                        "The camera now uses {now}. Its web interface does not respond from this PC; the address is \
                         outside this PC's network ({}) and no route leads there.",
                        camera.fdp.as_ref().map_or("?", |f| &f.local_cidr)
                    ),
                    _ => format!("The camera now uses {now}, but its web interface does not respond yet."),
                };
                if text != last {
                    println!("{text}");
                    last = text;
                }
            }
        }
        sleep(Duration::from_secs(1)).await;
    }
    if last.is_empty() {
        println!("The camera does not answer at its new address yet. It may need a moment.");
    }
}
