//! RTMP / RTMPS の送信側 (handshake、connect、createStream、publish、メディアの送り出し)。
//! 送るだけなので、受け取る側は返事を読み、Ping と Acknowledgement に答えるだけにしている。
//! 送り先の URL (ストリームキー入り) はエラーやログに出さない。

use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadHalf, WriteHalf};
use tokio::net::TcpStream;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use tokio::task::JoinHandle;

use super::amf::{self, Value};
use super::chunk::{self, Message, Reader};
use super::flv;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

trait Transport: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Transport for T {}
type Io = Box<dyn Transport>;

/// rtmp[s]://host[:port]/app/stream の分解
#[derive(Debug, PartialEq)]
pub struct Target {
    pub secure: bool,
    pub host: String,
    pub port: u16,
    pub app: String,
    /// ストリーム名 (キー。問い合わせがあればそれも含む)
    pub stream: String,
}

impl Target {
    pub fn parse(url: &str) -> anyhow::Result<Self> {
        let (secure, rest) = match url.split_once("://") {
            Some(("rtmps", r)) => (true, r),
            Some(("rtmp", r)) => (false, r),
            _ => anyhow::bail!("送り先は rtmp:// か rtmps:// で始めてください"),
        };
        let (hostport, path) = rest.split_once('/').context("送り先に app がありません")?;
        let (app, stream) = path.split_once('/').unwrap_or((path, ""));
        anyhow::ensure!(
            !app.is_empty() && !stream.is_empty(),
            "送り先は rtmp://host/app/stream の形にしてください"
        );
        let (host, port) = match hostport.rsplit_once(':') {
            Some((h, p)) if !p.contains(']') => (h, p.parse().context("送り先のポート")?),
            _ => (hostport, if secure { 443 } else { 1935 }),
        };
        Ok(Self {
            secure,
            host: host.trim_matches(['[', ']']).to_string(),
            port,
            app: app.to_string(),
            stream: stream.to_string(),
        })
    }

    fn tc_url(&self) -> String {
        let scheme = if self.secure { "rtmps" } else { "rtmp" };
        format!("{scheme}://{}:{}/{}", self.host, self.port, self.app)
    }
}

pub struct Client {
    w: WriteHalf<Io>,
    stream_id: u32,
    /// 受け取る側の仕事 (終わったら理由を返す)
    reader: JoinHandle<anyhow::Error>,
    /// 読む側が作った返事 (Ping・Acknowledgement)。次に送るときに一緒に出す
    replies: UnboundedReceiver<Vec<u8>>,
}

impl Client {
    pub async fn connect(url: &str) -> anyhow::Result<Self> {
        let target = Target::parse(url)?;
        tokio::time::timeout(CONNECT_TIMEOUT, Self::open(&target))
            .await
            .context("RTMP の接続がタイムアウトしました")?
    }

    async fn open(t: &Target) -> anyhow::Result<Self> {
        let tcp = TcpStream::connect((t.host.as_str(), t.port))
            .await
            .with_context(|| format!("{}:{} につなげません", t.host, t.port))?;
        tcp.set_nodelay(true)?;
        let io: Io = if t.secure {
            Box::new(tls(&t.host, tcp).await?)
        } else {
            Box::new(tcp)
        };
        let (mut r, mut w) = tokio::io::split(io);
        handshake(&mut r, &mut w).await?;
        let mut reader = Reader::new();
        w.write_all(&chunk::set_chunk_size(chunk::SEND_CHUNK as u32)).await?;
        let stream_id = publish(t, &mut r, &mut w, &mut reader).await?;
        let (tx, replies) = unbounded_channel();
        let reader = tokio::spawn(read_loop(r, reader, tx));
        Ok(Self {
            w,
            stream_id,
            reader,
            replies,
        })
    }

    /// メディア (FLV のタグの本体) を 1 つ送る
    pub async fn send(&mut self, kind: u8, ts: u32, body: &[u8]) -> anyhow::Result<()> {
        while let Ok(r) = self.replies.try_recv() {
            self.w.write_all(&r).await.context("RTMP の送信")?;
        }
        let csid = match kind {
            flv::TAG_AUDIO => 4,
            flv::TAG_DATA => 5,
            _ => 6,
        };
        self.w
            .write_all(&chunk::chunks(csid, ts, kind, self.stream_id, body))
            .await
            .context("RTMP の送信")
    }

    /// 送り先が切れた (読む側が終わった) 理由。切れるまで返らない
    pub async fn closed(&mut self) -> anyhow::Error {
        match (&mut self.reader).await {
            Ok(e) => e,
            Err(e) => e.into(),
        }
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        self.reader.abort();
    }
}

