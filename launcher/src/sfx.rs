//! Sound effects, synthesised at startup (no audio files are shipped).

use std::collections::HashMap;
use std::f32::consts::TAU;

use bevy::audio::{AudioSource, Volume};
use bevy::prelude::*;

use crate::profile::Profile;

#[derive(Event, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Sfx {
    CardPlay,
    CardDraw,
    Deal,
    Shuffle,
    Turn,
    LastCard,
    Caught,
    Special,
    Win,
    Dice,
    Coin,
    Step,
    Click,
}

#[derive(Resource)]
struct SfxBank(HashMap<Sfx, Handle<AudioSource>>);

const RATE: u32 = 22_050;

fn wav(samples: &[f32]) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&RATE.to_le_bytes());
    out.extend_from_slice(&(RATE * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        let v = (s.clamp(-1.0, 1.0) * 32_000.0) as i16;
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

/// Tiny deterministic noise source.
struct Noise(u32);

impl Noise {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        (self.0 as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
}

fn buffer(seconds: f32) -> Vec<f32> {
    vec![0.0; (seconds * RATE as f32) as usize]
}

/// A short papery snap: filtered noise plus a soft low thump.
fn add_snap(buf: &mut [f32], at: f32, gain: f32, decay: f32, seed: u32) {
    let mut noise = Noise(seed.max(1));
    let start = (at * RATE as f32) as usize;
    let mut lp = 0.0;
    for i in 0..(0.12 * RATE as f32) as usize {
        let Some(s) = buf.get_mut(start + i) else { break };
        let t = i as f32 / RATE as f32;
        let n = noise.next();
        lp += (n - lp) * 0.35;
        let env = (-t / decay).exp();
        let thump = (TAU * 110.0 * t).sin() * (-t / 0.03).exp() * 0.5;
        *s += (lp * 1.4 + thump) * env * gain;
    }
}

fn add_tone(buf: &mut [f32], at: f32, freq: f32, len: f32, gain: f32) {
    let start = (at * RATE as f32) as usize;
    for i in 0..(len * RATE as f32) as usize {
        let Some(s) = buf.get_mut(start + i) else { break };
        let t = i as f32 / RATE as f32;
        let attack = (t / 0.008).min(1.0);
        let env = attack * (-t / (len * 0.45)).exp();
        let w = (TAU * freq * t).sin() + 0.35 * (TAU * freq * 2.0 * t).sin() + 0.12 * (TAU * freq * 3.0 * t).sin();
        *s += w * env * gain;
    }
}

fn normalize(mut buf: Vec<f32>, peak: f32) -> Vec<f32> {
    let max = buf.iter().fold(0.0f32, |m, s| m.max(s.abs())).max(1e-6);
    for s in &mut buf {
        *s *= peak / max;
    }
    buf
}

fn synth(kind: Sfx) -> Vec<f32> {
    match kind {
        Sfx::CardPlay => {
            let mut b = buffer(0.16);
            add_snap(&mut b, 0.0, 1.0, 0.025, 11);
            normalize(b, 0.7)
        }
        Sfx::CardDraw => {
            let mut b = buffer(0.22);
            let mut noise = Noise(77);
            let mut lp = 0.0;
            for (i, s) in b.iter_mut().enumerate() {
                let t = i as f32 / RATE as f32;
                lp += (noise.next() - lp) * 0.12;
                let env = (t / 0.03).min(1.0) * (-t / 0.07).exp();
                *s = lp * env;
            }
            normalize(b, 0.45)
        }
        Sfx::Deal => {
            let mut b = buffer(0.9);
            for k in 0..8 {
                add_snap(&mut b, k as f32 * 0.1, 0.6 + (k % 3) as f32 * 0.1, 0.02, 31 + k);
            }
            normalize(b, 0.55)
        }
        Sfx::Shuffle => {
            let mut b = buffer(0.7);
            for k in 0..28 {
                let t = k as f32 * 0.022 + (k % 4) as f32 * 0.003;
                add_snap(&mut b, t, 0.3 + (k % 5) as f32 * 0.05, 0.012, 101 + k);
            }
            normalize(b, 0.5)
        }
        Sfx::Turn => {
            let mut b = buffer(0.7);
            add_tone(&mut b, 0.0, 659.25, 0.5, 0.5);
            add_tone(&mut b, 0.12, 880.0, 0.55, 0.5);
            normalize(b, 0.38)
        }
        Sfx::LastCard => {
            let mut b = buffer(0.8);
            for (i, f) in [523.25, 659.25, 783.99, 1046.5].iter().enumerate() {
                add_tone(&mut b, i as f32 * 0.08, *f, 0.45, 0.5);
            }
            normalize(b, 0.45)
        }
        Sfx::Caught => {
            let mut b = buffer(0.45);
            for (i, s) in b.iter_mut().enumerate() {
                let t = i as f32 / RATE as f32;
                let f = 320.0 - t * 300.0;
                let sq = if (TAU * f * t).sin() > 0.0 { 1.0 } else { -1.0 };
                *s = sq * (-t / 0.2).exp() * 0.4;
            }
            normalize(b, 0.35)
        }
        Sfx::Special => {
            let mut b = buffer(0.35);
            let mut noise = Noise(5);
            let mut lp = 0.0;
            for (i, s) in b.iter_mut().enumerate() {
                let t = i as f32 / RATE as f32;
                let k = 0.04 + t * 0.8;
                lp += (noise.next() - lp) * k.min(0.9);
                let env = (t / 0.08).min(1.0) * (1.0 - t / 0.35).max(0.0);
                *s = lp * env;
            }
            normalize(b, 0.4)
        }
        Sfx::Win => {
            let mut b = buffer(1.4);
            for (i, f) in [523.25, 659.25, 783.99, 1046.5, 783.99, 1046.5].iter().enumerate() {
                let len = if i == 5 { 0.8 } else { 0.25 };
                add_tone(&mut b, i as f32 * 0.12, *f, len, 0.5);
            }
            normalize(b, 0.45)
        }
        Sfx::Dice => {
            let mut b = buffer(0.75);
            let times = [0.0, 0.07, 0.15, 0.21, 0.3, 0.36, 0.45, 0.52, 0.6];
            for (k, t) in times.iter().enumerate() {
                add_snap(&mut b, *t, 1.0 - k as f32 * 0.08, 0.008, 200 + k as u32);
            }
            normalize(b, 0.55)
        }
        Sfx::Coin => {
            let mut b = buffer(0.5);
            add_tone(&mut b, 0.0, 987.77, 0.18, 0.5);
            add_tone(&mut b, 0.07, 1318.5, 0.4, 0.5);
            normalize(b, 0.32)
        }
        Sfx::Step => {
            let mut b = buffer(0.08);
            add_snap(&mut b, 0.0, 0.5, 0.01, 9);
            normalize(b, 0.25)
        }
        Sfx::Click => {
            let mut b = buffer(0.06);
            add_tone(&mut b, 0.0, 1400.0, 0.04, 0.3);
            normalize(b, 0.2)
        }
    }
}

fn build_bank(mut commands: Commands, mut sources: ResMut<Assets<AudioSource>>) {
    let kinds = [
        Sfx::CardPlay,
        Sfx::CardDraw,
        Sfx::Deal,
        Sfx::Shuffle,
        Sfx::Turn,
        Sfx::LastCard,
        Sfx::Caught,
        Sfx::Special,
        Sfx::Win,
        Sfx::Dice,
        Sfx::Coin,
        Sfx::Step,
        Sfx::Click,
    ];
    let map = kinds
        .into_iter()
        .map(|k| {
            let source = AudioSource {
                bytes: wav(&synth(k)).into(),
            };
            (k, sources.add(source))
        })
        .collect();
    commands.insert_resource(SfxBank(map));
}

fn play(
    mut commands: Commands,
    mut events: EventReader<Sfx>,
    bank: Option<Res<SfxBank>>,
    profile: Res<Profile>,
) {
    let Some(bank) = bank else { return };
    let mut played = Vec::new();
    for e in events.read() {
        // Never stack the same sound twice in one frame.
        if !profile.data.sound || played.contains(e) {
            continue;
        }
        played.push(*e);
        if let Some(h) = bank.0.get(e) {
            commands.spawn((
                AudioPlayer(h.clone()),
                PlaybackSettings::DESPAWN.with_volume(Volume::Linear(profile.data.volume)),
            ));
        }
    }
}

pub struct SfxPlugin;

impl Plugin for SfxPlugin {
    fn build(&self, app: &mut App) {
        app.add_event::<Sfx>()
            .add_systems(Startup, build_bank)
            .add_systems(PostUpdate, play);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_sound_is_valid_audio() {
        for k in [Sfx::CardPlay, Sfx::Win, Sfx::Dice, Sfx::Turn, Sfx::Shuffle] {
            let s = synth(k);
            assert!(!s.is_empty());
            assert!(s.iter().all(|x| x.is_finite() && x.abs() <= 1.0));
            let w = wav(&s);
            assert_eq!(&w[0..4], b"RIFF");
            assert_eq!(w.len(), 44 + s.len() * 2);
        }
    }
}
