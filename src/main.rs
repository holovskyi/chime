use std::collections::HashMap;
use std::num::NonZero;
use std::path::{Path, PathBuf};
use std::time::Duration;

use clap::{Parser, ValueEnum};
use rodio::Source;
use serde::{Deserialize, Serialize};

const DEFAULT_WAVE: Waveform = Waveform::Sine;
const DEFAULT_VOLUME: f32 = 0.3;
const DEFAULT_GAP: u64 = 150;
const DEFAULT_DURATION: u64 = 500;
const SAMPLE_RATE: u32 = 48000;
const SAMPLE_RATE_NZ: NonZero<u32> = NonZero::new(SAMPLE_RATE).unwrap();
const CHANNELS: NonZero<u16> = NonZero::new(1).unwrap();

fn fatal(msg: &str) -> ! {
    eprintln!("error: {msg}");
    std::process::exit(1);
}

// --- CLI ---

#[derive(Parser)]
#[command(about = "Play musical tones through the sound card", version)]
struct Cli {
    /// Notes: C5:150 Eb4:300 440:200 (name/freq, optional :duration_ms)
    notes: Vec<String>,

    /// Waveform shape
    #[arg(long)]
    wave: Option<Waveform>,

    /// Volume 0.0–1.0
    #[arg(long)]
    volume: Option<f32>,

    /// Gap between note starts (ms)
    #[arg(long)]
    gap: Option<u64>,

    /// Named preset instead of notes
    #[arg(long)]
    preset: Option<String>,

    /// Path to config file
    #[arg(long)]
    config: Option<PathBuf>,

    /// Show which config file is loaded and exit
    #[arg(long)]
    show_config: bool,

    /// Show effective settings and exit
    #[arg(long)]
    show_effective: bool,

    /// Dump effective settings as TOML config and exit
    #[arg(long)]
    dump_config: bool,

    /// Dump all presets (built-in + config) as TOML and exit
    #[arg(long)]
    dump_full_config: bool,
}

#[derive(Clone, Copy, ValueEnum, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum Waveform {
    Sine,
    Triangle,
    Square,
    Sawtooth,
}

impl std::fmt::Display for Waveform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Waveform::Sine => "sine",
            Waveform::Triangle => "triangle",
            Waveform::Square => "square",
            Waveform::Sawtooth => "sawtooth",
        })
    }
}

// --- Config ---

#[derive(Deserialize, Default)]
struct Config {
    #[serde(default)]
    presets: HashMap<String, PresetConfig>,
}

#[derive(Deserialize, Serialize)]
struct PresetConfig {
    notes: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    wave: Option<Waveform>,
    #[serde(skip_serializing_if = "Option::is_none")]
    volume: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    gap: Option<u64>,
}

fn find_config(explicit: Option<&Path>) -> Option<PathBuf> {
    if let Some(path) = explicit {
        if path.exists() {
            return Some(path.to_owned());
        }
        fatal(&format!("config file not found: {}", path.display()));
    }

    // Walk up from current directory
    if let Ok(mut dir) = std::env::current_dir() {
        loop {
            let candidate = dir.join(".chime.toml");
            if candidate.exists() {
                return Some(candidate);
            }
            if !dir.pop() {
                break;
            }
        }
    }

    // Platform config directory
    if let Some(config_dir) = dirs::config_dir() {
        let candidate = config_dir.join("chime").join("config.toml");
        if candidate.exists() {
            return Some(candidate);
        }
    }

    None
}

fn load_config(path: &Path) -> Config {
    let content = std::fs::read_to_string(path)
        .unwrap_or_else(|e| fatal(&format!("failed to read config {}: {e}", path.display())));
    toml::from_str(&content)
        .unwrap_or_else(|e| fatal(&format!("failed to parse config {}: {e}", path.display())))
}

// --- Note parsing ---

struct Note {
    freq: f32,
    duration_ms: u64,
}

fn note_name_to_semitone(name: &str) -> Option<(i32, usize)> {
    // Try two-char accidentals first
    if name.len() >= 2 {
        let semitone = match &name[..2] {
            "C#" | "Db" => Some(1),
            "D#" | "Eb" => Some(3),
            "F#" | "Gb" => Some(6),
            "G#" | "Ab" => Some(8),
            "A#" | "Bb" => Some(10),
            _ => None,
        };
        if let Some(s) = semitone {
            return Some((s, 2));
        }
    }
    // Single-char natural notes
    let s = match name.as_bytes().first()? {
        b'C' => 0,
        b'D' => 2,
        b'E' => 4,
        b'F' => 5,
        b'G' => 7,
        b'A' => 9,
        b'B' => 11,
        _ => return None,
    };
    Some((s, 1))
}

