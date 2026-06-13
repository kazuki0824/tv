# ueventd / APEX 統合後続仕様

## 目的

この文書は、Tuner HAL を APEX install へ移行する場合に後続で固定する事項だけを管理する。通常 vendor install における ueventd import、SELinux、VINTF/init、product makefile、二重登録禁止の現行統合手順は `tuner_hal2/INTEGRATION.md` を正とする。

## 後続で固定する項目

- APEX install を正式経路にする場合の sepolicy 配置。
- APEX 内サービス rc と vendor ueventd rc の責務分担。
- VINTF manifest、file_contexts、サービス domain の整合条件。
- product ごとの ueventd import 位置と検証手順。
- 通常 vendor install から APEX install へ移行する場合の片系化手順。

## 禁止事項

- device node permission を実機確認なしで成功扱いにすること。
- 通常 vendor install と APEX install の両方を同時に primary path として対応宣言すること。
- ueventd import 不足を HAL 側の retry だけで隠すこと。
- `tuner_hal2/INTEGRATION.md` の通常 vendor install 手順を壊す形で APEX 後続仕様を固定すること。
