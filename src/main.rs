use std::num::NonZero;
use std::time::Duration;

use clap::{Parser, ValueEnum};
use rodio::Source;

// --- CLI ---

#[derive(Parser)]
#[command(about = "Play musical tones through the sound card")]
struct Cli {
    /// Notes: C5:150 Eb4:300 440:200 (name/freq, optional :duration_ms)
    notes: Vec<String>,

    /// Waveform shape
    #[arg(long, default_value = "sine")]
    wave: Waveform,

    /// Volume 0.0–1.0
    #[arg(long, default_value_t = 0.3)]
    volume: f32,

    /// Gap between note starts (ms)
    #[arg(long, default_value_t = 150)]
    gap: u64,

    /// Named preset instead of notes
    #[arg(long)]
    preset: Option<Preset>,
}

#[derive(Clone, Copy, ValueEnum)]
enum Waveform {
    Sine,
    Triangle,
    Square,
    Sawtooth,
}

#[derive(Clone, Copy, ValueEnum)]
enum Preset {
    Start,
    Success,
    Fail,
    Goal,
    Reminder,
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

fn get_preset(preset: Preset) -> (Vec<&'static str>, Waveform) {
    match preset {
        Preset::Start => (vec!["C5:500", "G5:500"], Waveform::Sine),
        Preset::Goal => (vec!["E5:500", "A5:500"], Waveform::Sine),
        Preset::Success => (vec!["C5:500", "E5:500", "G5:500"], Waveform::Sine),
        Preset::Fail => (vec!["G4:500", "Eb4:500", "C4:500"], Waveform::Triangle),
        Preset::Reminder => (vec!["A4:500"], Waveform::Sine),
    }
}

// --- Main ---

fn main() {
    let cli = Cli::parse();

    let (note_strs, waveform) = if let Some(preset) = cli.preset {
        let (notes, wave) = get_preset(preset);
        (notes.into_iter().map(String::from).collect(), wave)
    } else {
        if cli.notes.is_empty() {
            eprintln!("error: provide notes or --preset");
            std::process::exit(1);
        }
        (cli.notes, cli.wave)
    };

    let notes: Vec<Note> = note_strs.iter().map(|s| parse_note(s)).collect();

    let mut handle = rodio::DeviceSinkBuilder::open_default_sink()
        .expect("failed to open audio output");
    handle.log_on_drop(false);
    let mixer = handle.mixer();

    let mut last_duration_ms = 0u64;
    for (i, note) in notes.iter().enumerate() {
        let source = ToneSource::new(note.freq, note.duration_ms, waveform, cli.volume);
        mixer.add(source);

        last_duration_ms = note.duration_ms;
        if i < notes.len() - 1 {
            std::thread::sleep(Duration::from_millis(cli.gap));
        }
    }

    // Wait for the last note to finish
    std::thread::sleep(Duration::from_millis(last_duration_ms));
}
