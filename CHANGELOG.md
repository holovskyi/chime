# Changelog

## [0.1.0] - 2026-04-09

### Added
- Tone playback through the default audio output via [rodio](https://github.com/RustAudio/rodio)
- Four waveforms: sine, triangle, square, sawtooth
- Note input by name with octave (`C5`, `Eb4`, `F#5`) or by raw frequency (`440`)
- Optional per-note duration via `:ms` suffix (`C5:200`)
- Built-in presets: `start`, `goal`, `success`, `fail`, `reminder`
- TOML config file with discovery: `--config` flag, `.chime.toml` walk-up from CWD, then platform config dir (`%APPDATA%\chime\config.toml` / `~/.config/chime/config.toml`)
- Custom presets in `[presets.<name>]`, including overrides of built-in preset names
- `[defaults]` config section for global `wave` / `volume` / `gap` shared by all custom presets and bare-notes invocations
- CLI flags: `--wave`, `--volume`, `--gap`, `--preset`, `--config`, `--version`
- Inspection flags:
  - `--show-config` — list config search paths and report which file (if any) is loaded
  - `--show-effective` — print the resolved settings for the current invocation
  - `--dump-config` — emit current effective settings as a valid TOML preset block
  - `--dump-full-config` — emit all built-in and config-defined presets plus `[defaults]` as a complete TOML config
- Claude Code hooks integration documentation
