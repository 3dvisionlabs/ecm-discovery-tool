# Changelog

All notable changes to Edge Camera Discovery are documented here.
Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

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
