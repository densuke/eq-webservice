# 配置手順 (Linux / systemd + Caddy)

リリースの tar.gz を展開すると、次のようになっています。

```
eq-server-<version>-<target>/
├── eq-server            # サーバ本体
├── web/dist/            # 地図ページ (eq-server がそのまま配信する)
├── config.example.toml  # 設定例
├── samples/             # デモデータ (demo.toml で架空の地震を再生)
└── deploy/              # systemd ユニット・Caddyfile の例
```

`eq-server` はまず作業ディレクトリの `web/dist` を探し、見つからなければ実行ファイルの隣の
`web/dist` を配信します。ディレクトリごと置けば、HTML などの配置を別にする必要はありません。

## 1. 展開

```sh
tar xzf eq-server-<version>-x86_64-unknown-linux-gnu.tar.gz
sudo mv eq-server-<version>-x86_64-unknown-linux-gnu /opt/eq-server
sudo useradd --system --home /opt/eq-server --shell /usr/sbin/nologin eq
sudo install -d -o eq -g eq /opt/eq-server/data
sudo cp /opt/eq-server/config.example.toml /opt/eq-server/config.toml   # 必要に応じて編集
```

## 2. まず手で動かしてみる

```sh
cd /opt/eq-server
./eq-server --config samples/demo.toml --port 8080   # デモ (架空の地震を繰り返し再生)
./eq-server --config config.toml --port 8080         # 本番 (P2P地震情報に接続)
curl http://127.0.0.1:8080/healthz                   # → ok
```

## 3. 常駐させる (systemd)

```sh
sudo cp /opt/eq-server/deploy/eq-server.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now eq-server
journalctl -u eq-server -f
```

ポートはユニットファイルの `Environment=EQ_PORT=8080` で変えられます。
Discord の Webhook URL などは `/etc/default/eq-server` に `DISCORD_WEBHOOK_URL=...` と書きます。

### メモリの記録と MALLOC_ARENA_MAX

eq-server と `eq-server broadcast` は、自分の cgroup (systemd のユニット) のメモリを 1 分ごとにログへ出します
(`memory: current_mb=... peak_mb=... max_mb=...`)。`MemoryMax` の 80% を超えると WARN になります。
最大値がいつ出たかは `journalctl -u eq-server | grep 'memory:'` (配信は `journalctl --user -u eq-broadcast`) で追えます。

ユニットには glibc の断片化を抑える `Environment=MALLOC_ARENA_MAX=2` が入っています。既存の配置に足すときは、
ユニットファイルを差し替えるか drop-in を作り、再起動します (配信は `PartOf` なので eq-server の再起動に連動)。

```sh
# system のユニット (eq-server)
sudo systemctl edit eq-server        # [Service] に Environment=MALLOC_ARENA_MAX=2 を書く
sudo systemctl restart eq-server
# user のユニット (eq-broadcast)
systemctl --user edit eq-broadcast   # 同じ内容
systemctl --user restart eq-broadcast
# 効いているか
tr '\0' '\n' < /proc/$(systemctl show -p MainPID --value eq-server)/environ | grep MALLOC_ARENA_MAX
```

## 4. Caddy から転送

`deploy/Caddyfile.example` を参考に `reverse_proxy 127.0.0.1:<port>` を追加して `caddy reload` します。
WebSocket (`/ws`) も `reverse_proxy` がそのまま通します。

## macOS (Apple Silicon) で試す場合

ブラウザでダウンロードした tar.gz は隔離属性が付くので、展開後に外してから起動します。

```sh
tar xzf eq-server-<version>-aarch64-apple-darwin.tar.gz
cd eq-server-<version>-aarch64-apple-darwin
xattr -d com.apple.quarantine eq-server 2>/dev/null || true
./eq-server --config samples/demo.toml   # → http://127.0.0.1:8080/
```

同じ Wi-Fi のスマホから見る場合は `--listen 0.0.0.0:8080` を付け、`http://<MacのIP>:8080/` を開きます。

## ポートの指定方法 (優先順)

1. 起動オプション `--port 8080` / `--listen 127.0.0.1:8080`
2. 環境変数 `EQ_PORT` / `EQ_LISTEN`
3. `config.toml` の `[server] listen`
