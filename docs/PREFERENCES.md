# MangaNia の既定値と好み

更新日: 2026-11-12

この文書は、製品の既定値、UX の好み、開発時の方針を記録します。
アーキテクチャは [DESIGN.md](DESIGN.md)、技術判断の理由は [DECISIONS.md](DECISIONS.md) を参照してください。

## 1. UI / UX

- UI の言語は日本語。
- 画像領域を最大化し、装飾的な UI は置かない。
- 設定画面は作らない。設定は TOML ファイルで行う。
- エラー、読み込み状態、保存結果はタイトルバーと stderr に表示する。
- メニューは「ファイル」「移動」「表示」の最小構成にする。
- 全画面はボーダーレス。`Esc` は全画面中なら解除、通常ウィンドウなら終了。
- Linux にはネイティブメニューを置かず、既定のキーバインドで操作する。
- 見開きは右→左。現在ページを右、次ページを左に置く。
- 見開きの組は先頭から `(1, 2)`, `(3, 4)`, ... とする。
- 表紙の単独表示は行わない。
- ウィンドウサイズは既定で 1200×900。背景色は `#202020`。

## 2. 既定の設定

### config.toml

```toml
spread = false
export_dir = "~/Pictures/MangaNia"
export_name = "{book}_{page:03}"
```

- `spread = false`（1ページ表示）が既定。
- `export_dir` の省略時は `~/Pictures/MangaNia`。
- `export_name` の省略時は `{book}_{page:03}`。
- 同名ファイルは上書きする。
- 保存するのは現在ページのみで、ZIP 内の元画像バイト列をそのまま使う。

### keybindings.toml

```toml
[keys]
next = ["Right"]
prev = ["Left"]
first = ["Home"]
last = ["End"]
toggle_spread = ["v"]
single_page = []
spread_page = []
toggle_fullscreen = ["F11"]
save_page = ["Ctrl+S", "Cmd+S"]
quit = ["Escape"]
```

- 修飾キーは `Ctrl` / `Shift` / `Alt` / `Super`（macOS の Cmd）。
- 文字キーは大文字小文字を区別しない。
- 修飾キーは完全一致で判定する。
- 1アクションに複数キーを指定できる。`[]` で無効化できる。

### mouse.toml

```toml
wheel_threshold = 50.0
double_click_ms = 400

[bindings]
next = ["left", "wheel_down"]
prev = ["right", "wheel_up"]
save_page = ["double_left"]
```

- 既定の左シングルクリックは次のページ、右クリックは前のページ。
- 左ダブルクリックで表示中ページを保存する。
- ダブルクリックが割り当てられたボタンは、シングルクリック動作を `double_click_ms` だけ遅延させる。
- `wheel_threshold` はホイールで1ページ送りするために必要な移動量。
- ダブルクリックの判定距離は 10px。

## 3. ファイルとディレクトリ

設定ディレクトリ:

- macOS: `~/Library/Application Support/mangania/`
- Windows: `%APPDATA%\mangania\`
- Linux: `$XDG_CONFIG_HOME/mangania/`（未設定時は `~/.config/mangania/`）

| ファイル | 作成タイミング |
| --- | --- |
| `config.toml` | 設定を保存したとき |
| `keybindings.toml` | 初回起動時 |
| `mouse.toml` | 初回起動時 |

既定の保存先は `~/Pictures/MangaNia`。出力先がなければ作成する。

## 4. パフォーマンスとリソース

- 先読み枚数の既定は 2。
- キャッシュは `先読み枚数 + 2` ページ、最大 128MiB。
- 現在ページは常にキャッシュに残す。
- ZIP 読み出しとデコードは UI スレッドで行わない。
- ディスクキャッシュは使わない。
- GPU 描画は必須にしない。

## 5. 対応形式と制約

- 対応形式: JPEG（ベースライン / プログレッシブ）、PNG、WebP。
- 対象外: RAR、7z、JPEG 2000、JPEG XL、AVIF、GIF。
- 読み方向は右→左固定。
- 設定画面、閲覧位置の保存、表紙の単独表示は未実装。
- Windows 7 は実機未検証。Windows / Linux はコンパイル確認まで。

## 6. 開発の好み

- Rust 2024、単一 crate。
- 1ファイル・1モジュールの責務を小さく保つ。目安は 600 行以下。
- 純粋なロジックは UI から分離し、単体テストを書く。
- 不必要な crate は追加しない。追加時は理由を DECISIONS.md に記録する。
- コード内コメントは英語、ユーザー向け文書と UI 文言は日本語。
- 変更は Conventional Commits に従い、英語のコミットメッセージにする。
- 変更後は `cargo fmt`、`cargo test`、`cargo clippy --all-targets` を通す。