fn parse_note(s: &str) -> Note {
    let (tone, duration_ms) = match s.split_once(':') {
        Some((t, d)) => (
            t,
            d.parse::<u64>()
                .unwrap_or_else(|e| fatal(&format!("invalid duration in '{s}': {e}"))),
        ),
        None => (s, DEFAULT_DURATION),
    };

    // Try parsing as raw frequency
    if let Ok(freq) = tone.parse::<f32>() {
        if !freq.is_finite() || freq <= 0.0 {
            fatal(&format!("frequency must be a positive number, got '{tone}'"));
        }
        return Note { freq, duration_ms };
    }

    // Parse note name + octave
    let (semitone, name_len) =
        note_name_to_semitone(tone).unwrap_or_else(|| fatal(&format!("unknown note: '{tone}'")));
    let octave: i32 = tone[name_len..]
        .parse()
        .unwrap_or_else(|e| fatal(&format!("invalid octave in '{tone}': {e}")));

    // A4 = 440Hz, semitone 9 in octave 4
    let semitones_from_a4 = (octave - 4) * 12 + semitone - 9;
    let freq = 440.0 * 2.0_f32.powf(semitones_from_a4 as f32 / 12.0);

    Note { freq, duration_ms }
}

// --- Tone source ---

struct ToneSource {
    sample_i: u32,
    frequency: f32,
    duration_samples: u32,
    duration_ms: u64,
    waveform: Waveform,
    volume: f32,
    decay_rate: f32,
}

impl ToneSource {
    fn new(freq: f32, duration_ms: u64, waveform: Waveform, volume: f32) -> Self {
        let duration_samples = (SAMPLE_RATE as u64 * duration_ms / 1000) as u32;
        let duration_secs = duration_samples as f32 / SAMPLE_RATE as f32;
        Self {
            sample_i: 0,
            frequency: freq,
            duration_samples,
            duration_ms,
            waveform,
            volume,
            decay_rate: 5.0 / duration_secs,
        }
    }
}

impl Iterator for ToneSource {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        if self.sample_i >= self.duration_samples {
            return None;
        }

        let t = self.sample_i as f32 / SAMPLE_RATE as f32;
        let ft = self.frequency * t;
        let phase = 2.0 * std::f32::consts::PI * ft;

        let wave = match self.waveform {
            Waveform::Sine => phase.sin(),
            Waveform::Triangle => 4.0 * (ft - (ft + 0.75).floor() + 0.25).abs() - 1.0,
            Waveform::Square => phase.sin().signum(),
            Waveform::Sawtooth => 2.0 * (ft - (ft + 0.5).floor()),
        };

        let envelope = (-self.decay_rate * t).exp();

        self.sample_i += 1;
        Some(wave * self.volume * envelope)
    }
}

impl Source for ToneSource {
    fn current_span_len(&self) -> Option<usize> {
        Some(self.duration_samples.saturating_sub(self.sample_i) as usize)
    }

    fn channels(&self) -> NonZero<u16> {
        CHANNELS
    }

    fn sample_rate(&self) -> NonZero<u32> {
        SAMPLE_RATE_NZ
    }

    fn total_duration(&self) -> Option<Duration> {
        Some(Duration::from_millis(self.duration_ms))
    }
}

// --- Presets ---

struct ResolvedPreset {
    notes: Vec<String>,
    wave: Waveform,
    volume: f32,
    gap: u64,
}

impl Default for ResolvedPreset {
    fn default() -> Self {
        Self {
            notes: Vec::new(),
            wave: DEFAULT_WAVE,
            volume: DEFAULT_VOLUME,
            gap: DEFAULT_GAP,
        }
    }
}

const BUILTIN_PRESET_NAMES: &[&str] = &["start", "goal", "success", "fail", "reminder"];

fn builtin_preset(name: &str) -> Option<ResolvedPreset> {
    let (notes, wave) = match name {
        "start" => (vec!["C5", "G5"], DEFAULT_WAVE),
        "goal" => (vec!["E5", "A5"], DEFAULT_WAVE),
        "success" => (vec!["C5", "E5", "G5"], DEFAULT_WAVE),
        "fail" => (vec!["G4", "Eb4", "C4"], Waveform::Triangle),
        "reminder" => (vec!["A4"], DEFAULT_WAVE),
        _ => return None,
    };
    Some(ResolvedPreset {
        notes: notes.into_iter().map(String::from).collect(),
        wave,
        ..Default::default()
    })
}

