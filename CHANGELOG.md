# Changelog

All notable changes to Edge Camera Discovery are documented here.
Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

---

## [0.10.0-alpha.4] - 2026-09-29

### Changed
- The command line tool `ecm-discovery-cli` comes with the app instead of as a separate download: in `/usr/bin` with the `.deb` and `.rpm` packages, added to your `PATH` by the Windows installer, inside the app on macOS
- New Linux download: portable `.tar.gz` with app and command line tool
- File names on the Releases page show the operating system, e.g. `ecm-discovery_0.10.0_windows_x64_setup.exe`

---

## [0.10.0-alpha.3] - 2026-09-29

### Added
- Cameras with a current firmware are found even if their IP address does not fit your network (e.g. a new camera on a direct cable), via the Ferndale Discovery Protocol. mDNS discovery for cameras with an older firmware stays as before.
- Serial number, model and firmware version for these cameras
- **Change IP**: set a static address, DHCP or the factory settings without the web interface, with a suggestion for your network. Confirmed with the web interface login, sent encrypted.
- The camera key is confirmed on first use and checked on every later change; a changed key shows a warning
- **Identify**: flash the camera's status LEDs
- Command line tool `ecm-discovery-cli` (separate download): list cameras, open the web interface, identify and change the IP address, with the same trusted camera keys as the app

### Changed
- Camera list: serial number, model, firmware and IP address on separate lines; the camera state is shown as a label next to the name
- Cameras that are running but whose web interface does not respond are shown as "Web UI not responding" instead of "Offline"
- Larger window, so dialogs fit without scrolling

### Fixed
- Linux AppImage: empty window on newer distributions (e.g. Fedora 44)
- Linux AppImage: "Open" did nothing on KDE Plasma 6

---

## [0.10.0-alpha.1] - 2026-09-24

### Changed
- Rewritten with Tauri (Rust) instead of Electron: much smaller download (the `.deb` and `.rpm` packages are now about 2 MB). The interface and features stay the same.
- Windows: new installer (`…_x64-setup.exe`), still per user without admin rights. **Uninstall version 0.9.x first** (Settings → Apps), it is not replaced automatically.
- Linux: the `.deb` and `.rpm` packages replace an installed version 0.9.x automatically.
- Linux: text is sharp with fractional display scaling (e.g. 110 %) on Wayland

### Added
- macOS: Edit menu, so copy and paste work in text fields

---

## [0.9.1] - 2026-03-19

### Fixed
- Windows: cameras now appear reliably on first scan (no longer requires manual refresh)
- Windows: discovery no longer conflicts with other mDNS tools running on the same machine

### Changed
- Windows mDNS discovery rewritten to use raw UDP multicast instead of binding port 5353

---

## [0.9.0] - 2026-02-20

Initial public release — feature complete beta. Tested on macOS, Windows, and Linux.

### Added
- Automatic mDNS camera discovery on the local network
- Platform-native discovery: `dns-sd` (macOS), `avahi-browse` (Linux), `dns-packet` (Windows)
- TCP health monitoring on port 443 — online/offline status updated every 10 seconds
- Camera list showing hostname, IP address, and live status indicator
- One-click **Open** button to launch the camera web interface in the default browser
- Manual refresh to re-scan and remove offline cameras from the list
- Branded dark theme UI (Bebas Neue + Roboto Condensed fonts, 3dvisionlabs color palette)
- Platform packages:
  - macOS: `.dmg` + `.zip`
  - Windows: `Setup.exe` (Squirrel installer) + `.zip` (portable)
  - Linux: `.deb` (Debian/Ubuntu) + `.rpm` (Fedora/RHEL) + `.AppImage` (universal)
