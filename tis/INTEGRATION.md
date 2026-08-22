# MaleicacidTvInput 統合手順

この文書は `tis/` の product 統合条件を固定する。Tuner HAL 側の統合手順は `tuner_hal2/INTEGRATION.md` を正とし、この文書には重複して記載しない。TIS の runtime 状態遷移、TvProvider 利用、Direct Boot の保留・再試行、ライブセッション優先順位、録画 API、視聴年齢制限、CAS 状態写像は `DESIGN_JA.md` を正とし、本書では再定義しない。

## product package

product makefile で次を継承する。

```make
$(call inherit-product, vendor/maleicacid/tv/tis/config/product_integration.mk)
```

`config/product_integration.mk` は次を `PRODUCT_PACKAGES` と `PRODUCT_COPY_FILES` に入れる正式ファイルである。`PRODUCT_PACKAGES` への列挙はinstall partitionを決めるものではなく、各Soong moduleのpartition属性を正とする。

```make
PRODUCT_PACKAGES += \
    MaleicacidTvInput \
    AribContentRatings \
    privapp-permissions-maleicacid-tvinput \
    libmaleicacid_arib_si_engine_jni \
    libmaleicacid_arib_caption_jni

PRODUCT_COPY_FILES += \
    frameworks/native/data/etc/android.software.live_tv.xml:$(TARGET_COPY_OUT_PRODUCT)/etc/permissions/android.software.live_tv.xml
```

`MaleicacidTvInput` の `<uses-feature android:name="android.software.live_tv" />` は APK の要求であり、device feature 宣言の代替ではない。TIF 対応 product では上記 feature XML を product image へ配置する。

`AribContentRatings` は Android TIF 標準の `android.media.tv.action.QUERY_CONTENT_RATING_SYSTEMS` receiver と `android.media.tv.metadata.CONTENT_RATING_SYSTEMS` XMLだけを公開する独立product componentであり、privileged permissionやplatform-private APIを要求しない。`product_specific: true` の `/product/app` として組み込む。

CAS HAL 仮実装は TIS 初回ビルド確認ゲートへ含めない。

## Treble partition / platform API 統合

`MaleicacidTvInput` は、`DESIGN_JA.md` の MediaSync Framework-private final-output observation を同一platform sourceから型付き利用する platform-coupled component である。そのため `/product` へ配置せず、`system_ext_specific: true` かつ `platform_apis: true` の `/system_ext` priv-app として組み込む。reflection、hidden API allowlist回避、`/product` からのprivate API依存へ置き換えない。

`privapp-permissions-maleicacid-tvinput` は `MaleicacidTvInput` と同じ `/system_ext` に配置する。TIS専用の `libmaleicacid_arib_si_engine_jni` と `libmaleicacid_arib_caption_jni` も `system_ext_specific: true` とし、TISから `/product` 専用native moduleへ逆向き依存を作らない。TIS専用 `libaribcaption` variantを正式統合する場合も、TISのnative依存closureから利用可能なsystem/system_ext側variantとして閉じ、product-only private dependencyにしない。

一方、`AribContentRatings` と `android.software.live_tv.xml` のようなpublic/System APIだけで成立する独立product component / product feature宣言は `/product` に置く。Tuner HAL、VINTF、vendor init、vendor sepolicy、ueventd、CAS HALは `/vendor` の責務を維持し、TISのsystem_ext化を理由にsystem側へ移さない。board固有差分が存在しない現構成では `/odm` を追加しない。

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

`MaleicacidTvInput` は `/system_ext` priv-app として組み込み、`privapp-permissions-maleicacid-tvinput` を同じ system_ext image に入れる。

確認対象は次のとおりとする。

