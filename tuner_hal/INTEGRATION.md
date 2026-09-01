# Tuner HAL product integration / VTS設定 手順書

この文書は、`vendor/maleicacid/tv/tuner_hal` を Android TV 14 系 product image に組み込むための SSOT である。README_JA.md にはこの文書への導線だけを置き、product makefile、BoardConfig、ueventd、SELinux、VINTF/init、VTS設定 の詳細を重複記載しない。

## 0. 固定方針

```text
- 通常 vendor binary 統合を primary path とする。
- APEX 統合は template として残すが、同一 product で通常方式と同時に有効化しない。
- Tuner HAL instance は android.hardware.tv.tuner.ITuner/default に固定する。
- サービス 名は vendor.maleicacid-tuner-default に固定する。
- ueventd node pattern の SSOT は config/ueventd.tuner_hal.rc である。
- SELinux vendor policy の SSOT は sepolicy/ 配下である。
- VTS設定 の SSOT は config/tuner_vts_config_aidl_V2.xml と profiles/*.yaml + tools/render_vts_config.py である。
```

### 0.1 px4_drv direct-slot 前提の確認

px4 backend で BS `STREAM_ID` または `DEMOD_LOCK` current readback を使う product は、対象 kernel driver を `kazuki0824/px4_drv` `feat/android-ddk` commit `90d9c6506389ece3e47cced826326ccd1c6d22e8`（`Add PX4 demod status readbacks (#1)`）または、その契約を明示的に引き継いだ検証済みcommitへ固定する。BS legacy `slot >= 8` reject が無効で、`PTX_SET_CHANNEL.slot` に absolute TSID を渡せること、および `PTX_GET_LOCK_STATUS` がread-only current demod lock ABIとして存在することを事前確認する。確認対象は次である。

```text
- driver/ptx_chrdev.c の BS path で slot >= 8 reject が有効ではないこと
- PTX_SET_CHANNEL の slot が PTX_ISDB_S_SYSTEM で stream_id として set_stream_id() へ渡ること
- HAL 側で TSID -> relative slot 変換表を持たず、absolute TSID をそのまま slot に渡すこと
- `include/ptx_ioctl.h` に `PTX_GET_LOCK_STATUS _IOR(0x8d, 0x0c, __u32)` が存在し、driver実装がcurrent `ops->check_lock()`結果を返すこと
- HAL `device/src/px4/abi.rs` と backend `observe_signal_state()` が同ABIを使用し、過去のtune成功/CNRをcurrent lockへ代用しないこと
```

公開 `nns779/px4_drv` develop 相当など、BS `slot >= 8` reject が有効な driver では px4 BS absolute TSID 経路は使用不可である。その構成では px4 BS `STREAM_ID` 対応を product capability、VTS profile、integration note で 対応宣言 してはならない。

## 1. 通常 vendor binary 統合

### 1.1 product makefile

製品の product makefile で次を継承する。

```make
$(call inherit-product, vendor/maleicacid/tv/tuner_hal/config/product_integration.mk)
```

`config/product_integration.mk` は次を `PRODUCT_PACKAGES` に入れる正式ファイルである。

```make
PRODUCT_PACKAGES += \
    maleicacid.tv.tuner_hal-service \
    maleicacid_tuner_hal_vts_config_aidl_v2 \
    maleicacid_tuner_hal_ueventd_rc
```

`.example.mk` は補助例として残っていても正式手順では参照しない。

### 1.2 BoardConfig / SELinux

製品の BoardConfig 系ファイルで次を include する。

```make
include vendor/maleicacid/tv/tuner_hal/config/BoardConfigVendorSePolicy.mk
```

`config/BoardConfigVendorSePolicy.mk` は次を持つ正式ファイルである。

```make
BOARD_VENDOR_SEPOLICY_DIRS += vendor/maleicacid/tv/tuner_hal/sepolicy
```

product makefile に `BOARD_VENDOR_SEPOLICY_DIRS` を直接書かない。

SELinux policy は次を vendor policy build に入れる。

```text
sepolicy/file_contexts
sepolicy/device.te
sepolicy/tuner_hal.te
```

合格条件:

```text
- hal_tv_tuner_maleicacid domain が policy build に含まれる。
- HAL サービス binary file context が含まれる。
- DVB / px4 device node label が含まれる。
- /dev/dma_heap/system access が AV MediaEvent 共有ハンドル 用に許可される。
```

### 1.3 ueventd

`maleicacid_tuner_hal_ueventd_rc` package は次を vendor image に install する。

```text
/vendor/etc/ueventd.tuner_hal.rc
```

target product の vendor ueventd rc から、必ず次を import する。

```rc
import /vendor/etc/ueventd.tuner_hal.rc
```

