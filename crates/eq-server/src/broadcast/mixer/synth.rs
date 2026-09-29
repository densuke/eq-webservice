//! 警戒音の合成。web/src/sound.ts の play() と同じ音を、44.1kHz・ステレオ・i16 (L R の交互) で作る。
//! 音は「波形 × 音の大きさの変化」。0.01 秒で立ち上がり、長さの終わりまで指数的に下がる。

use std::f64::consts::TAU;

use super::RATE;

/// 鳴らす音の種類 (web の AlertLevel と、波の刻み・揺れの報告)
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AlertLevel {
    Strong,
    Medium,
    Low,
    Info,
    Pip,
    Feel,
}

#[derive(Clone, Copy)]
enum Wave {
    Sine,
    Square,
    Triangle,
}

/// 音 1 つ: 周波数、始まり (秒)、長さ (秒)、波形、ピークの大きさ
struct Tone {
    freq: f64,
    at: f64,
    dur: f64,
    wave: Wave,
    peak: f64,
}

const fn tone(freq: f64, at: f64, dur: f64, wave: Wave, peak: f64) -> Tone {
    Tone {
        freq,
        at,
        dur,
        wave,
        peak,
    }
}

/// 立ち上がりの長さ (秒) と、下がりきったときの大きさ (sound.ts と同じ)
const ATTACK: f64 = 0.01;
const FLOOR: f64 = 0.0001;

fn tones(level: AlertLevel) -> Vec<Tone> {
    use Wave::*;
    match level {
        AlertLevel::Low => vec![tone(880.0, 0.0, 0.5, Sine, 0.25), tone(660.0, 0.25, 0.7, Sine, 0.25)],
        AlertLevel::Medium => [0.0, 0.9]
            .into_iter()
            .flat_map(|at| [tone(988.0, at, 0.5, Sine, 0.3), tone(784.0, at + 0.3, 0.8, Sine, 0.3)])
            .collect(),
        AlertLevel::Info => [660.0, 880.0, 1100.0]
            .into_iter()
            .enumerate()
            .map(|(i, f)| tone(f, i as f64 * 0.18, 0.6, Sine, 0.22))
            .collect(),
        AlertLevel::Pip => vec![tone(1200.0, 0.0, 0.12, Sine, 0.15)],
        AlertLevel::Feel => vec![
            tone(523.0, 0.0, 0.35, Triangle, 0.14),
            tone(523.0, 0.3, 0.35, Triangle, 0.14),
        ],
        AlertLevel::Strong => (0..12)
            .map(|i| {
                tone(
                    if i % 2 == 1 { 770.0 } else { 960.0 },
                    i as f64 * 0.2,
                    0.18,
                    Square,
                    0.12,
                )
            })
            .collect(),
    }
}

fn wave(w: Wave, phase: f64) -> f64 {
    let p = phase.fract();
    match w {
        Wave::Sine => (p * TAU).sin(),
        Wave::Square => {
            if p < 0.5 {
                1.0
            } else {
                -1.0
            }
        }
        Wave::Triangle => 4.0 * (p - 0.5).abs() - 1.0,
    }
}

/// t 秒 (音の始まりから) の大きさ。FLOOR から peak まで直線で上がり、そのあと FLOOR まで指数的に下がる
fn envelope(t: &Tone, s: f64) -> f64 {
    if s < ATTACK {
        FLOOR + (t.peak - FLOOR) * s / ATTACK
    } else {
        t.peak * (FLOOR / t.peak).powf((s - ATTACK) / (t.dur - ATTACK))
    }
}

