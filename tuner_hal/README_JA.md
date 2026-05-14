# Maleicacid Tuner HAL

この README は Tuner HAL の概要と統合手順への導線だけを持つ。

## 対象

- Android TV 14 系の `android.hardware.tv.tuner.ITuner/default`
- r51 target driver: `kazuki0824/px4_drv` `feat/android-ddk` branch、および Linux `earth_pt1`
- 対象放送: 日本向け ISDB-T / ISDB-S
- CAS HAL は 仮実装。descramble 前提の VTS / 視聴 flow は対象外。

## 統合手順の SSOT

product makefile、BoardConfig、ueventd import、SELinux、VINTF/init、VTS設定、通常 vendor binary 統合と APEX 統合の二重登録禁止は、すべて次を SSOT とする。

```text
tuner_hal/INTEGRATION.md
```

README に統合手順を重複記載しない。第三者が製品へ組み込む場合は `INTEGRATION.md` の手順だけを読む。

## 実装方針の SSOT

Tuner HAL の設計方針は次を SSOT とする。

```text
tuner_hal/DESIGN_JA.md
tuner_hal/CODE_CONVENTION.md
```

変更履歴は `CHANGELOG.md` にだけ記録する。