fn resolve_preset(name: &str, config: Option<&Config>) -> ResolvedPreset {
    if let Some(cfg) = config
        && let Some(preset) = cfg.presets.get(name)
    {
        if preset.notes.is_empty() {
            fatal(&format!("preset '{name}' has no notes"));
        }
        if let Some(v) = preset.volume
            && !(0.0..=1.0).contains(&v)
        {
            fatal(&format!("preset '{name}': volume must be between 0.0 and 1.0, got {v}"));
        }
        return ResolvedPreset {
            notes: preset.notes.clone(),
            wave: preset.wave.unwrap_or(DEFAULT_WAVE),
            volume: preset.volume.unwrap_or(DEFAULT_VOLUME),
            gap: preset.gap.unwrap_or(DEFAULT_GAP),
        };
    }

    builtin_preset(name).unwrap_or_else(|| fatal(&format!("unknown preset '{name}'")))
}

// --- Main ---

fn main() {
    let cli = Cli::parse();

    let config_path = find_config(cli.config.as_deref());

    if cli.show_config {
        match &config_path {
            Some(p) => println!("{}", p.display()),
            None => println!("no config file found"),
        }
        return;
    }

    let config = config_path.map(|p| load_config(&p));

    if cli.dump_full_config {
        use std::collections::BTreeMap;
        let mut presets = BTreeMap::new();
        for &name in BUILTIN_PRESET_NAMES {
            let r = builtin_preset(name).unwrap();
            presets.insert(name, PresetConfig {
                notes: r.notes,
                wave: Some(r.wave),
                volume: Some(r.volume),
                gap: Some(r.gap),
            });
        }
        if let Some(cfg) = &config {
            for (name, preset) in &cfg.presets {
                presets.insert(name, PresetConfig {
                    notes: preset.notes.clone(),
                    wave: preset.wave.or(Some(DEFAULT_WAVE)),
                    volume: preset.volume.or(Some(DEFAULT_VOLUME)),
                    gap: preset.gap.or(Some(DEFAULT_GAP)),
                });
            }
        }
        let wrapper = HashMap::from([("presets", presets)]);
        print!("{}", toml::to_string_pretty(&wrapper).expect("failed to serialize config"));
        return;
    }

    let base = match (cli.preset.as_deref(), cli.notes.is_empty()) {
        (Some(_), false) => fatal("cannot use both --preset and positional notes"),
        (Some(name), true) => resolve_preset(name, config.as_ref()),
        (None, false) => ResolvedPreset {
            notes: cli.notes,
            ..Default::default()
        },
        (None, true) => fatal("provide notes or --preset"),
    };

    let wave = cli.wave.unwrap_or(base.wave);
    let volume = cli.volume.unwrap_or(base.volume);
    let gap = cli.gap.unwrap_or(base.gap);

    if !(0.0..=1.0).contains(&volume) {
        fatal(&format!("volume must be between 0.0 and 1.0, got {volume}"));
    }

    if cli.show_effective {
        println!("notes:  {}", base.notes.join(" "));
        println!("wave:   {wave}");
        println!("volume: {volume}");
        println!("gap:    {gap}ms");
        return;
    }

    if cli.dump_config {
        let preset_name = cli.preset.as_deref().unwrap_or("current");
        let preset = PresetConfig {
            notes: base.notes,
            wave: Some(wave),
            volume: Some(volume),
            gap: Some(gap),
        };
        let wrapper = HashMap::from([("presets", HashMap::from([(preset_name, preset)]))]);
        print!("{}", toml::to_string_pretty(&wrapper).expect("failed to serialize config"));
        return;
    }
    let notes: Vec<Note> = base.notes.iter().map(|s| parse_note(s)).collect();

    let mut handle = rodio::DeviceSinkBuilder::open_default_sink()
        .expect("failed to open audio output");
    handle.log_on_drop(false);
    let mixer = handle.mixer();

    let mut elapsed = 0u64;
    let mut max_end = 0u64;
    for (i, note) in notes.iter().enumerate() {
        let source = ToneSource::new(note.freq, note.duration_ms, wave, volume);
        mixer.add(source);

        let note_end = elapsed + note.duration_ms;
        if note_end > max_end {
            max_end = note_end;
        }

        if i < notes.len() - 1 {
            std::thread::sleep(Duration::from_millis(gap));
            elapsed += gap;
        }
    }

    // Wait for the longest-running note to finish
    let remaining = max_end.saturating_sub(elapsed);
    std::thread::sleep(Duration::from_millis(remaining));
}
