# Maleicacid Tuner HAL legacy source

このディレクトリは旧 `tuner_hal` の参照用ソースである。

r50ee5以降、product default の Tuner HAL service は `tuner_hal2` だけとする。旧 `tuner_hal` は product package、VINTF manifest、init rc、PRODUCT_PACKAGES、product integration に含めない。旧product統合用のconfig、VINTF/init、sepolicy、profile断片、および旧 `tuner_hal/INTEGRATION.md` は同ディレクトリから削除済みである。product統合手順のSSOTは `tuner_hal2/INTEGRATION.md` だけである。

## 設計正本

Tuner HAL の設計方針は次を正本とする。

```text
tuner_hal/DESIGN_JA.md
tuner_hal/CODE_CONVENTION.md
```

変更履歴は `CHANGELOG.md` にだけ記録する。
