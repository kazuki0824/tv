# tuner_hal2 product integration

この文書は、`tuner_hal2` を Android TV 14 系 product image の既定 Tuner HAL service として組み込むためのSSOTである。

## 0. 固定方針

```text
- product default Tuner HAL service は tuner_hal2 だけとする。
- 旧 tuner_hal は参照用ソースとして repository に残すだけで、product image へ入れない。
- ITuner/default を登録する実体は android.hardware.tv.tuner-service.maleicacid2 だけとする。
- 旧 tuner_hal の product package、VINTF fragment、init rc、PRODUCT_PACKAGES、product integration を同一productで有効化しない。
- 旧 `tuner_hal/INTEGRATION.md` は legacy/reference 用であり、既定 product 統合手順のSSOTにはしない。
```

## 1. product makefile

製品の product makefile で次を継承する。

```make
$(call inherit-product, vendor/maleicacid/tv/tuner_hal2/config/product_integration.mk)
```

`config/product_integration.mk` は次だけを `PRODUCT_PACKAGES` に追加する。

```make
PRODUCT_PACKAGES += \
    android.hardware.tv.tuner-service.maleicacid2 \
    maleicacid_tuner_hal2_ueventd_rc
```

旧 `tuner_hal` の `maleicacid.tv.tuner_hal-service` は追加しない。

## 2. BoardConfig / sepolicy

BoardConfig 側で次を取り込む。

```make
include vendor/maleicacid/tv/tuner_hal2/config/BoardConfigVendorSePolicy.mk
```

`BoardConfigVendorSePolicy.mk` は `vendor/maleicacid/tv/tuner_hal2/sepolicy` だけを既定Tuner HAL用のvendor sepolicyとして追加する。

## 3. ueventd import

製品側の vendor ueventd rc から次を import する。

```rc
import /vendor/etc/ueventd.tuner_hal2.rc
```

`ueventd.tuner_hal2.rc` はDVB / px4 / dma_heap のdevice node permissionを設定する。

## 4. service登録

`tuner_hal2/Android.bp` の `rust_binary` は次を持つ。

```text
name: android.hardware.tv.tuner-service.maleicacid2
init_rc: tuner_hal2/init/android.hardware.tv.tuner-service.maleicacid2.rc
vintf_fragments: tuner_hal2/manifest/android.hardware.tv.tuner-service.maleicacid2.xml
```

init rc は `android.hardware.tv.tuner.ITuner/default` を登録する。VINTF fragmentも `ITuner/default` だけを宣言する。

## 5. 旧tuner_halの扱い

旧 `tuner_hal` は参照用ソースである。次をproductへ入れてはならない。

```text
- maleicacid.tv.tuner_hal-service
- tuner_hal/tuner-hal-service.rc
- tuner_hal/tuner-hal-service.xml
```

旧実装を手動でビルド・参照することは妨げないが、同一productで `ITuner/default` を二重登録してはならない。
## 6. VTS / product config policy

VTS / product config は `../tuner_hal/DESIGN_JA.md` の`製品スコープ / AOSP capability / VTS profile 境界`、`CapabilitySnapshot`、`ProductProfile`を正とする。現行 TS-only profile では monitor event feature を要求する構成にしない。

monitor event の API 戻り値、feature 宣言有無、別 profile へ切り替える条件は `../tuner_hal/DESIGN_JA.md` を正とし、本書では重複定義しない。

## 7. section filter runtime契約の参照

`TableInfo repeat=false`を含むsection filterの公開意味、first-instance解決、停止条件、`repeat=true`との使い分け、未知の全instance集合の終端をHALが推測しない契約は`../tuner_hal/DESIGN_JA.md`を正とする。複数table instanceのinstance別完成・更新・寿命は`../arib_si_engine_rs/DESIGN_JA.md`の「複数table instanceの完成・更新・寿命」、操作ごとの必要instance集合と完成時の明示`stop()`は`../tis/DESIGN_JA.md`の「複数table instance収集と停止」を正とする。

本書が所有するのはproduct統合だけであり、VINTF/init/package/VTS設定の配置によって上記runtime契約を変更または再定義してはならない。


## px4 probe prefix 変更時の同期対象（PR #28）

px4のdevice probe prefixを変更する場合は、実装側の`frontend_px4`と、`tuner_hal2/config/ueventd.tuner_hal2.rc`、`tuner_hal2/sepolicy/file_contexts`を同一変更で同期する。具体pathはintegration contractであり、公開AIDL契約の`DESIGN_JA.md`では再定義しない。
