//! 配信の音を eq-server の中で作る (mixer)。

#[allow(dead_code)] // W4 で mixer から使う
mod synth;

/// 音の形式: 44.1kHz・ステレオ・i16 (L R の交互)
pub const RATE: u32 = 44_100;
