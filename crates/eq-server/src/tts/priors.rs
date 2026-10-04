//! docs/tts.md S9 を参照
//! 同じまとまりの報 (priors) の集め方。純粋関数。

use crate::broadcast::native::model::{event_place, same_quake};
use crate::quake::{Event, EventBody};

/// ev より前に届いた、同じまとまりの報を届いた順に返す (docs/tts.md S9)。
pub fn priors_of<'a>(ev: &Event, events: &'a [Event]) -> Vec<&'a Event> {
    let first = events.iter().position(|x| x.id == ev.id);
    events
        .iter()
        .enumerate()
        .filter(|(i, x)| match first {
            Some(f) => *i < f,
            None => x.received_at_ms < ev.received_at_ms,
        })
        .map(|(_, x)| x)
        .filter(|x| same_group(ev, x))
        .collect()
}

fn same_group(ev: &Event, x: &Event) -> bool {
    match (&ev.body, &x.body) {
        (EventBody::Eew(a), EventBody::Eew(b)) => a.event_id == b.event_id,
        (EventBody::Quake(_), EventBody::Quake(_)) => match (event_place(ev), event_place(x)) {
            (Some(a), Some(b)) => same_quake(&a, &b),
            _ => false,
        },
        (EventBody::Tsunami(_), EventBody::Tsunami(_)) => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ev(v: serde_json::Value) -> Event {
        serde_json::from_value(v).unwrap()
    }

    fn eew(id: &str, event_id: &str, serial: u32, at: u64) -> Event {
        ev(json!({
            "id": id, "source": "t", "received_at_ms": at, "kind": "eew",
            "event_id": event_id, "serial": serial.to_string(), "cancelled": false,
            "test": false, "issued_at": "2024/01/01 16:00:00",
            "origin_time": "2024/01/01 16:00:00", "origin_time_ms": 1704092400000i64,
            "hypocenter": null, "areas": [], "pref_max": [], "max_scale": 40
        }))
    }

    /// 震源の有無と発生時刻 (ms) を指定した地震情報
    fn quake(id: &str, origin_ms: i64, hypo: bool, at: u64) -> Event {
        let h = if hypo {
            json!({"name": "x", "latitude": 37.5, "longitude": 137.3,
                   "depth_km": 10, "magnitude": 6.0})
        } else {
            json!(null)
        };
        ev(json!({
            "id": id, "source": "t", "received_at_ms": at, "kind": "quake",
            "info_type": if hypo { "destination" } else { "scale_prompt" },
            "origin_time": "2024/01/01 16:10:00", "origin_time_ms": origin_ms,
            "issued_at": "2024/01/01 16:12:00", "hypocenter": h, "max_scale": 40,
            "domestic_tsunami": "None", "points": [], "pref_max": [], "comment": ""
        }))
    }

    fn tsunami(id: &str, at: u64) -> Event {
        ev(json!({
            "id": id, "source": "t", "received_at_ms": at, "kind": "tsunami",
            "cancelled": false, "issued_at": "2024/01/01 16:12:00", "areas": []
        }))
    }

    fn ids<'a>(v: &[&'a Event]) -> Vec<&'a str> {
        v.iter().map(|e| e.id.as_str()).collect()
    }

    const T0: i64 = 1_704_093_000_000;

    #[test]
    fn eew_returns_earlier_reports_of_same_event_id_in_order() {
        let list = vec![
            eew("a1", "E1", 1, 1),
            eew("b1", "E2", 1, 2),
            eew("a2", "E1", 2, 3),
            eew("a3", "E1", 3, 4),
            eew("a4", "E1", 4, 5),
        ];
        assert_eq!(ids(&priors_of(&list[3], &list)), ["a1", "a2"]);
        assert!(priors_of(&list[0], &list).is_empty());
    }

    #[test]
    fn quake_collects_near_in_time_quakes_only() {
        let list = vec![
            quake("q1", T0, true, 1),
            quake("far", T0 + 600_000, true, 2),
            quake("q2", T0 + 30_000, true, 3),
            quake("q3", T0, true, 4),
        ];
        assert_eq!(ids(&priors_of(&list[3], &list)), ["q1", "q2"]);
    }

    #[test]
    fn quake_without_coordinates_matches_by_origin_time() {
        let list = vec![quake("q1", T0, true, 1), quake("sp", T0, false, 2)];
        assert_eq!(ids(&priors_of(&list[1], &list)), ["q1"]);
    }

    #[test]
    fn quake_never_includes_eew_or_tsunami() {
        let list = vec![eew("e1", "E1", 1, 1), tsunami("t1", 2), quake("q1", T0, true, 3)];
        assert!(priors_of(&list[2], &list).is_empty());
    }

    #[test]
    fn tsunami_collects_all_earlier_tsunami_only() {
        let list = vec![
            tsunami("t1", 1),
            quake("q1", T0, true, 2),
            tsunami("t2", 3),
            eew("e1", "E1", 1, 4),
            tsunami("t3", 5),
        ];
        assert_eq!(ids(&priors_of(&list[4], &list)), ["t1", "t2"]);
    }

    #[test]
    fn userquake_and_detection_have_no_priors() {
        let uq = ev(json!({
            "id": "u", "source": "t", "received_at_ms": 9, "kind": "userquake",
            "started_at": "x", "updated_at": "x", "count": 1, "confidence": 0.5, "areas": []
        }));
        let det = ev(json!({
            "id": "d", "source": "t", "received_at_ms": 9, "kind": "eew_detection",
            "detection_type": "Full"
        }));
        let list = vec![tsunami("t1", 1), quake("q1", T0, true, 2), uq.clone(), det.clone()];
        assert!(priors_of(&uq, &list).is_empty());
        assert!(priors_of(&det, &list).is_empty());
    }

    #[test]
    fn event_absent_from_list_uses_received_at() {
        let list = vec![eew("a1", "E1", 1, 10), eew("a2", "E1", 2, 20), eew("a3", "E1", 3, 30)];
        let me = eew("a9", "E1", 9, 25);
        assert_eq!(ids(&priors_of(&me, &list)), ["a1", "a2"]);
    }

    fn noto() -> Vec<Event> {
        let s = include_str!("../../../../web/public/demo/noto2024.json");
        let v: serde_json::Value = serde_json::from_str(s).unwrap();
        serde_json::from_value(v["events"].clone()).unwrap()
    }

    fn find<'a>(list: &'a [Event], id: &str) -> &'a Event {
        list.iter().find(|e| e.id == id).unwrap()
    }

    #[test]
    fn noto_quake_priors_are_the_earlier_reports_of_same_quake() {
        let list = noto();
        let got = priors_of(find(&list, "65926682f0f6de0007564792"), &list);
        // 16:06 の前震と 16:18 の余震は含まない
        assert_eq!(
            ids(&got),
            [
                "659265cbf0f6de0007564686",
                "659265cbf0f6de00075646bb",
                "659265e9f0f6de0007564705",
                "65926625f0f6de000756473c",
                "65926646f0f6de0007564743",
                "65926682f0f6de000756477e",
            ]
        );
    }

    #[test]
    fn noto_tsunami_priors_are_earlier_tsunami() {
        let list = noto();
        let got = priors_of(find(&list, "65926857f0f6de0007564895"), &list);
        assert_eq!(ids(&got), ["65926607f0f6de0007564727"]);
    }

    #[test]
    fn noto_main_shock_eew_serial_34_has_33_earlier_serials() {
        let list = noto();
        let me = list.iter().find(|e| e.id == "wolfx-20240101161010-34").unwrap();
        let got = priors_of(me, &list);
        assert_eq!(got.len(), 33);
        assert!(got.iter().all(|e| matches!(&e.body,
            crate::quake::EventBody::Eew(x) if x.event_id == "20240101161010")));
    }
}
