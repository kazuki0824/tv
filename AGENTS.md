# AGENTS.md

## 目的

このファイルは、`vendor/maleicacid/tv` 配下を編集する作業エージェント向けの入口である。

設計判断、状態遷移、戻り値、資源寿命、失敗時処理、完了判定、統合手順、変更履歴はこのファイルに定義しない。

## 作業開始位置

Codex は、作業者が次の位置で起動する前提とする。

```bash
cd <LINEAGE_ROOT>/vendor/maleicacid/tv
codex
```

`<LINEAGE_ROOT>` は、`repo init` / `repo sync` を実行した、`.repo` ディレクトリを含む LineageOS ソースツリーのルートである。

## build / test 初期化

Android/Soong build、rustfmt、atest、VTS を実行する場合は、LineageOS ソースツリーのルートで target 初期化を行う。

```bash
cd <LINEAGE_ROOT>
source build/envsetup.sh
breakfast virtio_x86_64_tv_grub
```

この project の target 初期化では、`lunch <your_android_tv_14_product>-userdebug` ではなく、`breakfast virtio_x86_64_tv_grub` を使う。

## 作業前に読む文書

1. `開発規則.md`
2. `タスク完了判定の実施方法.md`
3. `GLOBAL_CODE_CONVENTION.md`
4. 変更対象モジュールの `DESIGN_JA.md`
5. 変更対象モジュールの `CODE_CONVENTION.md`
6. build、atest、VTS、product統合を扱う場合は、変更対象モジュールの `INTEGRATION.md`
7. TvProvider投影を扱う場合は、`ARIB_SI_EPG_TvProvider投影方針.md`

## 最低禁止事項

- 正本ではない文書に、設計判断、完了条件、統合手順を重複定義しない。
- `future_work` を現行仕様、実装済み範囲、完了判定の根拠として扱わない。
- grep、rg、正規表現抽出、見出し抽出だけで全文精読または完了確認をしたと書かない。
- build、unit test、atest、VTS、実機確認を実施していない場合は、未実施と明記する。
- 完了条件を満たさない場合は No と書く。
- 共通部品の設計を変更する場合は、承認を得るまで実施してはならない。
- 共通部品の設計を変更する場合は、その場で必ず実装を追従する。
- DESIGN_JA.mdに共通部品化している処理を、あえてその共通部品を使用せずに手書き実装したり、機能の似た劣化コピーの部品を勝手に作ってそれを使用して実装してはならない。


## Git操作
build/Rust unit testが通った場合は、その時点でgit commitをするようにお願いします。
