# Edge Camera Discovery

A lightweight desktop application for discovering **3dvisionlabs ECM cameras** on your local network. The app automatically finds cameras, displays their hostname and IP address, and lets you open the camera's web interface in one click. Cameras with a current firmware are found even if their IP address does not fit your network, and you can move them into your network with **Change IP**.

A command line version (`ecm-discovery-cli`) offers the same functions for scripts and headless machines.

Runs on **Windows**, **macOS**, and **Linux**.

---

## How It Works

The app uses two ways to find cameras at the same time:

- **Ferndale Discovery Protocol (FDP)** — cameras with a current firmware answer a broadcast on the local Ethernet segment (UDP port 27270), **regardless of their IP configuration**. A camera with factory settings on a direct cable (static fallback `192.168.1.100`, while the PC has a `169.254.x.x` address) shows up as well. These cameras also report serial number, model and firmware version, and support **Change IP** and **Identify**.
- **mDNS/Bonjour** — for cameras with an older firmware. They announce themselves under their hostname (`ecm-*`); this only works within the same subnet.

A camera that answers both ways is shown once.

- New cameras appear automatically — no manual IP entry required
- The web interface of every camera is checked every 10 seconds
- Clicking **Open** launches the camera's web interface (`https://<IP>`) in your default browser

> The camera web interface default credentials are `admin` / `3dvl`.

---

## Requirements

### Runtime

| Platform | Requirement |
|---|---|
| Windows | Windows 10 or later with Microsoft Edge WebView2 (preinstalled on Windows 11 and current Windows 10; the installer adds it if missing) |
| macOS | macOS 10.15 (Catalina) or later |
| Linux (Debian/Ubuntu) | `avahi-utils` and WebKitGTK 4.1 (`sudo apt install avahi-utils libwebkit2gtk-4.1-0`) |
| Linux (Fedora/RHEL) | `avahi-tools` and WebKitGTK 4.1 (`sudo dnf install avahi-tools webkit2gtk4.1`) |

The Linux `.deb` and `.rpm` packages declare these as dependencies and install them automatically. They also replace an installed Electron-based version (0.9.x). For `.AppImage`, install `avahi-utils` / `avahi-tools` manually.

### Network

- The computer must be on the **same Ethernet segment** as the cameras (same switch or direct cable, no router in between). Cameras with an older firmware (mDNS only) must also be in the **same subnet**.
- FDP uses UDP broadcasts on port 27270, mDNS uses multicast UDP on port 5353 — ensure your firewall or managed switch does not block them. Windows may ask whether to allow network access on first start.

---

## Installation

Download the installer for your platform from the [Releases](../../releases) page.

