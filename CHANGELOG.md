# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.0] - 2026-07-XX

### Added
- `media_keys` module: evdev media key handling with POLLERR blacklist
- WiFi WMT power-on path for MTK devices
- Alpha-aware image blit with named constants
- `// SAFETY:` comments on all unsafe blocks

### Changed
- Audio pipeline: zero-loss tolerance, double A2DP write-ahead lead
- Synced to in-tree development source (0.3.0 release)

## [0.2.3] - 2026-0X-XX

### Added
- Pinned `kothok-edge-tts` to 0.2.8

## [0.2.2] - 2026-0X-XX

### Added
- WiFi robust reconnect with carrier polling and config discovery

## [0.2.1] - 2026-0X-XX

### Fixed
- README quick-start doctest (undefined `rgb565_bytes`)

### Added
- WiFi toggle improvements: multi-path config, wpa_cli reconnect, rfkill,
  conf fallbacks

## [0.2.0] - 2026-0X-XX

### Added
- BT discovery module (`device/bt/discover.rs`)
- Rendering loader module
- File splits for convention compliance (wifi, bt directories)

### Changed
- Breaking API changes (version bump from 0.1 to 0.2)
