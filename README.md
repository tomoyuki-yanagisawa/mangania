# MangaNia

ZIP 内の漫画画像を表示する、小さなネイティブビューアの試作です。
Windows 7 を優先しつつ、macOS / Linux でもビルドできる構成にしています。
設計は [docs/DESIGN.v1.md](docs/DESIGN.v1.md) を参照してください。

## 使い方

```sh
cargo run --release -- path/to/book.zip
# 先読みページ数を指定する場合（既定 2）
cargo run --release -- --prefetch 4 path/to/book.zip
```

引数なしで起動した場合は空のウィンドウが開きます。ZIP をウィンドウへドラッグ＆ドロップすると読み込みます。

GUIを開かずに確認することもできます。

```sh
# ページ一覧（自然順）を表示
cargo run --release -- --list path/to/book.zip

# 全ページをデコードしてエラーを確認（失敗時は終了コード1）
cargo run --release -- --check path/to/book.zip

# 指定ページを画像として出力（1始まり）
cargo run --release -- --render-page 1 out.png path/to/book.zip
```

| 操作 | キー・マウス |
| --- | --- |
| 次のページ | Right / 左クリック / ホイール下 |
| 前のページ | Left / 右クリック / ホイール上 |
| 先頭 / 末尾 | Home / End |
| 1ページ / 見開きの切替 | メニューの「表示」、または V（Linux 用フォールバック） |
| 全画面の切替 | メニューの「表示」>「全画面表示」、または F11 |
| 表示中のページを保存 | メニューの「ファイル」>「表示中のページを保存」、または Ctrl+S / Cmd+S |
| 終了 | Esc（全画面中は全画面解除。ウィンドウ表示中は終了） |

マウスはウィンドウ内の任意の位置で操作できます。ホイールはトラックパッドの小さなスクロールを約50px分ためて1ページ送りにします。見開き時はクリック・ホイールとも2ページ単位です。

メニューからも同じ操作ができます。

- **ファイル**: 表示中のページを保存
- **移動**: 次のページ / 前のページ / 先頭へ / 末尾へ
- **表示**: 1ページ表示 / 見開き表示 / 全画面表示

## 表示モード

- 既定は **1ページ表示** です。
- メニューの「表示」から **1ページ表示 / 見開き表示** を切り替えられます。
- 選んだ表示モードは設定ファイルに保存され、次回起動時に復元されます。
- 見開きは漫画向けに、現在ページを右、次ページを左に並べます（右→左）。
- 見開き時はページ送りが2ページ単位になり、末尾で1ページだけになる場合は中央に表示します。
- 見開きの組は先頭から `(1,2)`, `(3,4)`, ... です。表紙を単独表示する設定は未実装です。
- ネイティブメニューは Windows / macOS のみです。Linux では `V` キーで切り替えます。

ウィンドウサイズに合わせて画像をフィットして中央に表示します。

## 表示中のページを保存

「ファイル」>「表示中のページを保存」、または `Ctrl+S` / `Cmd+S` で、現在表示しているページを画像ファイルとして書き出します。

- 見開き表示中でも **現在ページのみ** を保存します。
- ZIP 内の元画像バイト列をそのまま書き出すため、再エンコードによる画質劣化はありません。
- 拡張子は元画像のものを引き継ぎます。
- 出力先がなければ作成し、同名ファイルは上書きします。
- 保存結果はタイトルバーに表示されます。

## 設定ファイル

表示モードと保存設定はTOMLで保存されます。

- macOS: `~/Library/Application Support/mangania/config.toml`
- Windows: `%APPDATA%\mangania\config.toml`
- Linux: `$XDG_CONFIG_HOME/mangania/config.toml`（未設定時は `~/.config/mangania/config.toml`）

```toml
spread = true
export_dir = "~/Pictures/MangaNia"
export_name = "{book}_{page:03}"
```

- `spread = true` が見開き表示、`false` が1ページ表示です。
- `export_dir` は保存先ディレクトリです。`~` を展開します。省略時は `~/Pictures/MangaNia` です。
- `export_name` は保存ファイル名のテンプレートです。省略時は `{book}_{page:03}` です。
- 拡張子はテンプレートに含めず、元画像から自動で付きます。

### ファイル名テンプレート

| プレースホルダー | 内容 | 例 |
| --- | --- | --- |
| `{book}` | ZIP 名（拡張子なし） | `book` |
| `{page}` | 1始まりのページ番号 | `7` |
| `{page:03}` | 3桁ゼロ埋めのページ番号 | `007` |
| `{page:000}` | `0` の数だけゼロ埋め | `007` |
| `{name}` | ZIP 内の元画像名（拡張子なし） | `001` |

未知のプレースホルダーはそのまま残ります。ファイル名に使えない文字やパス区切りは `_` に置換します。

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
- デコード済みページは `先読み枚数 + 2` 枚、かつ最大128MiBまで保持（現在ページは常に保持）
- 次ページの準備中は直前のフレームを保持して暗転を防ぐ
- 壊れた ZIP / 画像はエラーとして扱い、プロセスは落とさない
- パストラバーサル、隠しファイル、`__MACOSX` を除外
- エントリサイズ・エントリ数に上限を設け、対応形式を限定する
- 1ページ表示 / 見開き表示をメニューバーで切替
- 全画面表示（F11 / メニュー）
- 「移動」「表示」メニューからナビゲーションと表示モードを操作
- 左右クリックとホイールでページ送り
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
├── lib.rs       # ライブラリ入口（コアAPI）
├── main.rs      # CLI
├── app.rs       # winit イベントループ、D&D、UI
├── viewer.rs    # Viewer: ページ、現在位置、キャッシュ、Prefetch要求
├── cache.rs     # ページ数・バイト予算付きキャッシュ
├── navigation.rs # ページ移動の純粋関数
├── event.rs     # winit/menu のユーザーイベント
├── cli.rs       # 引数解析
├── config.rs    # TOML設定（見開き・保存設定の永続化）
├── export.rs    # 表示中ページの保存、ファイル名テンプレート
├── headless.rs  # GUIなしの list / check / render-page
├── archive.rs   # ZIP 索引・自然順・読み出し
├── decoder.rs   # JPEG / PNG / WebP デコード
├── prefetch.rs  # ワーカースレッドと優先度付きジョブ（winit非依存）
├── menu.rs      # Windows / macOS のネイティブメニュー
└── view.rs      # フィット・見開き配置・0RGB 描画

tests/
├── common/mod.rs
├── archive.rs
├── cache.rs
├── cli.rs
├── decoder.rs
├── config.rs
├── export.rs
├── headless.rs
├── navigation.rs
├── prefetch.rs
└── view.rs
```

## テスト

```sh
cargo test
cargo clippy --all-targets
```

自然順ソート、パス除外、PNG/JPEGデコード、1ページ/見開き描画、CLI引数、ヘッドレス `--check` / `--render-page`、キャッシュの枚数/バイト上限、ナビゲーション境界、winit非依存のPrefetch、設定のTOML読み書き、ファイル名テンプレート展開・サニタイズ・元画像バイト列の保存をテストしています。GUI操作は手動確認です。

## 設計書との主な差分

- UI は libui-ng ではなく `winit` + `softbuffer`（D&D 必須のため）
- ZIP は libarchive ではなく `zip` crate
- JPEG 2000 は対象外。設定画面、ディスクキャッシュ、閲覧位置保存は未実装
- 設定はメニューバーの表示モード・全画面切替のみ。読み方向は右→左固定
- macOS でのみ実行確認済み。Windows / Linux はコンパイル確認まで
- Windows 7 はビルド手順とコードのみ（実機未検証）
