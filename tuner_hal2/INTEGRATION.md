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

同じinit serviceは `maleicacid_cas_key_bridge` というmode `0660`、owner/group `media`のstream socketを生成し、Tuner HALへcontrol socket fdとして渡す。CAS HALは `/dev/socket/maleicacid_cas_key_bridge` へ接続し、session予約、key epoch publish、revokeだけをversioned bounded protocolで要求する。socket I/O中はTuner runtime lockを保持せず、decoded commandの適用中だけlockを取得する。CAS serviceとのSELinux接続許可は `../cas_hal/sepolicy` と同productで組み込む。

wire requestはbig-endianの20 byte header `MCKR | version=1 | operation | reserved[2] | request_id:u64 | payload_len:u32`を使う。operationはping=1、publish=2、revoke=3、reserve=4である。reserve/revoke payloadは `session_len:u8 | session_id | ca_system_id:i32 | session_generation:u64`、publishは同じsession identityに `key_epoch:u64 | system_key[32] | cbc_initial_value[8] | even_ks[8] | odd_ks[8]`を持つ。responseは16 byteの `MCKS | version | typed_status | reserved[2] | request_id:u64`、最大request frameは160 byte、I/O deadlineは2秒とする。session IDは1〜16 byteで `[0x00]`を拒否し、予約なしpublish、identity不一致、非単調epochを成功へ丸めない。

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
