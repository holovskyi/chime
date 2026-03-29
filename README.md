# chime

CLI utility for playing musical tones through the sound card. Designed for integration with [Claude Code](https://docs.anthropic.com/en/docs/claude-code) hooks and other shell automations.

## Installation

```bash
cargo install --git https://github.com/holovskyi/chime
```

Or build from source:

```bash
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
chime --preset fail       # G4, Eb4, C4 (triangle) — something went wrong
chime --preset goal       # E5, A5 — goal reached
chime --preset reminder   # A4 — notification
```

### Options

| Flag       | Default | Description                    |
|------------|---------|--------------------------------|
| `--wave`   | sine    | Waveform: sine, triangle, square, sawtooth |
| `--volume` | 0.3     | Volume 0.0-1.0                 |
| `--gap`    | 150     | Gap between note starts (ms)   |
| `--preset` | -       | Named preset instead of notes  |

### Note format

- Note name with octave: `C4`, `Eb4`, `F#5`, `A4`
- Supported: C, C#, Db, D, D#, Eb, E, F, F#, Gb, G, G#, Ab, A, A#, Bb, B
- Duration after `:` in ms (optional, default 500ms)
- Raw frequency: `440:200`

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