copy/paste 例は次に置く。

```text
config/ueventd.vendor.import.example.rc
```

禁止事項:

```text
- config/ueventd.tuner_hal.rc と同じ node pattern を製品側の別 rc に重複定義しない。
- config/ueventd.vendor.direct.example.rc のような直接 node pattern 複製ファイルを使わない。
```

合格条件:

```text
- config/ueventd.tuner_hal.rc が唯一の node pattern 定義元である。
- /vendor/etc/ueventd.tuner_hal.rc が install 対象である。
- target vendor ueventd rc に import /vendor/etc/ueventd.tuner_hal.rc が入る。
```

### 1.4 VINTF / init rc

通常 vendor binary 統合では `maleicacid.tv.tuner_hal-service` module の Android.bp property が VINTF fragment と init rc を install する。

```bp
init_rc: ["tuner-hal-service.rc"]
vintf_fragments: ["tuner-hal-service.xml"]
```

製品の manifest、device manifest、device rc に同じ HAL 宣言や サービス 定義を重複して書かない。

合格条件:

```text
- VINTF instance が android.hardware.tv.tuner.ITuner/default である。
- init サービス 名が vendor.maleicacid-tuner-default である。
- 通常方式では /vendor/bin/hw/maleicacid.tv.tuner_hal-サービスを使う。
```

## 2. APEX 統合 template

APEX 統合は将来または別 product 用の template であり、primary path ではない。

APEX 方式を採る場合は、通常方式の `maleicacid.tv.tuner_hal-service` package を同じ product に入れない。APEX payload の サービス binary と APEX init rc だけを有効化する。

APEX template:

```text
apex_template/Android.bp.fragment
apex_template/apex_manifest.json
apex_template/file_contexts
apex_template/tuner-hal-サービス.apex.rc
```

APEX 方式でも次は vendor 側に残す。

```text
maleicacid_tuner_hal_vts_config_aidl_v2
maleicacid_tuner_hal_ueventd_rc
BOARD_VENDOR_SEPOLICY_DIRS include
```

理由: VTS設定 と ueventd node permission は APEX payload ではなく product/vendor image の統合対象である。

## 3. VTS設定

`config/tuner_vts_config_aidl_V2.xml` は汎用製品設定ではなく、VTS が実際に受信・filter・DVR 確認できる試験用プロファイルである。

固定方針:

```text
- CAS HAL 仮実装 のため descramble 前提 flow を含めない。
- HAL-generated 範囲スキャン / ブラインドスキャン は 対応宣言しない。
- frequency は explicit tune point に固定する。
- ISDB-T bandwidth は 6MHz または AUTO 相当だけを使う。
- ISDB-S modulation / coderate / symbolRate は現行 HAL validation と一致させる。
- BS は streamId / streamIdType を対象 TS と一致させる。
- CS110 は stream selector を指定しない。
```

### 3.1 YAML profile

profile 例:

```text
profiles/earth_pt1_isdbs_bs_lab.yaml
profiles/px4_isdbs_cs110_lab.yaml
profiles/earth_pt1_isdbt_lab.yaml
```

上記 `*_lab.yaml` は検査手順確認用の lab 用仮profile であり、合否判定に使う前に、対象実機・対象地点で取得した PAT/PMT 由来の service_id、PMT PID、video ES PID、audio ES PID、record PID へ更新する。

PID は必ず対象 サービスの PMT から決める。

```text
1. 対象 frontend / frequency へ tune する。
2. PAT を読む。
3. PAT から service_id -> PMT PID を得る。
4. PMT を読む。
5. video ES PID / audio ES PID を得る。
6. record PID を実TSに存在する PID から選ぶ。
```

根拠なく次を使ってはいけない。

```text
video_pid: 272
audio_pid: 273
record_pid: 272
```

### 3.2 XML 生成

```bash
python3 tuner_hal/tools/render_vts_config.py \
  --select earth_pt1_isdbs_bs_lab \
  tuner_hal/profiles/earth_pt1_isdbs_bs_lab.yaml \
  tuner_hal/profiles/px4_isdbs_cs110_lab.yaml \
  tuner_hal/profiles/earth_pt1_isdbt_lab.yaml \
  tuner_hal/config/tuner_vts_config_aidl_V2.xml
```

生成後に必ず差分確認する。

```bash
git diff -- tuner_hal/config/tuner_vts_config_aidl_V2.xml
```

### 3.3 VTS 前 sanity check

```text
1. frontend tune で LOCKEDコールバック が来る。
2. audio/video filter の指定 PID で DATA_READY または MediaEvent が来る。
3. record filter の指定 PID で TS packet が流れる。
4. IDvr.start() 後、DVR queue から data を読める。
5. AV filter は getAvSharedHandle() 後の MediaEvent offset/length で mmap data を読める。
```

