# chime

CLI utility for playing musical tones through the sound card. Designed for integration with [Claude Code](https://docs.anthropic.com/en/docs/claude-code) hooks and other shell automations.

## Installation

```bash
cargo install chime-cli
```

The crate is published as `chime-cli` (the name `chime` was already taken on crates.io), but it installs a binary called `chime`.

Or from git / source:

```bash
cargo install --git https://github.com/holovskyi/chime
# or
git clone https://github.com/holovskyi/chime
cd chime
cargo install --path .
```

## Usage

### Notes

Play notes by name with optional duration in milliseconds (default 500ms):

```bash
chime C5:150 E5:150 G5:150
```

Use raw frequencies:

```bash
chime 440:200 880:200
```

### Presets

```bash
chime --preset start      # C5, G5 — start signal
chime --preset success    # C5, E5, G5 — task completed
chime --preset fail       # G4, Eb4, C4 (triangle wave) — something went wrong
chime --preset goal       # E5, A5 — goal reached
chime --preset reminder   # A4 — notification
```

### Options

| Flag                 | Default | Description                                |
|----------------------|---------|--------------------------------------------|
| `--wave`             | sine    | Waveform: sine, triangle, square, sawtooth |
| `--volume`           | 0.3     | Volume 0.0-1.0                             |
| `--gap`              | 150     | Gap between note starts (ms)               |
| `--preset`           | -       | Named preset instead of notes              |
| `--config`           | -       | Explicit path to config file               |
| `--show-config`      | -       | List config search paths and which file is loaded, then exit |
| `--show-effective`   | -       | Print resolved settings for the invocation and exit |
| `--dump-config`      | -       | Print current effective settings as a TOML preset block and exit |
| `--dump-full-config` | -       | Print all built-in + config presets (with `[defaults]`) as TOML and exit |

### Note format

- Note name with octave: `C4`, `Eb4`, `F#5`, `A4`
- Supported: C, C#, Db, D, D#, Eb, E, F, F#, Gb, G, G#, Ab, A, A#, Bb, B
- Duration after `:` in ms (optional, default 500ms)
- Raw frequency: `440:200`

## Config file

Chime looks for a TOML config file to load custom presets. Discovery order (first found wins):

1. `--config <path>` — explicit path
2. `.chime.toml` — walk up from current directory through parents
3. Platform config dir — `%APPDATA%\chime\config.toml` (Windows), `~/.config/chime/config.toml` (Linux/macOS)

Example config:

```toml
[defaults]
volume = 0.5
wave = "triangle"
# gap = 200          # optional

[presets.my-chord]
notes = ["A4:200", "C5:200", "E5:200", "A5:400"]
gap = 100            # overrides the [defaults] gap
# wave/volume inherited from [defaults]

[presets.success]    # overrides the built-in preset
notes = ["C6:300", "E6:300", "G6:300"]
```

Preset fields: `notes` (required), `wave` / `volume` / `gap` (optional). The `[defaults]` section sets `wave` / `volume` / `gap` shared by all custom presets and bare-notes invocations.

**Precedence (first set wins):** CLI flag → preset field → `[defaults]` → built-in default (sine, 0.3, 150ms).

Built-in presets (`start`, `goal`, `success`, `fail`, `reminder`) keep their own hardcoded values and are not affected by `[defaults]`. To customize them, define a preset with the same name in your config file.

### Inspecting configuration

```bash
chime --show-config             # show search paths + loaded file
chime --preset success --show-effective    # resolved notes/wave/volume/gap
chime --preset success --dump-config       # one preset as TOML
chime --dump-full-config        # all presets + [defaults] as TOML
```

## Claude Code hooks integration

Add to `~/.claude/settings.json`:

```json
{
  "hooks": {
    "Stop": [
      {
        "matcher": "",
        "hooks": [
          {
            "type": "command",
            "command": "chime --preset success"
          }
        ]
      }
    ],
    "Notification": [
      {
        "matcher": "",
        "hooks": [
          {
            "type": "command",
            "command": "chime --preset reminder"
          }
        ]
      }
    ]
  }
}
```

## How it works

- Audio output via [rodio](https://github.com/RustAudio/rodio) (cross-platform: Windows, macOS, Linux)
- Custom `rodio::Source` generating samples with configurable waveform and exponential decay envelope
- Notes overlap when duration exceeds gap, producing smooth chords
