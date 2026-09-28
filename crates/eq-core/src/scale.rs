//! 気象庁震度階級。P2P地震情報の数値表現 (10=震度1 ... 70=震度7) を基準にしている。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Scale(pub i32);

impl Scale {
    pub const UNKNOWN: Scale = Scale(-1);
    pub const S1: Scale = Scale(10);
    pub const S2: Scale = Scale(20);
    pub const S3: Scale = Scale(30);
    pub const S4: Scale = Scale(40);
    pub const S5_LOWER: Scale = Scale(45);
    /// 震度5弱以上と推定されるが震度情報を入手していない
    pub const S5_LOWER_ESTIMATED: Scale = Scale(46);
    pub const S5_UPPER: Scale = Scale(50);
    pub const S6_LOWER: Scale = Scale(55);
    pub const S6_UPPER: Scale = Scale(60);
    pub const S7: Scale = Scale(70);

    /// "3", "5弱", "5-", "6強", "6+" などを読む。
    pub fn parse(s: &str) -> Option<Scale> {
        Some(match s.trim() {
            "1" => Scale::S1,
            "2" => Scale::S2,
            "3" => Scale::S3,
            "4" => Scale::S4,
            "5-" | "5弱" => Scale::S5_LOWER,
            "5+" | "5強" => Scale::S5_UPPER,
            "6-" | "6弱" => Scale::S6_LOWER,
            "6+" | "6強" => Scale::S6_UPPER,
            "7" => Scale::S7,
            _ => return None,
        })
    }

    pub fn is_known(self) -> bool {
        self.0 > 0
    }

    /// 表示用ラベル ("5弱" など)。
    pub fn label(self) -> &'static str {
        match self.0 {
            10 => "1",
            20 => "2",
            30 => "3",
            40 => "4",
            45 => "5弱",
            46 => "5弱以上(推定)",
            50 => "5強",
            55 => "6弱",
            60 => "6強",
            70 => "7",
            _ => "不明",
        }
    }
}

impl Default for Scale {
    fn default() -> Self {
        Scale::UNKNOWN
    }
}

impl std::fmt::Display for Scale {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "震度{}", self.label())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordering_matches_intensity() {
        assert!(Scale::S5_LOWER < Scale::S5_UPPER);
        assert!(Scale::S6_UPPER < Scale::S7);
        assert!(Scale::UNKNOWN < Scale::S1);
        assert_eq!(Scale(55).label(), "6弱");
    }
}
