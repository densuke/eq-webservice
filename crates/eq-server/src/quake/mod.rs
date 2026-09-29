//! 地震情報のデータモデルと、上流 (P2P地震情報 API・Wolfx) の JSON からの変換。

pub mod area;
pub mod jst;
pub mod model;
pub mod p2pquake;
pub mod scale;
pub mod wolfx;

pub use model::*;
pub use scale::Scale;
