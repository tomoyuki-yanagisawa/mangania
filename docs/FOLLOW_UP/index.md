# Follow-up index

未実装の機能、改善、調査項目を管理します。1項目につき1ファイルを作成し、この index から辿れるようにします。

- テンプレート: [000.template.md](000.template.md)
- ファイル名: `NNN.md`（例: `500.md`）
- 番号は 001〜499 を初期項目用に確保し、後から見つけた項目は 500 以降を使う。
- 新規項目はテンプレートをコピーして作成する。

関連する調査結果は [KNOWLEDGE](../KNOWLEDGE/index.md) に残します。

## Status

| Status | 意味 |
| --- | --- |
| proposed | 起票しただけ |
| triaged | 内容・優先度・サイズを確認済み |
| ready | 実装可能な状態 |
| in-progress | 実装中 |
| blocked | 依存・判断待ち |
| done | 完了 |
| wontfix | 対応しない |

## Priority

| Priority | 意味 |
| --- | --- |
| P0 | すぐ対応が必要（破損・重大な不具合・リリース blocker） |
| P1 | 次に対応したい |
| P2 | 余裕があれば対応 |
| P3 | アイデア・将来候補 |

## Items

| ID | Title | Status | Priority | Size | Area |
| --- | --- | --- | --- | --- | --- |
| [500](500.md) | dirs に依存しないクロスプラットフォームな設定パス解決を考える | proposed | P2 | M | config |
| [501](501.md) | Windows の全画面表示でメニューバーを隠す | proposed | P2 | S | Windows / UI |
| [502](502.md) | Windows で exe 起動時の黒いコンソールを止める | proposed | P1 | M | Windows / build |
| [503](503.md) | archive.rs から Windows ショートカット解決を分離する | proposed | P2 | S | refactor |

<!-- triage した項目をここに追加する -->