async fn tls(host: &str, tcp: TcpStream) -> anyhow::Result<tokio_rustls::client::TlsStream<TcpStream>> {
    let mut roots = rustls::RootCertStore::empty();
    for c in rustls_native_certs::load_native_certs().certs {
        // 読めない証明書は飛ばす (残りで検証できれば足りる)
        let _ = roots.add(c);
    }
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let cfg = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()?
        .with_root_certificates(roots)
        .with_no_client_auth();
    let name = rustls::pki_types::ServerName::try_from(host.to_string()).context("送り先のホスト名")?;
    tokio_rustls::TlsConnector::from(Arc::new(cfg))
        .connect(name, tcp)
        .await
        .context("TLS の接続")
}

/// 単純な handshake (C0 C1 を送り、S0 S1 S2 を読んで、S1 を C2 として返す)
async fn handshake(r: &mut ReadHalf<Io>, w: &mut WriteHalf<Io>) -> anyhow::Result<()> {
    let mut c1 = vec![0u8; 1536];
    // 時刻 4 バイト + 0 4 バイト + 残りは任意の値
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(1, |d| d.subsec_nanos() | 1);
    let mut x = seed;
    for b in c1[8..].iter_mut() {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        *b = x as u8;
    }
    let mut out = vec![3u8];
    out.extend_from_slice(&c1);
    w.write_all(&out).await?;
    let mut s0 = [0u8; 1];
    r.read_exact(&mut s0).await.context("handshake")?;
    anyhow::ensure!(s0[0] == 3, "RTMP のバージョンが違います");
    let mut s1 = vec![0u8; 1536];
    r.read_exact(&mut s1).await.context("handshake")?;
    let mut s2 = vec![0u8; 1536];
    r.read_exact(&mut s2).await.context("handshake")?;
    w.write_all(&s1).await?;
    Ok(())
}

fn command(values: &[Value]) -> Vec<u8> {
    chunk::chunks(3, 0, chunk::MSG_COMMAND, 0, &amf::write_all(values))
}

/// コマンドの返事 (name, 通番, 残りの引数)
fn parse_command(m: &Message) -> Option<(String, f64, Vec<Value>)> {
    if m.kind != chunk::MSG_COMMAND {
        return None;
    }
    let mut v = amf::read_all(&m.body).into_iter();
    let name = v.next()?.as_str()?.to_string();
    let txn = v.next()?.as_num().unwrap_or(0.0);
    Some((name, txn, v.collect()))
}

/// connect → createStream → publish まで進め、メディアを送るストリーム ID を返す
async fn publish(t: &Target, r: &mut ReadHalf<Io>, w: &mut WriteHalf<Io>, rd: &mut Reader) -> anyhow::Result<u32> {
    let connect = Value::Obj(vec![
        ("app".into(), amf::s(&t.app)),
        ("type".into(), amf::s("nonprivate")),
        ("flashVer".into(), amf::s("FMLE/3.0 (compatible; eq-server)")),
        ("tcUrl".into(), amf::s(&t.tc_url())),
    ]);
    w.write_all(&command(&[amf::s("connect"), Value::Num(1.0), connect]))
        .await?;
    wait_result(r, rd, 1.0).await.context("connect")?;
    let name = amf::s(&t.stream);
    w.write_all(&command(&[
        amf::s("releaseStream"),
        Value::Num(2.0),
        Value::Null,
        name.clone(),
    ]))
    .await?;
    w.write_all(&command(&[
        amf::s("FCPublish"),
        Value::Num(3.0),
        Value::Null,
        name.clone(),
    ]))
    .await?;
    w.write_all(&command(&[amf::s("createStream"), Value::Num(4.0), Value::Null]))
        .await?;
    let args = wait_result(r, rd, 4.0).await.context("createStream")?;
    let stream_id = args
        .get(1)
        .and_then(Value::as_num)
        .context("createStream の返事にストリーム ID がありません")? as u32;
    let publish = amf::write_all(&[amf::s("publish"), Value::Num(5.0), Value::Null, name, amf::s("live")]);
    w.write_all(&chunk::chunks(3, 0, chunk::MSG_COMMAND, stream_id, &publish))
        .await?;
    loop {
        let m = rd.next(r).await?;
        if let Some((name, _, args)) = parse_command(&m) {
            if name == "_error" {
                anyhow::bail!("publish が断られました");
            }
            if name == "onStatus" {
                let info = args.get(1).cloned().unwrap_or(Value::Null);
                let code = info.get("code").and_then(Value::as_str).unwrap_or("");
                if code == "NetStream.Publish.Start" {
                    return Ok(stream_id);
                }
                if info.get("level").and_then(Value::as_str) == Some("error") {
                    anyhow::bail!("publish が断られました: {code}");
                }
            }
        }
    }
}

