# maleicacid Android TV components
[![Ask DeepWiki](https://deepwiki.com/badge.svg)](https://deepwiki.com/kazuki0824/tv)

このディレクトリは、日本向け Android TV 14 系の製品ツリーに統合する Tuner HAL、TIS、ARIB SI engine、CAS HAL プレースホルダー、録画関連候補を保持する。

この README は人間向けの入口であり、設計判断、完了判定、統合手順、変更履歴の正本ではない。

## 最初に読む文書

- リリース物・文書配置・責務分担: `開発規則.md`
- 完了判定方法: `タスク完了判定の実施方法.md`
- 横断実装規約: `GLOBAL_CODE_CONVENTION.md`
- TvProvider 投影方針: `ARIB_SI_EPG_TvProvider投影方針.md`

## モジュール

- Tuner HAL: `tuner_hal/README_JA.md`
- Tuner HAL product default 構成: `tuner_hal2/README_JA.md`
- TIS: `tis/README_JA.md`
- ARIB SI engine: `arib_si_engine_rs/README_JA.md`
- CAS HAL プレースホルダー: `cas_hal/README_JA.md`
- 録画関連候補: `rec/README_JA.md`

## Codex を使う場合

`<LINEAGE_ROOT>` は、`repo init` / `repo sync` を実行した、`.repo` ディレクトリを含む LineageOS ソースツリーのルートである。

Codex を使う作業者は、次の位置で起動する。

```bash
cd <LINEAGE_ROOT>/vendor/maleicacid/tv
codex
```

作業エージェント向け入口は `AGENTS.md` とする。

build、rustfmt、atest、VTS を行う場合の target 初期化は `AGENTS.md` および各モジュールの `INTEGRATION.md` に従う。

## 注意

現行仕様、状態遷移、戻り値、資源寿命、失敗時処理は各モジュールの `DESIGN_JA.md` を正とする。

product 統合、build、atest、VTS、実機確認の手順は各モジュールの `INTEGRATION.md` を正とする。

`future_work` 配下の文書は、現行仕様の正本ではない。
