# MaleicacidTvInput 統合手順

この文書は `tis/` の product 統合条件を固定する。Tuner HAL 側の統合手順は `tuner_hal2/INTEGRATION.md` を正とし、この文書には重複して記載しない。TIS の runtime 状態遷移、TvProvider 利用、Direct Boot の保留・再試行、ライブセッション優先順位、録画 API、視聴年齢制限、CAS 状態写像は `DESIGN_JA.md` を正とし、本書では再定義しない。

## product package

product makefile で次を継承する。

```make
$(call inherit-product, vendor/maleicacid/tv/tis/config/product_integration.mk)
```

`config/product_integration.mk` は次を `PRODUCT_PACKAGES` と `PRODUCT_COPY_FILES` に入れる正式ファイルである。

```make
PRODUCT_PACKAGES += \
    MaleicacidTvInput \
    privapp-permissions-maleicacid-tvinput \
    libmaleicacid_arib_si_engine_jni \
    libmaleicacid_arib_caption_jni

PRODUCT_COPY_FILES += \
    frameworks/native/data/etc/android.software.live_tv.xml:$(TARGET_COPY_OUT_PRODUCT)/etc/permissions/android.software.live_tv.xml
```

`MaleicacidTvInput` の `<uses-feature android:name="android.software.live_tv" />` は APK の要求であり、device feature 宣言の代替ではない。TIF 対応 product では上記 feature XML を product image へ配置する。

CAS HAL 仮実装は TIS 初回ビルド確認ゲートへ含めない。

## libaribcaption Soong / renderer 統合

ARIB字幕表示の product 統合では、repoで供給される `libaribcaption-android` の product fork を Soong graph に含め、renderer 有効の `libaribcaption.so` を生成する。`libmaleicacid_arib_caption_jni` はこの `libaribcaption` に明示依存し、`MaleicacidTvInput` は `libmaleicacid_arib_caption_jni` を JNI library として同梱する。

次は字幕対応宣言条件として認めない。

```text
- `dlopen()` で .so が開けることだけ
- decoder API を呼べることだけ
- Canvas 文字描画だけ
- renderer 無効 build
- provenance と build option が不明な out-of-graph .so
```

ビルド確認では `m libaribcaption libmaleicacid_arib_caption_jni MaleicacidTvInput` を確認対象に含める。実機確認では字幕 PES 入力から libaribcaption renderer 出力、TIS字幕 overlay 表示までを接続確認対象とする。

## 権限と priv-app

`MaleicacidTvInput` は product priv-app として組み込み、`privapp-permissions-maleicacid-tvinput` を同じ product image に入れる。

確認対象は次のとおりとする。

```text
/product/priv-app/MaleicacidTvInput/MaleicacidTvInput.apk
/product/etc/permissions/privapp-permissions-maleicacid-tvinput.xml
/product/etc/permissions/android.software.live_tv.xml
/product/priv-app/MaleicacidTvInput/lib/<abi>/libmaleicacid_arib_si_engine_jni.so
/product/priv-app/MaleicacidTvInput/lib/<abi>/libmaleicacid_arib_caption_jni.so
```

## 録画・予約の product 統合境界

現行 product 統合では `rec/` 配下の予約録画サービス、receiver、test module を product package または release確認条件へ入れない。TIS manifest metadata は `android:canRecord="false"` を維持する。`TvInputService` の録画 API に対する runtime 契約は `DESIGN_JA.md` を正とする。

`MaleicacidRecScopeTests` は録画・予約作業で明示指定して使う範囲に限定し、現行 product の build / atest / VTS / 実機確認 gate へ混ぜない。

## Direct Boot の product 統合条件

TIS は `directBootAware=true` を維持する。`AndroidManifest.xml` には `<uses-permission android:name="android.permission.RECEIVE_BOOT_COMPLETED" />` を宣言し、`BootReceiver` は `android:directBootAware="true"` とする。`BootReceiver` の intent filter は `ACTION_LOCKED_BOOT_COMPLETED` と `ACTION_BOOT_COMPLETED` を含める。`ACTION_USER_UNLOCKED` は manifest receiver の対象にしない。

