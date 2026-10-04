//! 音声アナウンス (Google Cloud Text-to-Speech)。仕様は docs/tts.md。
//! 文を部品に分け、部品ごとにディスクへキャッシュする (無料枠に収めるため)。

pub mod budget;
pub mod cache;
pub mod google;
pub mod phrase;
pub mod wav;
