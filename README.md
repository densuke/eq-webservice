# eq-webservice

[P2P地震情報](https://www.p2pquake.net/) の WebSocket API から地震情報を受け取り、
日本地図の上に **震央・P波/S波の広がり・都道府県ごとの震度** をほぼリアルタイムで描くツールです。
受け取った情報は、RSS・Discord などの配信先（プラグイン）にも流せます。

```
P2P地震情報 (wss) ──▶ eq-server (Rust) ──▶ ブラウザ (静的ページ + TypeScript, WebSocket)
                           │
                           └─▶ プラグイン: RSS / Discord / JSON Lines 蓄積 / 汎用 Webhook
```

## 構成

| パス | 内容 |
| --- | --- |
| `crates/eq-core` | データモデルと P2P地震情報 JSON の変換（tokio 非依存。将来 WASM 化できるようにしてある） |
| `crates/eq-server` | 上流への接続（再接続付き）、重複排除、ブラウザ向け WebSocket、静的ファイル配信、プラグイン |
| `web/` | フロントエンド（TypeScript + esbuild、地図は SVG で自前描画。外部タイル不要） |
| `samples/` | 地震が起きていないときの確認用デモデータと設定 |
| `tools/simplify_geojson.py` | 都道府県境界データを軽量化するスクリプト |
| `tools/tsunami_areas.py` | 津波予報区の沿岸線データを軽量化するスクリプト |

### 扱う情報

| code | 内容 | 画面での表示 |
| --- | --- | --- |
| 556 | 緊急地震速報（警報） | 警報バナー、震央、P波・S波の円、予測震度で都道府県を塗る（白の破線） |
| 551 | 地震情報（震度速報・震源・各地の震度） | 震央、観測震度で都道府県を塗る、観測点の一覧 |
| 552 | 津波予報 | 対象の沿岸を等級の色（大津波警報=紫、津波警報=赤、津波注意報=黄）で描き、解除まで表示。バナー、一覧・詳細 |
| 554 | 緊急地震速報の発表検出 | 受信のみ |

同じ地震についての複数の情報（震度速報 → 震源 → 各地の震度、EEW の続報）は 1 行にまとめて表示します。

## 使い方

### ビルド済みのリリースを使う (サーバへの配置)

[Releases](https://github.com/densuke/eq-webservice/releases) から次のどちらかをダウンロードして展開します。
サーバ側で Rust や Node.js を使ってビルドする必要はありません。

- `eq-server-<version>-x86_64-unknown-linux-gnu.tar.gz` (Linux / amd64)
- `eq-server-<version>-aarch64-apple-darwin.tar.gz` (macOS / Apple Silicon)

バイナリ・地図ページ (`web/dist`)・設定例・systemd ユニット例が 1 つのディレクトリに入っています。
配置手順は [deploy/INSTALL.md](deploy/INSTALL.md) を見てください。

### 起動オプション

| オプション | 環境変数 | 内容 |
| --- | --- | --- |
| `-c, --config <path>` | | 設定ファイル (省略時は `./config.toml`、無ければ既定値) |
| `-p, --port <port>` | `EQ_PORT` | 待ち受けポートだけ変える |
| `-l, --listen <addr:port>` | `EQ_LISTEN` | 待ち受けアドレスとポート |
| `--static-dir <path>` | | 地図ページの場所 (既定は `web/dist`。無ければ実行ファイルの隣を探す) |
| `-V, --version` | | バージョン表示 |

優先順位は 起動オプション > 環境変数 > 設定ファイル (`[server] listen`) です。

### ソースからビルドする

必要なもの: Rust (stable)、Node.js 22 以降

```sh
# フロントエンドをビルド (web/dist に出力)
cd web && npm ci && npm run build && cd ..

# デモ: 架空の地震を「今」起きたものとして繰り返し再生
cargo run -p eq-server -- --config samples/demo.toml
# → http://127.0.0.1:8080/

# 本番: P2P地震情報に接続
cp config.example.toml config.toml   # 必要に応じて編集
cargo run --release -p eq-server -- --config config.toml
```

HTTP の口:

- `/` … 地図ページ
- `/ws` … ブラウザ向け WebSocket（接続時に `hello` で直近の情報とサーバ時刻、その後は `event` を送る）
- `/api/events` … 直近の情報（JSON）
- `/feed.xml` … RSS（rss プラグインを有効にした場合）
- `/healthz` … 死活監視

## プラグイン（配信先）

`config.toml` の `[[sinks]]` に並べます。共通のキー:

- `type` … `rss` / `discord` / `jsonl` / `webhook`
- `kinds` … 流す種類（`quake` / `eew` / `eew_detection` / `tsunami`）。省略するとすべて
- `min_scale` … 最大震度がこれ未満の地震情報・EEW は流さない（`"3"`, `"5弱"` など）
- `enabled`, `name`

各プラグインは別々のタスクで動くので、Discord が遅くても他の配信先や画面の更新は止まりません。
詳しくは [config.example.toml](config.example.toml) を見てください。

新しいプラグインを作るときは `crates/eq-server/src/plugins/` に `Sink` トレイトの実装を追加し、
`plugins::build()` に `type` 名を登録します。Rust 以外で書きたい場合は `webhook` プラグインで
正規化済みの JSON を受け取れます。

## リリースの作り方

`Cargo.toml` の `version` を上げてから、同じ番号のタグを push します。

```sh
git tag v0.1.0 && git push origin v0.1.0
```

CI がビルド・テストのあと、両ターゲットの tar.gz と sha256 を GitHub Releases に添付します。
`v0.2.0-rc1` のように `-` を含むタグはプレリリースになります。
GitHub の画面の「Draft a new release」で新しいタグを作って公開しても構いません
(CI が完了すると、そのリリースに tar.gz が追加されます)。

## リバースプロキシ (Caddy) の例

ページは相対パスで WebSocket・地図データを読むので、サブパスの下にも置けます。

```caddyfile
eq.example.jp {
	reverse_proxy 127.0.0.1:8080
}

# サブパスの下に置く場合
example.jp {
	handle_path /eq/* {
		reverse_proxy 127.0.0.1:8080
	}
}
```

設定例は [deploy/Caddyfile.example](deploy/Caddyfile.example) にもあります。
Caddy の `reverse_proxy` は WebSocket をそのまま通します。サーバは 30 秒ごとに ping を送るので、
アイドル状態での切断は起きにくいはずです。

## CI

GitHub Actions（`.github/workflows/ci.yml`）で次をビルド・テストします。

- web: 型チェック、テスト、ビルド
- rust: `x86_64-unknown-linux-gnu`（ubuntu）と `aarch64-apple-darwin`（macOS arm64）で
  fmt / clippy / test / release ビルド、配布用 tar.gz の作成、展開した tar.gz を使ったデモ再生での動作確認
- release: `v*` タグのときだけ GitHub Releases を作成

## 今後の拡張候補

- 観測点の座標表を持ち、震度を観測点ごとの点で描く（今は都道府県単位で塗り分け）
- P波・S波を気象庁の走時表 (JMA2001) で計算する（今は速度を一定とした概算）
- 推計震度分布のような重い計算が必要になったら、`eq-core` を WASM としてブラウザで使う

## 出典・ライセンス

- 地震情報: [P2P地震情報](https://www.p2pquake.net/)（気象庁発表の情報）
- 地図: 地球地図日本（国土地理院）を [dataofjapan/land](https://github.com/dataofjapan/land) 経由で加工。
  営利目的で使う場合は、国土地理院の利用規約に従って利用報告が必要です。
- 津波予報区: [気象庁「予報区等GISデータ」](https://www.data.jma.go.jp/developer/gis.html)を加工して作成
- ソースコード: GPL-3.0-or-later（[LICENSE](LICENSE)）