/// 警戒音 1 回分。ステレオ (L R の交互) で、サンプル数 = 長さ × 44100 × 2
pub fn alert(level: AlertLevel) -> Vec<i16> {
    let tones = tones(level);
    let end = tones.iter().map(|t| t.at + t.dur).fold(0.0, f64::max);
    let frames = (end * RATE as f64).round() as usize;
    let mut mono = vec![0.0f64; frames];
    for t in &tones {
        let first = (t.at * RATE as f64).round() as usize;
        let last = (((t.at + t.dur) * RATE as f64).round() as usize).min(frames);
        for (i, out) in mono.iter_mut().enumerate().take(last).skip(first) {
            let s = (i - first) as f64 / RATE as f64;
            *out += wave(t.wave, t.freq * s) * envelope(t, s);
        }
    }
    mono.iter()
        .flat_map(|&v| {
            let x = (v * i16::MAX as f64).round().clamp(i16::MIN as f64, i16::MAX as f64) as i16;
            [x, x]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [AlertLevel; 6] = [
        AlertLevel::Strong,
        AlertLevel::Medium,
        AlertLevel::Low,
        AlertLevel::Info,
        AlertLevel::Pip,
        AlertLevel::Feel,
    ];

    /// sound.ts の各 level が終わる時刻 (秒): 最後の音の始まり + 長さ
    fn seconds(level: AlertLevel) -> f64 {
        match level {
            AlertLevel::Low => 0.95,
            AlertLevel::Medium => 2.0,
            AlertLevel::Info => 0.96,
            AlertLevel::Pip => 0.12,
            AlertLevel::Feel => 0.65,
            AlertLevel::Strong => 2.38,
        }
    }

    fn peak(pcm: &[i16], from_s: f64, to_s: f64) -> i32 {
        let (a, b) = ((from_s * RATE as f64) as usize * 2, (to_s * RATE as f64) as usize * 2);
        pcm[a..b.min(pcm.len())]
            .iter()
            .map(|&x| (x as i32).abs())
            .max()
            .unwrap()
    }

    #[test]
    fn each_level_has_the_length_of_sound_ts() {
        for level in ALL {
            let want = (seconds(level) * RATE as f64).round() as usize * 2;
            assert_eq!(alert(level).len(), want, "{level:?}");
        }
    }

    #[test]
    fn stereo_channels_are_identical() {
        for level in ALL {
            assert!(alert(level).chunks(2).all(|c| c[0] == c[1]), "{level:?}");
        }
    }

    #[test]
    fn the_sound_is_audible_and_never_leaves_the_i16_range() {
        for level in ALL {
            let p = peak(&alert(level), 0.0, 10.0);
            assert!(p > 1000 && p <= i16::MAX as i32, "{level:?} peak {p}");
        }
    }

    #[test]
    fn the_first_tone_rises_in_ten_milliseconds_then_decays() {
        // pip: 1200Hz、ピーク 0.15、0.12 秒。立ち上がりの前半は小さく、0.01 秒あたりでピーク近くになる
        let pcm = alert(AlertLevel::Pip);
        let early = peak(&pcm, 0.0, 0.002);
        let at_peak = peak(&pcm, 0.008, 0.012);
        let late = peak(&pcm, 0.10, 0.12);
        let top = 0.15 * i16::MAX as f64;
        assert!((early as f64) < top * 0.25, "early {early}");
        assert!((at_peak as f64) > top * 0.9, "at_peak {at_peak}");
        assert!((late as f64) < top * 0.05, "late {late}");
    }

    #[test]
    fn the_peak_matches_the_peak_of_sound_ts() {
        // feel は triangle 0.14。2 音目 (0.3 秒〜) の頭でも 0.14 を超えない
        let p = peak(&alert(AlertLevel::Feel), 0.0, 1.0) as f64 / i16::MAX as f64;
        assert!((0.13..=0.29).contains(&p), "{p}"); // 0.3〜0.35 秒は 2 音が重なる
                                                    // low: sine 0.25。重なる区間 (0.25〜0.5) は足し算になる
        let p = peak(&alert(AlertLevel::Low), 0.0, 0.24) as f64 / i16::MAX as f64;
        assert!((0.2..=0.25).contains(&p), "{p}");
    }

    #[test]
    fn level_names_come_from_the_page() {
        let l: AlertLevel = serde_json::from_str("\"strong\"").unwrap();
        assert_eq!(l, AlertLevel::Strong);
        assert!(serde_json::from_str::<AlertLevel>("\"loud\"").is_err());
    }
}
