//! のちに届く観測点を、最初の円に合わせて灯す (記録からの描き直し。replay_info::Foresight) ときの判断 (純粋な関数)。
//! ライブでは使わない: 地震から数分後に円を描き直すと、その時刻にまた揺れたように見える (2026-11-03 千葉県北東部)。
//! ライブの観測点は、届いたらすぐ全部出す (web と同じ)

use super::eew::{surface_radius_km, VS_KM_S};
use super::model::distance_km;

/// 広がる円。start_ms は発生時刻
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spread {
    pub lat: f64,
    pub lon: f64,
    pub depth_km: f64,
    pub start_ms: u64,
    pub end_ms: u64,
}

fn elapsed_sec(s: &Spread, now_ms: u64) -> f64 {
    (now_ms as f64 - s.start_ms as f64) / 1000.0
}

/// S 波の地表での半径 (km)。開始前・まだ地表に届かないときは None (点を全部隠す)
fn reach_km(s: &Spread, now_ms: u64) -> Option<f64> {
    surface_radius_km(VS_KM_S, s.depth_km, elapsed_sec(s, now_ms))
}

/// その点 (経度, 緯度) に S 波が届いたか
pub fn lit(s: &Spread, now_ms: u64, lon: f64, lat: f64) -> bool {
    reach_km(s, now_ms).is_some_and(|r| distance_km(s.lat, s.lon, lat, lon) <= r)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dots_light_only_inside_the_s_wave() {
        let (lat, lon) = (35.0, 139.0);
        let s = Spread {
            lat,
            lon,
            depth_km: 10.0,
            start_ms: 1_000_000,
            end_ms: 1_180_000,
        };
        let (near, far) = (lat + 30.0 / 111.2, lat + 120.0 / 111.2);
        assert!(!lit(&s, s.start_ms - 1, lon, near));
        let t = s.start_ms + 9_000;
        assert!(lit(&s, t, lon, near));
        assert!(!lit(&s, t, lon, far));
    }
}
