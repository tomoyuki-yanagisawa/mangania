# MangaNia

## ナレッジの置き場所

調査メモ、ハマりどころ、未対応・改善項目はこのリポジトリに置かず、プロジェクト外の `~/Projects/knowledge/` で管理する。

- 一覧: `~/Projects/knowledge/index.md`
- MangaNia の項目: `~/Projects/knowledge/mangania/`

新しい知見や未対応項目を記録するときは、`~/Projects/knowledge/mangania/` に `NNN.md` を追加し、`~/Projects/knowledge/index.md` に1行追加する。

- ファイル名は `NNN.md` の連番
- 後から見つけた項目は 500 以降
- 症状、原因、対応をセットで書く
- 未対応項目は Status / Priority / Size / Area を書く

リポジトリ内のドキュメントは次の3つだけにする。

- [DESIGN.md](docs/DESIGN.md): 現在のアーキテクチャと動作
- [DECISIONS.md](docs/DECISIONS.md): 技術判断と理由
- [PREFERENCES.md](docs/PREFERENCES.md): 既定値・UX・開発の好み
