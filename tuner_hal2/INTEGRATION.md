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

VTS / product config は `../tuner_hal/DESIGN_JA.md` の`製品スコープ / AOSP capability / VTS profile 境界`、`CapabilitySnapshot`、`ProductProfile`を正とする。本製品は monitor event feature を製品能力として採用せず、静的VTS/product configでも同featureを要求・広告する構成にしない。

monitor event の公開API戻り値とcapability契約は `../tuner_hal/DESIGN_JA.md` を正とし、本書では重複定義しない。本書のproduct integration設定を、未定義の将来profileでmonitor eventを有効化するための切替点として扱ってはならない。

## 7. section filter runtime契約の参照

`TableInfo repeat=false`を含むsection filterの公開意味、first-instance解決、停止条件、`repeat=true`との使い分け、未知の全instance集合の終端をHALが推測しない契約は`../tuner_hal/DESIGN_JA.md`を正とする。複数table instanceのinstance別完成・更新・寿命は`../arib_si_engine_rs/DESIGN_JA.md`の「複数table instanceの完成・更新・寿命」、操作ごとの必要instance集合と完成時の明示`stop()`は`../tis/DESIGN_JA.md`の「複数table instance収集と停止」を正とする。

本書が所有するのはproduct統合だけであり、VINTF/init/package/VTS設定の配置によって上記runtime契約を変更または再定義してはならない。

## 8. px4 device probe path契約

px4系device nodeのprobe prefixは本節をproduct integration上のSSOTとする。対象prefixは次のとおりである。

```text
/dev/px4video
/dev/pxmlt5video
/dev/pxmlt8video
/dev/isdb6014video
/dev/isdb2056video
/dev/pxm1urvideo
/dev/pxs1urvideo
/dev/isdbt2071video
```

このprefix集合を変更する場合は、次を同一変更で同期する。

- `tuner_hal2`のpx4 frontend probe adapterは本節のprefix集合だけを参照してdevice node候補を構成する。実装owner/anchorは`DESIGN_JA.md`のfrontend/backend実装ownerに従い、本書では別の実装ownerを設けない。
- `tuner_hal2/config/ueventd.tuner_hal2.rc`は同じdevice node集合のpermission entryを持つ。
- `tuner_hal2/sepolicy/file_contexts`その他のSELinux path設定で同device nodeを列挙する場合は、本節のprefix集合と一致させる。

probe adapter、ueventd、SELinux側のいずれかだけに別prefixを追加してはならない。具体device pathの正本を実装helper名やPR履歴へ置かず、本節から一方向に同期する。

## 9. LineageOS 22.1 nullable Tuner AIDL current 統合

`tuner_hal/DESIGN_JA.md` の `nullable Binder 境界` が定義する公開意味論を Rust generated trait から到達可能にするため、本製品の LineageOS 22.1 / Android 15 統合では Tuner Stable AIDL の frozen V1/V2 を変更せず、unfrozen current（最新 frozen V2 に対する V3）へ nullable annotation を追加する。

platform checkout には、通常ビルドの前に次の patch を `hardware/interfaces` へ適用する。

```text
vendor/maleicacid/tv/tuner_hal2/platform_patches/lineage-22.1/android_hardware_tv_tuner_nullable_current.patch
```

Android build root から適用する例は次のとおりとする。

```bash
git -C hardware/interfaces apply \
  "$ANDROID_BUILD_TOP/vendor/maleicacid/tv/tuner_hal2/platform_patches/lineage-22.1/android_hardware_tv_tuner_nullable_current.patch"
```

patch は次だけを変更する。

- `IFilter.setDataSource()` の filter を `@nullable` とする。
- `IDescrambler.addPid()` / `removePid()` の optional source filter を `@nullable` とする。
- `IFrontend.setCallback()` / `ILnb.setCallback()` の callback を `@nullable` とする。
- 上記 source AIDL と一致する `aidl_api/android.hardware.tv.tuner/current/` snapshot を更新する。
- LineageOS 22.1 の FCM 202404 で Tuner AIDL V3 を許容するため、framework compatibility matrix の version range を `1-3` とする。

`aidl_api/android.hardware.tv.tuner/1` と `2` は変更しない。`android.hardware.tv.tuner-freeze-api` は実行せず、`versions_with_info` に V3 を追加しない。したがって nullable 版は current/unfrozen V3 のまま使用する。

source AIDL の nullable 契約を変更する場合は、LineageOS checkout 上で source AIDL を先に変更した後、次を実行して `current` snapshot を更新し、その生成差分をこの patch へ反映する。

```bash
m android.hardware.tv.tuner-update-api
```

通常の product build のたびに `update-api` を実行してはならず、`aidl_api/.../current` を手編集して source AIDL と独立に変更してはならない。

`tuner_hal2` は current V3 Rust binding を `android.hardware.tv.tuner-V3-rust` として参照し、VINTF fragment も Tuner version 3 を宣言する。採用 build configuration では `RELEASE_AIDL_USE_UNFROZEN=true` が実効値でなければならない。`false` の構成では最新 unfrozen APIを製品契約として使用できないため、この nullable V3 統合の完了buildとして扱わない。

この統合は LineageOS 22.1 / Android 15 checkout を前提とする。LineageOS 21.0 / Android 14 checkout は本節の V3 current、FCM、Rust generated trait 契約を満たさないため、この統合の入力として使用しない。
