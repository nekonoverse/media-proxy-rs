# media-proxy-rs
## misskey/cherrypick用メディアプロキシのrust実装
機能的には互換性を維持しつつ、様々な画像形式のデコードに対応  
ほとんどの画像読み書きに[image crate v0.25](https://crates.io/crates/image/0.25.10)を使用しています

## 実行(Docker)
```
docker run -itd -p 12766:12766 ghcr.io/nekonoverse/media-proxy-rs:main
```

### Unix Domain Socket(UDS)で利用する場合
`config.json`の`bind_addr`を`unix:///path/to.sock`形式で設定するとUDSモードで起動します
(`/`で始まるパスや`.sock`で終わるパスをそのまま指定する旧書式も後方互換のため動作しますが、
起動時に非推奨警告が出力されるため`unix://`を付けた書式への移行を推奨します)
nginxなどのリバースプロキシと同じボリュームを共有してソケットファイル経由で通信できます

ソケットファイルのパーミッションは`unix_socket_permissions`(8進数文字列、例: `"0660"`)で指定でき、
未設定時は`0660`になります。プロキシと異なるユーザーで動くリバースプロキシから接続する場合は
グループ権限が届くように調整してください

config.json:
```json
{
  "bind_addr": "unix:///run/media-proxy-rs/media-proxy.sock",
  "unix_socket_permissions": "0660"
}
```

docker-compose.yml:
```yaml
services:
  media-proxy:
    image: ghcr.io/nekonoverse/media-proxy-rs:main
    volumes:
      - media-proxy-sock:/run/media-proxy-rs
      - ./config.json:/media-proxy-rs/config.json:ro

  nginx:
    image: nginx:stable-alpine
    ports:
      - "80:80"
    volumes:
      - media-proxy-sock:/run/media-proxy-rs
      - ./nginx.conf:/etc/nginx/conf.d/default.conf:ro

volumes:
  media-proxy-sock:
```

nginx.conf:
```nginx
upstream media-proxy {
    server unix:/run/media-proxy-rs/media-proxy.sock;
}
server {
    listen 80;
    location / {
        proxy_pass http://media-proxy;
    }
}
```

コンテナ内のユーザーはdistrolessの`nonroot`(UID/GID=65532)で動作するため、ソケットディレクトリの権限に注意してください

## 実行(Linux)
例(x86_64/amd64)
```
curl -L https://github.com/nekonoverse/media-proxy-rs/releases/download/nightly/media-proxy-rs_linux-amd64.gz | gzip -d > ./media-proxy-rs
chmod u+x ./media-proxy-rs
./media-proxy-rs
```
配布しているバイナリはx86-64-v3向けにビルドした`media-proxy-rs_linux-amd64.gz`のみです(下記target support参照)

> Docker イメージは `gcr.io/distroless/static-debian13` ベースに移行したため、shell や `docker exec sh` が使えません。トラブル時はファイルシステム確認に `docker create <image> && docker export <cid> | tar -tvf -` などを利用してください。

## ライセンス通知

本ソフトウェアは Apache-2.0 です。依存コンポーネントの著作権表示・ライセンス全文と、
`mp4parse` (MPL-2.0) のソース入手先は
[THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md) に記載しています。Docker イメージでは
`/media-proxy-rs/LICENSE` と `/media-proxy-rs/THIRD-PARTY-NOTICES.md` に同梱されます。

## 設定ファイル
環境変数`MEDIA_PROXY_CONFIG_PATH`を設定する事でファイルの場所を指定できます  
デフォルト値は`$(pwd)/config.json`です  
十分に強力なマシンでは`encode_avif`を`true`に変更することでAVIFエンコードを利用する事ができます

以下の環境変数はconfig.jsonの同名設定(SSRF対策のネットワーク許可/拒否リスト)にカンマ区切りで追記されます
- `MEDIA_PROXY_ALLOWED_NETWORKS`
- `MEDIA_PROXY_BLOCKED_NETWORKS`
- `MEDIA_PROXY_BLOCKED_HOSTS`

`MEDIA_PROXY_MEMORY_BUDGET_MIB`でフェッチ/デコード処理全体のメモリ予算(MiB)を変更できます。未設定時は1024です

`proxy`でHTTPプロキシ経由の取得を設定する場合、そのプロキシ自身が同等のSSRF対策を行っていることを確認した上で
`unsafe_allow_proxy`を`true`にしてください。未設定/`false`のままだと`proxy`設定は拒否され起動時にエラー終了します

## target support
- [x] x86_64-unknown-linux-musl

(Docker イメージおよびリリースバイナリはamd64専用です。過去にはaarch64/armv7/riscv64向けのクロスビルドにも対応していましたが、
現在このフォークではメンテナンスコストの都合でamd64のみをサポート対象としています。詳細は[AGENT.md](AGENT.md)を参照してください)

## ビルド(x64 Docker)
1. `git clone https://github.com/nekonoverse/media-proxy-rs && cd media-proxy-rs`
2. `docker build -t media-proxy-rs .`

## プラットフォーム最適化
デフォルトでx86-64-v3向けにビルドしますが、x86-64-v3未満の環境やx86-64-v4向け最適化を利用したい場合は
`./crossfiles/amd64.sh`のRUSTFLAGSを編集してください
最も簡単なのはtarget-cpu=nativeを指定し、実行環境と同じCPUでビルドする方法です

## ビルド(x64 Debian系)
この方法では`x86_64-unknown-linux-gnu`向けにビルドします  
すべてを静的に組み込むmusl系とは異なる共有ライブラリを必要とする場合があります
1. https://www.rust-lang.org/ja/tools/install に従ってrustをインストール
1. `apt-get install -y meson ninja-build pkg-config nasm git`
2. `git clone https://github.com/nekonoverse/media-proxy-rs && cd media-proxy-rs`
3. `cargo build --release`

Docker以外の環境でヘルスチェック(`/healthz`)用の実行ファイルが必要な場合は`cargo build --release --example healthcheck`でも
ビルドしてください。`config.json`の`bind_addr`を見てTCP/UDSのどちらでも自動判定して疎通確認します

## 対応する画像形式
- AVIF(dav1d)
- BMP
- DDS
- Farbfeld
- GIF
- HDR
- ICO(png+rgba not support)
- JPEG
- EXR
- PNG
- PNM
- QOI
- TGA
- TIFF
- WebP
- JPEG XL(jxl-oxide)
- JPEG 2000(openjp2)
- JPEG XR(jxrlib)
- MNG (MNG-LC規格相当)
