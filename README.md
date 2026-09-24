# Edge Camera Discovery

A lightweight desktop application for discovering **3dvisionlabs ECM cameras** on your local network. The app automatically finds cameras via mDNS, displays their hostname and IP address, and lets you open the camera's web interface in one click.

Runs on **Windows**, **macOS**, and **Linux**.

---

## How It Works

ECM cameras broadcast their presence on the local network using **mDNS/Bonjour** (the same protocol used by printers and AirPlay devices). Edge Camera Discovery listens for these announcements, filters for ECM camera hostnames (`ecm-*`), and displays all found cameras in a list.

- New cameras appear automatically — no manual IP entry required
- Online/offline status is monitored via TCP health checks every 10 seconds
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

- The computer running Edge Camera Discovery must be on the **same local network subnet** as the cameras
- mDNS uses multicast UDP on port 5353 — ensure your firewall or managed switch does not block multicast traffic

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
3. Each row shows the camera hostname, IP address, and online status
   - **Green dot** — camera is online and reachable
   - **Grey dot** — camera is offline or unreachable
4. Click **Open** to launch the camera's web interface in your browser
5. Click the **↻ refresh** button to re-scan and remove offline cameras from the list

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

### Linux with fractional display scaling

On Wayland, GTK3 only supports integer scale factors, so text in the app would look grainy at e.g. 110 % scaling. The app therefore runs via XWayland when it is available. Set `ECM_NATIVE_WAYLAND=1` to run it natively on Wayland.

---

## Tech Stack

- [Tauri 2](https://v2.tauri.app/): Rust backend, system web view for the UI (WebView2, WKWebView, WebKitGTK)
- Plain HTML/CSS/TypeScript UI, bundled with [Vite](https://vite.dev/) — no UI framework
- Platform-native mDNS: `dns-sd` (macOS), `avahi-browse` (Linux), raw mDNS query via UDP multicast (Windows)

---

## License

MIT © [3dvisionlabs GmbH](https://www.3dvisionlabs.com)
