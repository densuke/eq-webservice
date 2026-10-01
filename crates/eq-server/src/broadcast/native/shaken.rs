//! 寄りの「揺れた範囲」: 揺れた地震情報細分区域の外接矩形の和 (web の map.areaBox)。
//! 観測の震度は、区域で届くもの (震度速報) と観測点で届くもの (各地の震度) があり、観測点は
//! 観測点の表 (stations.json) で区域を引く。観測点の位置も範囲に入れる。
//! 何も引けないときだけ県の本土の範囲に戻す (離島だけが揺れたとき、県の本土を映さないため)。

use std::collections::HashMap;

use anyhow::Context;

use super::camera::MapBox;
use super::geo::{project, Shape};

/// 地震情報の観測点 1 つ
#[derive(Debug, Clone, PartialEq)]
pub struct Point {
    /// 区域名 (is_area) か観測点名
    pub addr: String,
    pub is_area: bool,
    /// 記録に付いている観測点の位置 (緯度・経度・区域)。ライブには付かない
    pub station: Option<(f64, f64, String)>,
}

/// 観測点の表: 名前 -> (緯度, 経度, 区域)
pub type Stations = HashMap<String, (f64, f64, String)>;

/// web/public/stations.json (行は [名前, 緯度, 経度, 区域])
pub fn load_stations(file: &std::path::Path) -> anyhow::Result<Stations> {
    let text = std::fs::read_to_string(file).with_context(|| format!("reading {}", file.display()))?;
    let rows: Vec<(String, f64, f64, String)> =
        serde_json::from_str(&text).with_context(|| file.display().to_string())?;
    Ok(rows.into_iter().map(|(n, lat, lon, a)| (n, (lat, lon, a))).collect())
}

/// 名前から外接矩形を引く表
#[derive(Default)]
pub struct Zones {
    areas: HashMap<String, MapBox>,
    prefs: HashMap<String, MapBox>,
    stations: Stations,
}

impl Zones {
    /// areas は地震情報細分区域、prefs は都道府県 (本土の矩形を使う)
    pub fn new(areas: Vec<Shape>, prefs: &[Shape], stations: Stations) -> Zones {
        Zones {
            areas: areas.into_iter().map(|s| (s.key, s.bounds)).collect(),
            prefs: prefs.iter().map(|s| (s.key.clone(), s.main)).collect(),
            stations,
        }
    }

    /// 揺れた範囲。areas は区域名、points は観測点、prefs は県名
    pub fn shaken(&self, areas: &[&str], points: &[Point], prefs: &[&str]) -> Option<MapBox> {
        let mut boxes: Vec<MapBox> = areas.iter().filter_map(|n| self.areas.get(*n).copied()).collect();
        for p in points {
            if p.is_area {
                boxes.extend(self.areas.get(&p.addr));
                continue;
            }
            let Some((lat, lon, area)) = p.station.as_ref().or_else(|| self.stations.get(&p.addr)) else {
                continue;
            };
            boxes.extend(self.areas.get(area));
            let (x, y) = project(*lon, *lat);
            boxes.push(MapBox::around(x, y, 0.0));
        }
        let union = |b: Vec<MapBox>| b.into_iter().fold(None, |a, b| MapBox::union(a, Some(b)));
        union(boxes).or_else(|| union(prefs.iter().filter_map(|p| self.prefs.get(*p).copied()).collect()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(x0: f64, y0: f64, x1: f64, y1: f64) -> MapBox {
        MapBox { x0, y0, x1, y1 }
    }

    fn zones() -> Zones {
        Zones {
            areas: HashMap::from([
                ("島".to_string(), b(0.0, 0.0, 10.0, 10.0)),
                ("本土".to_string(), b(100.0, 0.0, 200.0, 50.0)),
            ]),
            prefs: HashMap::from([("県".to_string(), b(90.0, -10.0, 300.0, 90.0))]),
            stations: HashMap::from([("観測点A".to_string(), (35.0, 137.0, "島".to_string()))]),
        }
    }

    fn point(addr: &str, is_area: bool) -> Point {
        Point {
            addr: addr.into(),
            is_area,
            station: None,
        }
    }

    #[test]
    fn areas_and_stations_are_used_before_the_prefecture() {
        let z = zones();
        // 区域で届いた点
        assert_eq!(
            z.shaken(&[], &[point("島", true)], &["県"]),
            Some(b(0.0, 0.0, 10.0, 10.0))
        );
        // 観測点: 区域の矩形と観測点の位置 (137E 35N は地図の原点の近く)
        let s = z.shaken(&[], &[point("観測点A", false)], &["県"]).unwrap();
        assert!(s.x0 <= 0.0 && s.x1 >= 10.0 && s.y1 >= 10.0);
        // 区域名で直接 (緊急地震速報)
        assert_eq!(z.shaken(&["本土"], &[], &["県"]), Some(b(100.0, 0.0, 200.0, 50.0)));
        // 何も引けないときだけ県
        assert_eq!(
            z.shaken(&[], &[point("不明", false)], &["県"]),
            Some(b(90.0, -10.0, 300.0, 90.0))
        );
        assert_eq!(z.shaken(&[], &[], &[]), None);
    }
}
