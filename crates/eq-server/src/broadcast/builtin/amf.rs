//! AMF0 (RTMP のコマンドと FLV の onMetaData に使う) の最小の書き込みと読み込み。

use anyhow::Context;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Num(f64),
    Bool(bool),
    Str(String),
    Obj(Vec<(String, Value)>),
    Null,
}

impl Value {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_num(&self) -> Option<f64> {
        match self {
            Value::Num(n) => Some(*n),
            _ => None,
        }
    }

    /// オブジェクトの項目
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Obj(kv) => kv.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
}

pub fn write(out: &mut Vec<u8>, v: &Value) {
    match v {
        Value::Num(n) => {
            out.push(0x00);
            out.extend_from_slice(&n.to_be_bytes());
        }
        Value::Bool(b) => out.extend_from_slice(&[0x01, *b as u8]),
        Value::Str(s) => {
            out.push(0x02);
            write_short_str(out, s);
        }
        Value::Obj(kv) => {
            out.push(0x03);
            for (k, v) in kv {
                write_short_str(out, k);
                write(out, v);
            }
            out.extend_from_slice(&[0x00, 0x00, 0x09]);
        }
        Value::Null => out.push(0x05),
    }
}

/// 文字列の長さ (2 バイト) つきで書く (オブジェクトのキーにも使う)
fn write_short_str(out: &mut Vec<u8>, s: &str) {
    out.extend_from_slice(&(s.len().min(u16::MAX as usize) as u16).to_be_bytes());
    out.extend_from_slice(&s.as_bytes()[..s.len().min(u16::MAX as usize)]);
}

/// 値を続けて書く (コマンドの引数の並び)
pub fn write_all(values: &[Value]) -> Vec<u8> {
    let mut out = Vec::new();
    values.iter().for_each(|v| write(&mut out, v));
    out
}

pub fn s(x: &str) -> Value {
    Value::Str(x.to_string())
}

/// 値を 1 つ読み、残りを返す
fn read(buf: &[u8]) -> anyhow::Result<(Value, &[u8])> {
    let (&marker, rest) = buf.split_first().context("AMF: 途中で終わった")?;
    match marker {
        0x00 => {
            anyhow::ensure!(rest.len() >= 8, "AMF: 途中で終わった");
            let n = f64::from_be_bytes(rest[..8].try_into()?);
            Ok((Value::Num(n), &rest[8..]))
        }
        0x01 => {
            let (&b, rest) = rest.split_first().context("AMF: 途中で終わった")?;
            Ok((Value::Bool(b != 0), rest))
        }
        0x02 => {
            let (s, rest) = read_short_str(rest)?;
            Ok((Value::Str(s), rest))
        }
        0x03 => {
            let mut kv = Vec::new();
            let mut rest = rest;
            loop {
                let (k, r) = read_short_str(rest)?;
                if k.is_empty() {
                    // 終わりの印は 00 00 09
                    return Ok((Value::Obj(kv), r.get(1..).context("AMF: 途中で終わった")?));
                }
                let (v, r) = read(r)?;
                kv.push((k, v));
                rest = r;
            }
        }
        // null / undefined
        0x05 | 0x06 => Ok((Value::Null, rest)),
        m => anyhow::bail!("AMF: 知らない型 {m}"),
    }
}

fn read_short_str(buf: &[u8]) -> anyhow::Result<(String, &[u8])> {
    anyhow::ensure!(buf.len() >= 2, "AMF: 途中で終わった");
    let n = u16::from_be_bytes([buf[0], buf[1]]) as usize;
    anyhow::ensure!(buf.len() >= 2 + n, "AMF: 途中で終わった");
    Ok((String::from_utf8_lossy(&buf[2..2 + n]).into_owned(), &buf[2 + n..]))
}

/// 値の並びを、読めるところまで読む (知らない型が出たらそこで止める)
pub fn read_all(mut buf: &[u8]) -> Vec<Value> {
    let mut out = Vec::new();
    while !buf.is_empty() {
        match read(buf) {
            Ok((v, rest)) => {
                out.push(v);
                buf = rest;
            }
            Err(_) => break,
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_round_trip() {
        let vs = vec![
            s("connect"),
            Value::Num(1.0),
            Value::Obj(vec![("app".into(), s("live2")), ("ok".into(), Value::Bool(true))]),
            Value::Null,
        ];
        assert_eq!(read_all(&write_all(&vs)), vs);
    }

    #[test]
    fn a_string_is_length_prefixed_and_a_number_is_big_endian_f64() {
        assert_eq!(write_all(&[s("ab")]), [0x02, 0, 2, b'a', b'b']);
        assert_eq!(write_all(&[Value::Num(1.0)]), [0x00, 0x3f, 0xf0, 0, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn truncated_input_stops_without_panicking() {
        assert!(read_all(&[0x02, 0, 9, b'a']).is_empty());
        assert!(read_all(&[0x03, 0, 1]).is_empty());
    }
}