## 4. 二重登録禁止

禁止:

```text
- 通常 vendor binary と APEX を同時に product package へ入れる。
- product/device manifest に tuner-hal-サービス.xml と同じ VINTF 宣言を重複追加する。
- device rc に tuner-hal-サービス.rc と同じ サービスを重複定義する。
- ueventd node pattern を config/ueventd.tuner_hal.rc 以外に重複定義する。
- BoardConfig と product makefile の両方へ sepolicy path を二重定義する。
```

## 5. 受け入れ条件

```text
- product makefile が config/product_integration.mk を継承する。
- BoardConfig 系 file が config/BoardConfigVendorSePolicy.mk を include する。
- target vendor ueventd rc が import /vendor/etc/ueventd.tuner_hal.rc を持つ。
- config/ueventd.tuner_hal.rc が唯一の device node pattern 定義元である。
- config/ueventd.vendor.direct.example.rc が存在しない。
- VINTF/init は Android.bp module property から install され、device 側に重複しない。
- VTS設定 は実TSの PMT 由来 PID と一致する。
- README_JA.md に integration 詳細が重複していない。
```

## 6. ビルド・試験確認ゲート

この章は、tv 直下に作業メモを追加しない形で Tuner HAL 単体の確認対象を固定するための統合手順である。Tuner HAL 単体の合否判定は、Tuner HAL モジュールビルド、Rust 試験、atest、Tuner VTS モジュール、実機簡易確認 の順で行う。full VTS / full CTS はこの章の合格条件に含めない。

### 6.1 Soong モジュールビルド

LineageOS ソースツリーのルートで次を実行する。

```bash
source build/envsetup.sh
breakfast virtio_x86_64_tv_grub
m nothing
m \
  libmaleicacid_tuner_hal_common \
  libmaleicacid_tuner_hal_frontend_dvb \
  libmaleicacid_tuner_hal_frontend_px4 \
  libmaleicacid_tuner_hal_dvr \
  libmaleicacid_tuner_hal_descrambler \
  libmaleicacid_tuner_hal_soft_demux \
  libmaleicacid_tuner_hal_fmq_shim \
  maleicacid.tv.tuner_hal-service \
  maleicacid_tuner_hal_vts_config_aidl_v2 \
  maleicacid_tuner_hal_ueventd_rc
```

### 6.2 Rust 試験モジュール

```bash
m \
  maleicacid_tuner_hal_frontend_dvb_test \
  maleicacid_tuner_hal_frontend_px4_test \
  maleicacid_tuner_hal_dvr_test \
  maleicacid_tuner_hal_soft_demux_test \
  maleicacid_tuner_hal_descrambler_test \
  maleicacid_tuner_hal_binder_service_test

atest \
  maleicacid_tuner_hal_frontend_dvb_test \
  maleicacid_tuner_hal_frontend_px4_test \
  maleicacid_tuner_hal_dvr_test \
  maleicacid_tuner_hal_soft_demux_test \
  maleicacid_tuner_hal_descrambler_test \
  maleicacid_tuner_hal_binder_service_test
```

`maleicacid_tuner_hal_binder_service_test` は `tuner_hal/binder_service/src/main.rs` と同ディレクトリの Rust source を試験用 crate として使う。`tests/` ディレクトリの有無ではなく、Android.bp の `rust_test` module と `#[cfg(test)]` を含む source を確認対象とする。

### 6.3 Tuner VTS モジュール

VTS 設定は、3章の手順で対象実機の PAT/PMT 由来 PID に更新してから使う。更新後に次を実行する。

```bash
m maleicacid_tuner_hal_vts_config_aidl_v2 vendorimage
m vts -j
out/host/linux-x86/vts/android-vts/tools/vts-tradefed
# vts-tradefed 内
run vts --module VtsHalTvTunerTargetTest
```

`VtsHalTvTunerTargetTest` より前に `run vts-hal` または `run vts` の結果を Tuner HAL 単体の合否判定に使ってはならない。

### 6.4 flash 後の確認

```bash
adb root
adb shell service list | grep -i tuner
adb shell ps -A | grep maleicacid
adb shell ls -l /dev/dvb /dev/px4video* /dev/px4stream* 2>/dev/null
adb shell dmesg | grep -i -E 'dvb|px4|tuner|maleicacid'
```

合格条件:

```text
- HAL サービスが android.hardware.tv.tuner.ITuner/default として見える。
- /dev/dvb または /dev/px4* の権限が config/ueventd.tuner_hal.rc と一致する。
- 対象 profile の frontend tune が LOCKED へ到達する。
- 指定 PID の filter / DVR 経路で data が取得できる。
```
