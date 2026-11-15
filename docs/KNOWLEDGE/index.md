# Knowledge

調査・検証で得た知見を残す。症状、原因、回避策をセットで書き、後から同じ問題を踏まないようにする。

- 1項目につき1ファイルを作成する
- ファイル名は `NNN.md` の連番
- 新しい項目は既存のファイルをコピーして作成する

## Items

| ID | Title | Area | Updated |
| --- | --- | --- | --- |
| [001](001.md) | stable の通常ビルドは Windows 7 で起動しない | Windows 7 / Rust | 2026-11-15 |
| [002](002.md) | Win7 ターゲットでも `dirs` が `combase.dll` を要求する | Windows 7 / 依存関係 | 2026-11-15 |
| [003](003.md) | `dirs` 5 系は Win7 ターゲットのビルドで失敗する | Windows 7 / 依存関係 | 2026-11-15 |

## まとめ: Windows 7 向けビルド

- Windows のパス解決は環境変数ベースにする（[002](002.md)）
- CRT を静的リンクする

```sh
RUSTFLAGS="-C target-feature=+crt-static" \
  cargo +nightly build -Z build-std --target x86_64-win7-windows-msvc --release
```

静的 CRT により、`VCRUNTIME140.dll` と `api-ms-win-crt-*` への依存が消える。修正後の exe のインポート DLL は次の通り。

```text
user32.dll, kernel32.dll, gdi32.dll, comctl32.dll, shell32.dll,
advapi32.dll, imm32.dll, ole32.dll, dwmapi.dll, uxtheme.dll, ntdll.dll
```

`combase.dll`、`VCRUNTIME140.dll`、`api-ms-win-crt-*` は含まれない。