/// 通番 txn の _result を待つ (返ってきた引数を返す。_error なら失敗)
async fn wait_result(r: &mut ReadHalf<Io>, rd: &mut Reader, txn: f64) -> anyhow::Result<Vec<Value>> {
    loop {
        let m = rd.next(r).await?;
        if let Some((name, t, args)) = parse_command(&m) {
            if t == txn && name == "_result" {
                return Ok(args);
            }
            if t == txn && name == "_error" {
                anyhow::bail!("サーバが断りました: {:?}", args.get(1).and_then(|v| v.get("code")));
            }
        }
    }
}

/// 接続してからの受信。Ping と Acknowledgement に答え、切れたら理由を返す
async fn read_loop(mut r: ReadHalf<Io>, mut rd: Reader, replies: UnboundedSender<Vec<u8>>) -> anyhow::Error {
    let (mut window, mut acked) = (0u64, 0u64);
    loop {
        let m = match rd.next(&mut r).await {
            Ok(m) => m,
            Err(e) => return e.context("送り先が切れました"),
        };
        match m.kind {
            chunk::MSG_USER_CONTROL if m.body.get(..2) == Some(&[0, 6]) => {
                let _ = replies.send(chunk::ping_response(&m.body));
            }
            chunk::MSG_ACK_WINDOW if m.body.len() >= 4 => {
                window = u32::from_be_bytes([m.body[0], m.body[1], m.body[2], m.body[3]]) as u64;
            }
            chunk::MSG_COMMAND => {
                if let Some((name, _, args)) = parse_command(&m) {
                    let info = args.get(1).cloned().unwrap_or(Value::Null);
                    let code = info.get("code").and_then(Value::as_str).unwrap_or("");
                    if name == "onStatus" && info.get("level").and_then(Value::as_str) == Some("error") {
                        return anyhow::anyhow!("送り先がエラーを返しました: {code}");
                    }
                    if name == "onStatus" && code.starts_with("NetStream.Unpublish") {
                        return anyhow::anyhow!("送り先が配信を止めました: {code}");
                    }
                }
            }
            _ => {}
        }
        if window > 0 && rd.received - acked >= window {
            acked = rd.received;
            let _ = replies.send(chunk::ack(rd.received as u32));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_are_split_into_host_port_app_and_stream() {
        let t = Target::parse("rtmps://a.rtmps.youtube.com/live2/abcd-1234").unwrap();
        assert_eq!(
            t,
            Target {
                secure: true,
                host: "a.rtmps.youtube.com".into(),
                port: 443,
                app: "live2".into(),
                stream: "abcd-1234".into()
            }
        );
        let t = Target::parse("rtmp://127.0.0.1:19350/live/test?x=1").unwrap();
        assert_eq!((t.port, t.app.as_str(), t.stream.as_str()), (19350, "live", "test?x=1"));
        assert_eq!(t.tc_url(), "rtmp://127.0.0.1:19350/live");
        assert_eq!(Target::parse("rtmp://h/live/k").unwrap().port, 1935);
        assert!(Target::parse("http://h/live/k").is_err());
        assert!(Target::parse("rtmp://h/live").is_err());
        assert!(Target::parse("rtmp://h").is_err());
    }

    #[test]
    fn errors_about_a_bad_url_do_not_repeat_it() {
        let e = Target::parse("rtmp://h:xx/live/secret-key").unwrap_err();
        assert!(!format!("{e:#}").contains("secret-key"));
    }

    #[test]
    fn commands_are_amf_on_chunk_stream_3() {
        let c = command(&[amf::s("createStream"), Value::Num(4.0), Value::Null]);
        assert_eq!(c[0], 3);
        assert_eq!(c[7], chunk::MSG_COMMAND);
        let m = Message {
            kind: chunk::MSG_COMMAND,
            body: c[12..].to_vec(),
        };
        let (name, txn, args) = parse_command(&m).unwrap();
        assert_eq!((name.as_str(), txn, args), ("createStream", 4.0, vec![Value::Null]));
    }

    #[tokio::test]
    async fn the_handshake_sends_c0_c1_then_echoes_s1_as_c2() {
        let (client, mut server) = tokio::io::duplex(8192);
        let io: Io = Box::new(client);
        let (mut r, mut w) = tokio::io::split(io);
        let srv = tokio::spawn(async move {
            let mut c01 = vec![0u8; 1537];
            server.read_exact(&mut c01).await.unwrap();
            assert_eq!(c01[0], 3);
            let s1: Vec<u8> = (0..1536).map(|i| (i % 251) as u8).collect();
            let mut s = vec![3u8];
            s.extend_from_slice(&s1);
            s.extend_from_slice(&c01[1..]);
            server.write_all(&s).await.unwrap();
            let mut c2 = vec![0u8; 1536];
            server.read_exact(&mut c2).await.unwrap();
            assert_eq!(c2, s1);
        });
        handshake(&mut r, &mut w).await.unwrap();
        srv.await.unwrap();
    }
}
