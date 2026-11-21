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

- 設計・文書責務・release規則は `開発規則.md` を正とし、本書では再定義しない。
- 全module共通の実装禁止事項と共通部品再利用規則は `GLOBAL_CODE_CONVENTION.md` を正とする。
- 完了判定、検索結果の扱い、未実施検証の記録は `タスク完了判定の実施方法.md` を正とする。


