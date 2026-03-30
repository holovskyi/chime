use std::collections::HashMap;
use std::num::NonZero;
use std::path::{Path, PathBuf};
use std::time::Duration;

use clap::{Parser, ValueEnum};
use rodio::Source;
use serde::Deserialize;

// --- CLI ---

#[derive(Parser)]
#[command(about = "Play musical tones through the sound card")]
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
}

#[derive(Clone, Copy, ValueEnum, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Waveform {
    Sine,
    Triangle,
    Square,
    Sawtooth,
}

// --- Config ---

#[derive(Deserialize, Default)]
struct Config {
    #[serde(default)]
    presets: HashMap<String, PresetConfig>,
}

#[derive(Deserialize)]
struct PresetConfig {
    notes: Vec<String>,
    wave: Option<Waveform>,
    volume: Option<f32>,
    gap: Option<u64>,
}

fn find_config(explicit: Option<&Path>) -> Option<PathBuf> {
    if let Some(path) = explicit {
        return if path.exists() { Some(path.to_owned()) } else { None };
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
        .unwrap_or_else(|e| panic!("failed to read config {}: {e}", path.display()));
    toml::from_str(&content)
        .unwrap_or_else(|e| panic!("failed to parse config {}: {e}", path.display()))
}

// --- Note parsing ---

struct Note {
    freq: f32,
    duration_ms: u64,
}

fn note_name_to_semitone(name: &str) -> Option<(i32, usize)> {
    let (semitone, len) = if name.len() >= 2 {
        match &name[..2] {
            "C#" | "Db" => (1, 2),
            "D#" | "Eb" => (3, 2),
            "F#" | "Gb" => (6, 2),
            "G#" | "Ab" => (8, 2),
            "A#" | "Bb" => (10, 2),
            _ => match &name[..1] {
                "C" => (0, 1),
                "D" => (2, 1),
                "E" => (4, 1),
                "F" => (5, 1),
                "G" => (7, 1),
                "A" => (9, 1),
                "B" => (11, 1),
                _ => return None,
            },
        }
    } else {
        match &name[..1] {
            "C" => (0, 1),
            "D" => (2, 1),
            "E" => (4, 1),
            "F" => (5, 1),
            "G" => (7, 1),
            "A" => (9, 1),
            "B" => (11, 1),
            _ => return None,
        }
    };
    Some((semitone, len))
}

fn parse_note(s: &str) -> Note {
    let (tone, duration_ms) = match s.split_once(':') {
        Some((t, d)) => (t, d.parse::<u64>().expect("invalid duration")),
        None => (s, 500),
    };

    // Try parsing as raw frequency
    if let Ok(freq) = tone.parse::<f32>() {
        return Note { freq, duration_ms };
    }

    // Parse note name + octave
    let (semitone, name_len) =
        note_name_to_semitone(tone).unwrap_or_else(|| panic!("unknown note: {tone}"));
    let octave: i32 = tone[name_len..]
        .parse()
        .unwrap_or_else(|_| panic!("invalid octave in: {tone}"));

    // A4 = 440Hz, semitone 9 in octave 4
    let semitones_from_a4 = (octave - 4) * 12 + semitone - 9;
    let freq = 440.0 * 2.0_f32.powf(semitones_from_a4 as f32 / 12.0);

    Note { freq, duration_ms }
}

// --- Tone source ---

const SAMPLE_RATE: u32 = 48000;

struct ToneSource {
    sample_i: u32,
    frequency: f32,
    duration_samples: u32,
    waveform: Waveform,
    volume: f32,
}

impl ToneSource {
    fn new(freq: f32, duration_ms: u64, waveform: Waveform, volume: f32) -> Self {
        Self {
            sample_i: 0,
            frequency: freq,
            duration_samples: (SAMPLE_RATE as u64 * duration_ms / 1000) as u32,
            waveform,
            volume,
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
        let phase = 2.0 * std::f32::consts::PI * self.frequency * t;

        let wave = match self.waveform {
            Waveform::Sine => phase.sin(),
            Waveform::Triangle => 2.0 / std::f32::consts::PI * phase.sin().asin(),
            Waveform::Square => phase.sin().signum(),
            Waveform::Sawtooth => 2.0 * (self.frequency * t - (self.frequency * t + 0.5).floor()),
        };

        // Exponential decay envelope
        let duration_secs = self.duration_samples as f32 / SAMPLE_RATE as f32;
        let envelope = (-5.0 * t / duration_secs).exp();

        self.sample_i += 1;
        Some(wave * self.volume * envelope)
    }
}

impl Source for ToneSource {
    fn current_span_len(&self) -> Option<usize> {
        Some((self.duration_samples - self.sample_i) as usize)
    }

    fn channels(&self) -> NonZero<u16> {
        NonZero::new(1).unwrap()
    }

    fn sample_rate(&self) -> NonZero<u32> {
        NonZero::new(SAMPLE_RATE).unwrap()
    }

    fn total_duration(&self) -> Option<Duration> {
        Some(Duration::from_millis(
            self.duration_samples as u64 * 1000 / SAMPLE_RATE as u64,
        ))
    }
}

// --- Presets ---

struct ResolvedPreset {
    notes: Vec<String>,
    wave: Waveform,
    volume: f32,
    gap: u64,
}

fn builtin_preset(name: &str) -> Option<ResolvedPreset> {
    let (notes, wave) = match name {
        "start" => (vec!["C5:500", "G5:500"], Waveform::Sine),
        "goal" => (vec!["E5:500", "A5:500"], Waveform::Sine),
        "success" => (vec!["C5:500", "E5:500", "G5:500"], Waveform::Sine),
        "fail" => (vec!["G4:500", "Eb4:500", "C4:500"], Waveform::Triangle),
        "reminder" => (vec!["A4:500"], Waveform::Sine),
        _ => return None,
    };
    Some(ResolvedPreset {
        notes: notes.into_iter().map(String::from).collect(),
        wave,
        volume: 0.3,
        gap: 150,
    })
}

fn resolve_preset(name: &str, config: Option<&Config>) -> ResolvedPreset {
    // Config presets take priority over built-in
    if let Some(cfg) = config {
        if let Some(preset) = cfg.presets.get(name) {
            return ResolvedPreset {
                notes: preset.notes.clone(),
                wave: preset.wave.unwrap_or(Waveform::Sine),
                volume: preset.volume.unwrap_or(0.3),
                gap: preset.gap.unwrap_or(150),
            };
        }
    }

    builtin_preset(name).unwrap_or_else(|| {
        eprintln!("error: unknown preset '{name}'");
        std::process::exit(1);
    })
}

// --- Main ---

fn main() {
    let cli = Cli::parse();

    let config = find_config(cli.config.as_deref()).map(|p| load_config(&p));

    let (note_strs, wave, volume, gap) = if let Some(ref preset_name) = cli.preset {
        let preset = resolve_preset(preset_name, config.as_ref());
        (
            preset.notes,
            cli.wave.unwrap_or(preset.wave),
            cli.volume.unwrap_or(preset.volume),
            cli.gap.unwrap_or(preset.gap),
        )
    } else {
        if cli.notes.is_empty() {
            eprintln!("error: provide notes or --preset");
            std::process::exit(1);
        }
        (
            cli.notes,
            cli.wave.unwrap_or(Waveform::Sine),
            cli.volume.unwrap_or(0.3),
            cli.gap.unwrap_or(150),
        )
    };

    let notes: Vec<Note> = note_strs.iter().map(|s| parse_note(s)).collect();

    let mut handle = rodio::DeviceSinkBuilder::open_default_sink()
        .expect("failed to open audio output");
    handle.log_on_drop(false);
    let mixer = handle.mixer();

    let mut last_duration_ms = 0u64;
    for (i, note) in notes.iter().enumerate() {
        let source = ToneSource::new(note.freq, note.duration_ms, wave, volume);
        mixer.add(source);

        last_duration_ms = note.duration_ms;
        if i < notes.len() - 1 {
            std::thread::sleep(Duration::from_millis(gap));
        }
    }

    // Wait for the last note to finish
    std::thread::sleep(Duration::from_millis(last_duration_ms));
}
