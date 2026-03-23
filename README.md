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
| Windows | Windows 10 or later |
| macOS | macOS 10.15 (Catalina) or later |
| Linux (Debian/Ubuntu) | `avahi-utils` (`sudo apt install avahi-utils`) |
| Linux (Fedora/RHEL) | `avahi-tools` (`sudo dnf install avahi-tools`) |

The Linux `.deb` and `.rpm` packages declare `avahi-utils` / `avahi-tools` as a dependency and will install it automatically. For `.AppImage`, install it manually.

### Network

- The computer running Edge Camera Discovery must be on the **same local network subnet** as the cameras
- mDNS uses multicast UDP on port 5353 — ensure your firewall or managed switch does not block multicast traffic

---

## Installation

Download the installer for your platform from the [Releases](../../releases) page.

| Platform | File | Notes |
|---|---|---|
| Windows | `Edge Camera Discovery-x.x.x Setup.exe` | Installs to user profile, no admin rights needed |
| Windows (portable) | `Edge Camera Discovery-win32-x64-x.x.x.zip` | Extract and run, no installation |
| macOS | `Edge Camera Discovery-x.x.x-arm64.dmg` | Drag to Applications |
| macOS (zip) | `Edge Camera Discovery-darwin-arm64-x.x.x.zip` | Extract and run |
| Linux | `ecm-discovery_x.x.x_amd64.deb` | Debian/Ubuntu |
| Linux | `ecm-discovery-x.x.x.x86_64.rpm` | Fedora/RHEL/openSUSE |
| Linux | `Edge Camera Discovery-x.x.x-x64.AppImage` | Universal (any distro) |

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

### Prerequisites (all platforms)

- [Node.js](https://nodejs.org/) 18 or later
- npm (included with Node.js)

Clone the repository and install dependencies:

```bash
git clone <repo-url>
cd ecm-discovery-tool
npm install
```

### Development

```bash
npm start
```

Launches the app with hot-reload. Note: the dock/taskbar tooltip shows "Electron" in dev mode — this is an Electron limitation. Packaged builds show the correct app name.

---

### Build — macOS

Requires a Mac. Produces `.dmg` and `.zip`.

```bash
./scripts/build-mac.sh
```

This script installs dependencies, generates the `.icns` icon from the iconset, and runs `electron-forge make`.

**Output** (exact path and arch suffix depend on your Mac's CPU):
```
out/make/Edge Camera Discovery-x.x.x-arm64.dmg       ← Apple Silicon
out/make/zip/darwin/arm64/Edge Camera Discovery-darwin-arm64-x.x.x.zip
```

---

### Build — Windows

Requires a Windows machine. Produces a Squirrel installer `.exe` and a portable `.zip`.

```powershell
powershell -ExecutionPolicy Bypass -File scripts\build-windows.ps1
```

**Output:**
```
out/make/squirrel.windows/x64/Edge Camera Discovery-x.x.x Setup.exe
out/make/zip/win32/x64/Edge Camera Discovery-win32-x64-x.x.x.zip
```

---

### Build — Linux

Can be run on any Linux x64 machine. Produces `.deb`, `.rpm`, and `.AppImage`.

```bash
npm run make:linux
```

**Output:**
```
out/make/deb/x64/ecm-discovery_x.x.x_amd64.deb
out/make/rpm/x64/ecm-discovery-x.x.x-1.x86_64.rpm
out/make/AppImage/x64/Edge Camera Discovery-x.x.x-x64.AppImage
```

> Cross-compilation is not supported — each platform must be built on its native OS.

---

## Tech Stack

- [Electron](https://www.electronjs.org/) + TypeScript
- [electron-forge](https://www.electronforge.io/) for build pipeline and packaging
- Platform-native mDNS: `dns-sd` (macOS), `avahi-browse` (Linux), `dns-packet` + raw UDP multicast (Windows)
- Plain HTML/CSS renderer — no UI framework

---

## License

MIT © [3dvisionlabs GmbH](https://www.3dvisionlabs.com)