`BootEpgSyncJobService` は `AndroidManifest.xml` に service として宣言し、`android.permission.BIND_JOB_SERVICE` で保護する。`BootReceiver`、`BootEpgSyncJobService`、`DirectBootEpgPending`、`BootEpgSyncCoordinator` の実行時役割、ジョブ登録・再試行、保留解除、開始条件、ライブセッションとの優先順位は `DESIGN_JA.md` を正とし、本書では状態遷移を再定義しない。

## flash 後の確認

```bash
adb shell pm list features | grep android.software.live_tv
adb shell ls /product/etc/permissions/android.software.live_tv.xml
adb shell ls /product/etc/permissions/privapp-permissions-maleicacid-tvinput.xml
adb shell dumpsys tv_input | grep -i Maleicacid
```

## TIS discovery 確認

システムTVアプリから設定画面を起動でき、setup 後に少なくとも1つの非スクランブル視聴可能チャンネルが `TvContract.Channels` に登録されることを確認する。runtime の channel 登録条件と Tuner SDK API 利用境界は `DESIGN_JA.md` を正とする。

## 視聴年齢制限 / CAS の product 統合確認

product のシステムTVアプリ / rating definitions に、`DESIGN_JA.md` と `../ARIB_SI_EPG_TvProvider投影方針.md` が使用する AOSP system-defined ISDB rating が存在することを確認する。視聴制限判定、`notifyContentBlocked()`、unblock identity、CAS unavailable reason、provider-data の意味論はそれぞれの設計正本を参照し、本書では再定義しない。

`MaleicacidTvInputAcceptanceTests` と実機確認では、設計正本に定義された視聴年齢制限、CAS 仮実装境界、TvProvider 投影が product integration 後も成立することを確認する。

## ビルド・試験確認ゲート

この章は、tv 直下に作業メモを置かずに TIS / ARIB SI / ARIB字幕 JNI の確認対象を固定するための統合手順である。

### Soong モジュールビルド

LineageOS ソースツリーのルートで次を実行する。

```bash
source build/envsetup.sh
breakfast virtio_x86_64_tv_grub
m nothing
m \
  libaribcaption \
  libmaleicacid_arib_si_engine_jni \
  libmaleicacid_arib_caption_jni \
  MaleicacidTvInput \
  privapp-permissions-maleicacid-tvinput
```

### 試験モジュール

```bash
m \
  maleicacid_arib_si_engine_rs_test \
  libmaleicacid_arib_caption_jni_test \
  MaleicacidTvInputAcceptanceTests

atest \
  maleicacid_arib_si_engine_rs_test \
  libmaleicacid_arib_caption_jni_test \
  MaleicacidTvInputAcceptanceTests
```

`maleicacid_arib_si_engine_rs_test` は `arib_si_engine_rs/src/lib.rs` を試験用 crate として使う。`libmaleicacid_arib_caption_jni_test` は `tis/arib_caption_jni/src/lib.rs` を試験用 crate として使う。`MaleicacidTvInputAcceptanceTests` は `tis/tests/src/**/*.kt` と `tis/tests/assets` を確認対象とする。

### 仕様カバレッジ

```text
- provider-data JSON v1、descriptor 診断、未対応 codec 試験データは maleicacid_arib_si_engine_rs_test と MaleicacidTvInputAcceptanceTests で確認する。
- TvProvider 標準列投影、字幕トラック、視聴年齢制限、CAS 仮実装境界、設定、scan、チャンネル登録は MaleicacidTvInputAcceptanceTests の対象とする。
- 録画・予約は現行 product の確認対象外とし、MaleicacidRecScopeTests は録画・予約作業で明示指定して使う。
```

### 実機投入後の確認

```bash
adb shell pm list features | grep android.software.live_tv
adb shell ls /product/etc/permissions/android.software.live_tv.xml
adb shell ls /product/etc/permissions/privapp-permissions-maleicacid-tvinput.xml
adb shell dumpsys tv_input | grep -i Maleicacid
```

合格条件:

```text
- システムTVアプリから設定画面を起動できる。
- setup 後に少なくとも1つの非スクランブル視聴可能チャンネルが TvContract.Channels に登録される。
- product image に必要な feature / permission / JNI library が配置される。
- android:canRecord="false" が product manifest metadata に反映される。
```
