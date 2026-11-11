# MangaNia

ZIP 内の漫画画像を表示する、小さなネイティブビューアの試作です。
Windows 7 を優先しつつ、macOS / Linux でもビルドできる構成にしています。

## 使い方

```sh
cargo run --release -- path/to/book.zip
# 先読みページ数を指定する場合（既定 2）
cargo run --release -- --prefetch 4 path/to/book.zip
```

引数なしで起動した場合は空のウィンドウが開きます。ZIP をウィンドウへドラッグ＆ドロップすると読み込みます。

| 操作 | キー |
| --- | --- |
| 次のページ | Right |
| 前のページ | Left |
| 1ページ / 見開きの切替 | メニューの「表示」、または V（Linux 用フォールバック） |
| 全画面の切替 | メニューの「表示」>「全画面表示」、または F11 |
| 終了 | Esc（全画面中は全画面解除。ウィンドウ表示中は終了） |

## 表示モード

- 既定は **1ページ表示** です。
- メニューの「表示」から **1ページ表示 / 見開き表示** を切り替えられます。
- 見開きは漫画向けに、現在ページを右、次ページを左に並べます（右→左）。
- 見開き時はページ送りが2ページ単位になり、末尾で1ページだけになる場合は中央に表示します。
- 見開きの組は先頭から `(1,2)`, `(3,4)`, ... です。表紙を単独表示する設定は未実装です。
- ネイティブメニューは Windows / macOS のみです。Linux では `V` キーで切り替えます。

ウィンドウサイズに合わせて画像をフィットして中央に表示します。

## 全画面

- メニューの「表示」>「全画面表示」、または `F11` で切り替えます。
- 全画面中は `Esc` で全画面を解除します。ウィンドウ表示中の `Esc` は終了です。
- ページ番号やエラーはタイトルバーに表示するため、全画面中は見えません。
- Windows の全画面ではメニューバーも隠れます。`F11` / `Esc` で戻れます。

## 対応形式

- JPEG（ベースライン / プログレッシブ）
- PNG
- WebP

JPEG 2000 は対象外です。

## 設計上の特徴

- ZIP の Central Directory を一度だけ索引化し、自然順に並べる
- 現在ページは最優先でデコードし、次ページ以降を `--prefetch` の枚数だけ先読み
- ZIP 読み出しと画像デコードはワーカースレッドで行い、UI をブロックしない
- デコード済みページは `先読み枚数 + 2` 枚まで保持
- 壊れた ZIP / 画像はエラーとして扱い、プロセスは落とさない
- パストラバーサル、隠しファイル、`__MACOSX` を除外
- エントリサイズ・エントリ数に上限を設け、対応形式を限定する
- 1ページ表示 / 見開き表示をメニューバーで切替
- 全画面表示（F11 / メニュー）
- UI は `winit` + `softbuffer`（D&D と CPU 描画のため）

## Windows 7 向けビルド

Windows 7 は Rust の Tier 3 ターゲットです。nightly と `build-std` が必要です。

```sh
rustup toolchain install nightly
rustup component add rust-src --toolchain nightly
cargo +nightly build -Z build-std --target x86_64-win7-windows-msvc --release
```

- MSVC ツールチェーンと、Windows 7 側の Universal CRT 更新が必要です。
- `winit` は Windows 10 以降の DPI API を動的に読み込み、存在しない場合はフォールバックします。
- ネイティブメニューは `muda` を使用しています。
- **Windows 7 実機での検証は未実施です。** クリーンな Win7 SP1 x64 環境での起動確認が残っています。

## 構成

```text
src/
├── main.rs      # 引数処理・起動
├── app.rs       # winit イベントループ、D&D、ページ状態
├── archive.rs   # ZIP 索引・自然順・読み出し
├── decoder.rs   # JPEG / PNG / WebP デコード
├── prefetch.rs  # ワーカースレッドと優先度付きジョブ
├── menu.rs      # Windows / macOS のネイティブメニュー
└── view.rs      # フィット・見開き配置・0RGB 描画
```

## テスト

```sh
cargo test
cargo clippy --all-targets
```

自然順ソートのみ自動テストしています。

## 設計書との主な差分

- UI は libui-ng ではなく `winit` + `softbuffer`（D&D 必須のため）
- ZIP は libarchive ではなく `zip` crate
- JPEG 2000 は対象外。設定画面、ディスクキャッシュ、閲覧位置保存は未実装
- 設定はメニューバーの表示モード切替のみ。読み方向は右→左固定
- macOS でのみ実行確認済み。Windows / Linux はコンパイル確認まで
- Windows 7 はビルド手順とコードのみ（実機未検証）