```text
/system_ext/priv-app/MaleicacidTvInput/MaleicacidTvInput.apk
/system_ext/etc/permissions/privapp-permissions-maleicacid-tvinput.xml
/product/app/AribContentRatings/AribContentRatings.apk
/product/etc/permissions/android.software.live_tv.xml
/system_ext/priv-app/MaleicacidTvInput/lib/<abi>/libmaleicacid_arib_si_engine_jni.so
/system_ext/priv-app/MaleicacidTvInput/lib/<abi>/libmaleicacid_arib_caption_jni.so
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
adb shell ls /product/app/AribContentRatings/AribContentRatings.apk
adb shell ls /system_ext/etc/permissions/privapp-permissions-maleicacid-tvinput.xml
adb shell ls /system_ext/priv-app/MaleicacidTvInput/MaleicacidTvInput.apk
adb shell dumpsys tv_input | grep -i Maleicacid
```

## TIS discovery 確認

システムTVアプリから設定画面を起動でき、setup 後に少なくとも1つの非スクランブル視聴可能チャンネルが `TvContract.Channels` に登録されることを確認する。runtime の channel 登録条件と Tuner SDK API 利用境界は `DESIGN_JA.md` を正とする。

## 視聴年齢制限 / CAS の product 統合確認

AOSP system-defined `com.android.tv / ISDB / ISDB_4..ISDB_20` に加え、`AribContentRatings` が `com.maleicacid.tv.ratings / ARIB_EXCEPTIONAL / BROADCASTER_DEFINED` をTIF標準rating-provider機構から公開していることを確認する。TISは明示的なARIB `0x12..0xFF` をこのexceptional ratingへ写像し、rating情報そのものが存在しない場合だけ `TvContentRating.UNRATED` を使用する。

System TV App本体は本repoの所有物ではないため、同等実装を本repoへ複製しない。製品platform統合ではSystem TV App側に、上記product固有exceptional ratingだけを対象とするpolicy patchを含めることを必須条件とする。parental controlsが有効でglobal policyが`NONE`以外の場合はこのratingをglobal blocked-rating集合へ反映する一方、PIN認証済みの現在コンテンツに対する `onUnblockContent()` 一時解除は維持する。第三者custom rating、CTS Verifierが提供するrating、他domain/ratingSystemのblock/unblock可否へこのpolicyを波及させない。

TIS自身はraw ARIB値から独自にAV blockを強制せず、現在コンテンツの `TvContentRating` を `TvInputManager.isRatingBlocked()` に渡した結果だけをpolicy判定として使用する。`notifyContentBlocked()` / `notifyContentAllowed()` とPIN解除のruntime意味論は `DESIGN_JA.md` を正とする。

`MaleicacidTvInputAcceptanceTests` と実機確認では、通常ISDB年齢rating、`0x12` / `0xFF` exceptional rating、rating情報欠落時のUNRATED、PINによるcurrent-content unblock、第三者custom rating非干渉、CAS 仮実装境界、TvProvider投影が product integration 後も成立することを確認する。

## ビルド・試験確認ゲート

この章は、tv 直下に作業メモを置かずに TIS / ARIB SI / ARIB字幕 JNI の確認対象を固定するための統合手順である。

### Soong モジュールビルド

LineageOS ソースツリーのルートで次を実行する。

```bash
source build/envsetup.sh
breakfast virtio_x86_64_tv_grub
m nothing
m \
  AribContentRatings \
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
adb shell ls /product/app/AribContentRatings/AribContentRatings.apk
adb shell ls /system_ext/etc/permissions/privapp-permissions-maleicacid-tvinput.xml
adb shell ls /system_ext/priv-app/MaleicacidTvInput/MaleicacidTvInput.apk
adb shell dumpsys tv_input | grep -i Maleicacid
```

合格条件:

```text
- システムTVアプリから設定画面を起動できる。
- setup 後に少なくとも1つの非スクランブル視聴可能チャンネルが TvContract.Channels に登録される。
- system_ext image にTIS本体・privapp allowlist・TIS専用JNIが配置され、product image にlive_tv feature XMLとAribContentRatingsが配置される。
- System TV AppのARIB exceptional policyが有効で、PINによる現在コンテンツ一時解除と第三者rating非干渉を維持する。
- android:canRecord="false" が product manifest metadata に反映される。
```