| Platform | File | Notes |
|---|---|---|
| Windows | `Edge Camera Discovery_x.x.x_x64-setup.exe` | Installs to user profile, no admin rights needed |
| Windows (portable) | `Edge Camera Discovery_x.x.x_x64-portable.zip` | Extract and run, no installation (needs WebView2) |
| macOS | `Edge Camera Discovery_x.x.x_aarch64.dmg` | Apple Silicon. Drag to Applications |
| macOS (zip) | `Edge Camera Discovery_x.x.x_aarch64.app.zip` | Apple Silicon. Extract and run |
| Linux | `Edge Camera Discovery_x.x.x_amd64.deb` | Debian/Ubuntu |
| Linux | `Edge Camera Discovery-x.x.x-1.x86_64.rpm` | Fedora/RHEL/openSUSE |
| Linux | `Edge Camera Discovery_x.x.x_amd64.AppImage` | Universal (any distro) |
| Command line | `ecm-discovery-cli_x.x.x_<platform>` (`.tar.gz` / `.zip`) | Single executable, see [Command Line](#command-line) |

---

## Security Warning on First Launch

The distributed binaries are **not code-signed**. macOS and Windows will show a security warning the first time you run the app.

### macOS — Gatekeeper

macOS will block the app with *"Edge Camera Discovery cannot be opened because it is from an unidentified developer."*

**To open it:**
1. In Finder, right-click (or Control-click) the app → **Open**
2. Click **Open** in the confirmation dialog

You only need to do this once. After that, the app opens normally.

Alternatively, remove the quarantine attribute via Terminal:
```bash
xattr -dr com.apple.quarantine "/Applications/Edge Camera Discovery.app"
```

### Windows — SmartScreen

Windows may show *"Windows protected your PC"* when running the installer.

**To proceed:**
1. Click **More info**
2. Click **Run anyway**

### Build from source

If you prefer not to bypass OS security warnings, you can build the app yourself from source — the build instructions are below. You get a binary built on your own machine that macOS and Windows treat as locally built (no warning).

---

## Usage

1. Launch **Edge Camera Discovery**
2. The app immediately begins scanning — cameras appear within a few seconds
3. Each row shows the camera hostname and IP address; cameras with a current firmware also show serial number, model and firmware version
   - **Green dot** — the web interface is reachable
   - **Amber dot, "Not reachable"** — the camera answers, but its address is outside your PC's network. Use **Change IP**.
   - **Amber dot, "Web UI not responding"** — the camera answers, but its web interface does not
   - **Grey dot, "Offline"** — the camera no longer answers
4. Click **Open** to launch the camera's web interface in your browser
5. Click the **↻ refresh** button to re-scan and remove offline cameras from the list

### Change IP

For cameras with a current firmware, **Change IP** sets the camera's network configuration without the web interface: a static address, DHCP (with an optional fallback address), or the camera's factory settings (**Factory default**). If the camera is not reachable from your PC, the dialog suggests a suitable setting that you can take over with **Use**.

- You confirm the change with the user name and password of the camera's web interface (default `admin` / `3dvl`). They are sent encrypted and are not stored.
- The first time you change a camera, the app shows the camera's key fingerprint and asks whether to trust it. If the key changes later, the app warns you: this is expected after a factory reset of the camera, otherwise another device may be pretending to be the camera.
- Changing the IP address from the app can be disabled in the camera's web interface.

### Identify

The **lamp button** makes the camera's status LEDs flash for 10 seconds, so you can tell which camera in the list is which. Only shown for cameras that support it.

---

## Command Line

`ecm-discovery-cli` offers the same functions without a window, e.g. for scripts or headless machines. It is a single executable without dependencies (on Linux, mDNS discovery uses `avahi-browse` like the app). Download the archive for your platform from the [Releases](../../releases) page, extract it and put `ecm-discovery-cli` somewhere in your `PATH`. On macOS, remove the quarantine attribute once: `xattr -d com.apple.quarantine ecm-discovery-cli`.

```bash
ecm-discovery-cli list                      # scan once and list all cameras
ecm-discovery-cli watch                     # keep scanning and print changes
ecm-discovery-cli open ecm-244260001238     # open the web interface in the browser
ecm-discovery-cli open 244260001238 --print # only print the URL
ecm-discovery-cli identify 244260001238     # flash the camera LED
```

A camera can be given by serial number, hostname, IP address or MAC address. Add `--json` for machine-readable output (`watch --json` prints one JSON object per line).

**Change IP** (cameras with a current firmware), the same as the **Change IP** dialog of the app:

```bash
ecm-discovery-cli set-ip 244260001238 --suggested              # settings suggested for this PC's network
ecm-discovery-cli set-ip 244260001238 --static 192.168.1.50/24 --gateway 192.168.1.1 --dns 192.168.1.1
ecm-discovery-cli set-ip 244260001238 --dhcp --fallback 192.168.1.100/24
ecm-discovery-cli set-ip 244260001238 --default                # factory network settings
```

The tool asks for the password of the camera's web interface (user `admin` unless `--user` is given) and for confirmation. The first time you change a camera, it shows the camera's key fingerprint and asks whether to trust it; if the key changes later, it warns you. Trusted keys are shared with the app.

In scripts, pass the password on stdin and confirm the key explicitly:

```bash
echo "$PASSWORD" | ecm-discovery-cli set-ip 244260001238 --dhcp \
    --password-stdin --fingerprint SHA256:vXOB6rCNLWJnXCfHnpmCFX5xYXoJWIZR2Ku6xQPTmSM --yes
```

The exit code is 0 on success and 1 on any error. `ecm-discovery-cli <command> --help` lists all options.

---

## Building from Source

### Prerequisites

- [Node.js](https://nodejs.org/) 24 or later (version in `.nvmrc`) with npm
- [Rust](https://www.rust-lang.org/tools/install) (stable)
- Platform build dependencies for Tauri, see [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/). On Debian/Ubuntu:
  ```bash
  sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev
  ```

Clone the repository and install dependencies:

```bash
git clone <repo-url>
cd ecm-discovery-tool
npm install
```

### Development

```bash
npm start          # app with hot-reload of the UI
npm test           # Rust unit tests (protocol test vectors, parsers)
npm run lint       # ESLint for the TypeScript UI
```

### Build

```bash
npm run make
```

Builds the installers for the platform you are on (cross-compilation is not supported):

| Platform | Output in `target/release/bundle/` |
|---|---|
| Windows | `nsis/Edge Camera Discovery_x.x.x_x64-setup.exe` |
| macOS | `dmg/Edge Camera Discovery_x.x.x_aarch64.dmg`, `macos/Edge Camera Discovery.app` |
| Linux | `deb/…amd64.deb`, `rpm/…x86_64.rpm`, `appimage/…amd64.AppImage` |

### Command line tool

```bash
cargo build --release -p ecm-discovery-cli    # → target/release/ecm-discovery-cli
```

Only needs Rust, no Node.js or WebKitGTK.

### Linux with fractional display scaling

On Wayland, GTK3 only supports integer scale factors, so text in the app would look grainy at e.g. 110 % scaling. The app therefore runs via XWayland when it is available. Set `ECM_NATIVE_WAYLAND=1` to run it natively on Wayland.

---

## Tech Stack

- [Tauri 2](https://v2.tauri.app/): Rust backend, system web view for the UI (WebView2, WKWebView, WebKitGTK)
- Plain HTML/CSS/TypeScript UI, bundled with [Vite](https://vite.dev/) — no UI framework
- Ferndale Discovery Protocol (FDP): own UDP discovery; Change IP encrypted with X25519 + HKDF-SHA256 + AES-256-GCM ([RustCrypto](https://github.com/RustCrypto))
- Platform-native mDNS: `dns-sd` (macOS), `avahi-browse` (Linux), raw mDNS query via UDP multicast (Windows)
- Command line tool with [clap](https://github.com/clap-rs/clap), sharing the discovery code with the app

---

## License

MIT © [3dvisionlabs GmbH](https://www.3dvisionlabs.com)
