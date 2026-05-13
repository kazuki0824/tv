# ueventd / APEX 統合後続仕様

## 目的

この文書は、Tuner HAL を通常 vendor install または APEX install へ統合する場合の ueventd と SELinux 境界を、現行仕様として整理する。

## 現行の固定事項

- 通常 vendor install では、product 側 vendor ueventd rc から `/vendor/etc/ueventd.tuner_hal.rc` を明示的に import する。
- 任意名の ueventd rc が自動 import される前提にしない。
- DVB / px4 device node の group / mode は、HAL service が device node を開けることを実機で確認する。
- APEX install を正式 primary path にするかどうかは未固定であり、通常 vendor install の統合手順を壊してはならない。

## 後続で固定する項目

- APEX install を正式経路にする場合の sepolicy 配置。
- APEX 内 service rc と vendor ueventd rc の責務分担。
- VINTF manifest、file_contexts、service domain の整合条件。
- product ごとの ueventd import 位置と検証手順。

## 禁止事項

- device node permission を実機確認なしで成功扱いにすること。
- 通常 vendor install と APEX install の両方を同時に primary path として claim すること。
- ueventd import 不足を HAL 側の retry だけで隠すこと。
