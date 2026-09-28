//! 上流の形式に依存しない、正規化済みの地震関連イベント。
//!
//! ブラウザへの配信・プラグイン (RSS / Discord など) はすべてこの型を扱う。
//! 上流を追加する場合は、その上流の形式からこの型への変換を書けばよい。

use serde::{Deserialize, Serialize};

use crate::jst;
use crate::scale::Scale;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    /// 上流が付与した ID (重複排除に使う)
    pub id: String,
    /// 上流の識別子 ("p2pquake" など)
    pub source: String,
    /// サーバが受信した時刻 (UNIX epoch ミリ秒)。0 は未設定。
    #[serde(default)]
    pub received_at_ms: u64,
    #[serde(flatten)]
    pub body: EventBody,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EventBody {
    /// 地震情報 (震度速報・震源情報・各地の震度など)
    Quake(Quake),
    /// 緊急地震速報 (警報・予報)
    Eew(Eew),
    /// 緊急地震速報の発表検出 (内容なしの「鳴った」通知)
    EewDetection(EewDetection),
    /// 津波予報
    Tsunami(Tsunami),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Hypocenter {
    pub name: String,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    /// 深さ (km)。0 は「ごく浅い」。
    pub depth_km: Option<i32>,
    pub magnitude: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuakeInfoType {
    /// 震度速報
    ScalePrompt,
    /// 震源に関する情報
    Destination,
    /// 震源・震度に関する情報
    ScaleAndDestination,
    /// 各地の震度に関する情報
    DetailScale,
    /// 遠地地震に関する情報
    Foreign,
    Other,
}

impl QuakeInfoType {
    pub fn label(self) -> &'static str {
        match self {
            QuakeInfoType::ScalePrompt => "震度速報",
            QuakeInfoType::Destination => "震源に関する情報",
            QuakeInfoType::ScaleAndDestination => "震源・震度に関する情報",
            QuakeInfoType::DetailScale => "各地の震度に関する情報",
            QuakeInfoType::Foreign => "遠地地震に関する情報",
            QuakeInfoType::Other => "地震情報",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ObservationPoint {
    pub pref: String,
    /// 観測点名、または (is_area の場合) 地域名
    pub addr: String,
    pub is_area: bool,
    pub scale: Scale,
}

/// 都道府県ごとの最大震度。地図の塗り分けに使う。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PrefScale {
    pub pref: String,
    pub scale: Scale,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Quake {
    pub info_type: QuakeInfoType,
    /// 発生時刻 (JST, "YYYY/MM/DD HH:MM:SS")
    pub origin_time: String,
    /// 発生時刻 (epoch ミリ秒)。P波・S波の描画に使う。
    pub origin_time_ms: Option<i64>,
    /// 発表時刻 (JST)
    pub issued_at: String,
    pub hypocenter: Option<Hypocenter>,
    pub max_scale: Scale,
    /// 国内への津波の有無 ("None" / "Unknown" / "Checking" / "NonEffective" / "Watch" / "Warning")
    pub domestic_tsunami: String,
    pub points: Vec<ObservationPoint>,
    /// points から集計した都道府県別最大震度 (震度の大きい順)
    pub pref_max: Vec<PrefScale>,
    pub comment: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EewArea {
    pub pref: String,
    pub name: String,
    pub scale_from: Scale,
    /// None は「〜程度以上」
    pub scale_to: Option<Scale>,
    /// 主要動の到達予想時刻 (JST)
    pub arrival_time: Option<String>,
    /// 既に主要動が到達していると推測される
    pub arrived: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Eew {
    pub event_id: String,
    pub serial: String,
    pub cancelled: bool,
    /// テスト配信
    pub test: bool,
    /// 警報 (予測震度5弱以上)。false は予報。P2P地震情報は警報だけを配信する
    #[serde(default = "yes")]
    pub warning: bool,
    pub issued_at: String,
    pub origin_time: Option<String>,
    /// 発生時刻 (epoch ミリ秒)。P波・S波の描画に使う。
    pub origin_time_ms: Option<i64>,
    pub hypocenter: Option<Hypocenter>,
    pub areas: Vec<EewArea>,
    pub pref_max: Vec<PrefScale>,
    pub max_scale: Scale,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EewDetection {
    /// "Full" (チャイム + 音声) / "Chime" (チャイムのみ)
    pub detection_type: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TsunamiGrade {
    Unknown,
    /// 津波注意報
    Watch,
    /// 津波警報
    Warning,
    /// 大津波警報
    MajorWarning,
}

impl TsunamiGrade {
    pub fn label(self) -> &'static str {
        match self {
            TsunamiGrade::MajorWarning => "大津波警報",
            TsunamiGrade::Warning => "津波警報",
            TsunamiGrade::Watch => "津波注意報",
            TsunamiGrade::Unknown => "不明",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TsunamiArea {
    pub name: String,
    pub grade: TsunamiGrade,
    /// ただちに津波来襲と予測
    pub immediate: bool,
    pub first_height: Option<String>,
    pub max_height: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tsunami {
    pub cancelled: bool,
    pub issued_at: String,
    pub areas: Vec<TsunamiArea>,
}

impl Event {
    /// 通知やフィードの見出し用の1行。
    pub fn title(&self) -> String {
        match &self.body {
            EventBody::Quake(q) => {
                let place = q
                    .hypocenter
                    .as_ref()
                    .map(|h| h.name.as_str())
                    .filter(|n| !n.is_empty())
                    .unwrap_or("震源調査中");
                if q.max_scale.is_known() {
                    format!("【{}】{} 最大{}", q.info_type.label(), place, q.max_scale)
                } else {
                    format!("【{}】{}", q.info_type.label(), place)
                }
            }
            EventBody::Eew(e) => {
                let prefix = match (e.test, e.warning) {
                    (true, _) => "【緊急地震速報(テスト)】",
                    (false, true) => "【緊急地震速報(警報)】",
                    (false, false) => "【緊急地震速報(予報)】",
                };
                if e.cancelled {
                    format!("{prefix}取消")
                } else {
                    let place = e.hypocenter.as_ref().map(|h| h.name.as_str()).unwrap_or("震源不明");
                    format!("{prefix}{place} 第{}報", e.serial)
                }
            }
            EventBody::EewDetection(_) => "緊急地震速報の発表を検出".to_string(),
            EventBody::Tsunami(t) => {
                if t.cancelled {
                    "【津波予報】解除".to_string()
                } else {
                    let top = t.areas.iter().map(|a| a.grade).max().unwrap_or(TsunamiGrade::Unknown);
                    format!("【{}】{}地域", top.label(), t.areas.len())
                }
            }
        }
    }

    /// 本文用の複数行テキスト。
    pub fn summary(&self) -> String {
        let mut lines = Vec::new();
        match &self.body {
            EventBody::Quake(q) => {
                lines.push(format!("発生時刻: {}", q.origin_time));
                if let Some(h) = &q.hypocenter {
                    lines.extend(hypocenter_lines(h));
                }
                if q.max_scale.is_known() {
                    lines.push(format!("最大震度: {}", q.max_scale.label()));
                }
                if let Some(t) = domestic_tsunami_label(&q.domestic_tsunami) {
                    lines.push(t.to_string());
                }
                for p in q.pref_max.iter().take(8) {
                    lines.push(format!("  {} {}", p.pref, p.scale));
                }
                if !q.comment.is_empty() {
                    lines.push(q.comment.clone());
                }
            }
            EventBody::Eew(e) => {
                if e.cancelled {
                    lines.push("先ほどの緊急地震速報は取り消されました。".to_string());
                } else {
                    if let Some(t) = &e.origin_time {
                        lines.push(format!("発生時刻: {t}"));
                    }
                    if let Some(h) = &e.hypocenter {
                        lines.extend(hypocenter_lines(h));
                    }
                    let areas: Vec<_> = e.pref_max.iter().map(|p| p.pref.as_str()).collect();
                    if !areas.is_empty() {
                        lines.push(format!("強い揺れに警戒: {}", areas.join("、")));
                    }
                }
            }
            EventBody::EewDetection(d) => {
                lines.push(format!("検出種別: {}", d.detection_type));
            }
            EventBody::Tsunami(t) => {
                lines.push(format!("発表時刻: {}", t.issued_at));
                for a in &t.areas {
                    let mut s = format!("  {} {}", a.grade.label(), a.name);
                    if a.immediate {
                        s.push_str(" (ただちに来襲)");
                    }
                    if let Some(h) = &a.max_height {
                        s.push_str(&format!(" 予想高さ {h}"));
                    }
                    lines.push(s);
                }
            }
        }
        lines.join("\n")
    }

    /// 過去データを「今」の出来事として再生するために時刻をずらす。
    /// 発表時刻は issued_delta_ms、発生時刻と到達予想時刻は origin_delta_ms だけずらす
    /// (再生で待ち時間を詰めても、同じ地震の情報どうしで発生時刻がそろうように分けてある)。
    pub fn shift_times(&mut self, issued_delta_ms: i64, origin_delta_ms: i64) {
        let shift_origin = |s: &str| jst::shift_str(s, origin_delta_ms);
        let shift_origin_ms = |v: &mut Option<i64>| *v = v.map(|t| t + origin_delta_ms);
        match &mut self.body {
            EventBody::Quake(q) => {
                q.origin_time = shift_origin(&q.origin_time);
                q.issued_at = jst::shift_str(&q.issued_at, issued_delta_ms);
                shift_origin_ms(&mut q.origin_time_ms);
            }
            EventBody::Eew(e) => {
                e.issued_at = jst::shift_str(&e.issued_at, issued_delta_ms);
                e.origin_time = e.origin_time.as_deref().map(shift_origin);
                shift_origin_ms(&mut e.origin_time_ms);
                for a in &mut e.areas {
                    a.arrival_time = a.arrival_time.as_deref().map(shift_origin);
                }
            }
            EventBody::Tsunami(t) => t.issued_at = jst::shift_str(&t.issued_at, issued_delta_ms),
            EventBody::EewDetection(_) => {}
        }
    }

    /// 発表時刻 (epoch ミリ秒)。
    pub fn issued_at_ms(&self) -> Option<i64> {
        match &self.body {
            EventBody::Quake(q) => jst::parse_ms(&q.issued_at),
            EventBody::Eew(e) => jst::parse_ms(&e.issued_at),
            EventBody::Tsunami(t) => jst::parse_ms(&t.issued_at),
            EventBody::EewDetection(_) => None,
        }
    }

    /// フィルタ用の代表震度 (地震情報・EEW 以外は None)。
    pub fn max_scale(&self) -> Option<Scale> {
        match &self.body {
            EventBody::Quake(q) => Some(q.max_scale),
            EventBody::Eew(e) => Some(e.max_scale),
            _ => None,
        }
    }

    pub fn kind(&self) -> &'static str {
        match &self.body {
            EventBody::Quake(_) => "quake",
            EventBody::Eew(_) => "eew",
            EventBody::EewDetection(_) => "eew_detection",
            EventBody::Tsunami(_) => "tsunami",
        }
    }
}

fn hypocenter_lines(h: &Hypocenter) -> Vec<String> {
    let mut v = vec![format!("震源地: {}", if h.name.is_empty() { "不明" } else { &h.name })];
    match h.depth_km {
        Some(0) => v.push("深さ: ごく浅い".to_string()),
        Some(d) => v.push(format!("深さ: 約{d}km")),
        None => {}
    }
    if let Some(m) = h.magnitude {
        v.push(format!("規模: M{m:.1}"));
    }
    v
}

pub fn domestic_tsunami_label(v: &str) -> Option<&'static str> {
    match v {
        "None" => Some("この地震による津波の心配はありません。"),
        "NonEffective" => Some("若干の海面変動が予想されますが、被害の心配はありません。"),
        "Checking" => Some("津波の有無については現在調査中です。"),
        "Watch" => Some("津波注意報を発表中です。"),
        "Warning" => Some("津波予報(警報等)を発表中です。"),
        _ => None,
    }
}

fn yes() -> bool {
    true
}

/// 都道府県ごとの最大震度を、震度の大きい順 (同震度は出現順) で返す。
pub fn aggregate_pref_max<'a>(items: impl IntoIterator<Item = (&'a str, Scale)>) -> Vec<PrefScale> {
    let mut out: Vec<PrefScale> = Vec::new();
    for (pref, scale) in items {
        if pref.is_empty() {
            continue;
        }
        match out.iter_mut().find(|p| p.pref == pref) {
            Some(p) if scale > p.scale => p.scale = scale,
            Some(_) => {}
            None => out.push(PrefScale {
                pref: pref.to_string(),
                scale,
            }),
        }
    }
    out.sort_by_key(|p| std::cmp::Reverse(p.scale));
    out
}
