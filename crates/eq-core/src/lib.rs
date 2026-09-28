//! 地震情報のデータモデルと、上流 (P2P地震情報 API) の JSON からの変換。
//!
//! このクレートは tokio などの実行環境に依存しない。将来フロントエンド側で
//! WASM として同じモデル/変換ロジックを使えるようにするため。

pub mod jst;
pub mod model;
pub mod p2pquake;
pub mod scale;

pub use model::*;
pub use scale::Scale;
