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

`MaleicacidTvInput` は、`DESIGN_JA.md` の MediaSync Framework-private final-output observation を正規製品で利用する platform-coupled component である。そのため `/product` へ配置せず、`system_ext_specific: true` かつ `platform_apis: true` の `/system_ext` priv-app として組み込む。MediaSyncの追加private listenerだけはstock LineageOSでも同一TISをbuild/runできるようruntime reflectionで解決して呼び出し、private listener経路が呼び出し可能な場合はExact modeを使う。API不存在、reflection解決失敗、登録setter呼出し失敗などを含めprivate listener経路を呼び出せない場合は、公開`MediaCodec.OnFrameRenderedListener`を型付きで使うCompatibility modeへfallbackする。その他のplatform APIをreflectionやHAL binder直呼びへ一般化してはならない。

`privapp-permissions-maleicacid-tvinput` は `MaleicacidTvInput` と同じ `/system_ext` に配置する。TIS専用の `libmaleicacid_arib_si_engine_jni` と `libmaleicacid_arib_caption_jni` も `system_ext_specific: true` とし、TISから `/product` 専用native moduleへ逆向き依存を作らない。TIS専用 `libaribcaption` variantを正式統合する場合も、TISのnative依存closureから利用可能なsystem/system_ext側variantとして閉じ、product-only private dependencyにしない。

一方、`AribContentRatings` と `android.software.live_tv.xml` のようなpublic/System APIだけで成立する独立product component / product feature宣言は `/product` に置く。Tuner HAL、VINTF、vendor init、vendor sepolicy、ueventd、CAS HALは `/vendor` の責務を維持し、TISのsystem_ext化を理由にsystem側へ移さない。board固有差分が存在しない現構成では `/odm` を追加しない。

## libaribcaption Soong / renderer 統合

ARIB字幕表示のproduct統合では、repoで供給される `libaribcaption-android` の製品forkをSoong graphに含め、renderer有効の `cc_library_static { name: "libaribcaption" }` を正式経路とする。製品repo manifestの `revision` はbranchではなく検証済みcommitへ固定し、repo syncのたびにsource list・C API・renderer構成が暗黙変更される状態をrelease構成として認めない。`libaribcaption` はcore側variantを使用し、`system_ext_specific: true` の `libmaleicacid_arib_caption_jni` からSoongの静的native dependencyとして直接linkする。`libaribcaption` のFreeType依存は `libft2.nodep` を使用し、TISのsystem/system_ext dependency closure内で解決する。

正式な依存関係は次とする。

```text
/system_ext/priv-app/MaleicacidTvInput
  └─ libmaleicacid_arib_caption_jni.so
       └─ Soong static dependency: libaribcaption
            └─ static dependency: libft2.nodep
```

Rust JNIはlibaribcaption C APIを `extern "C"` で直接参照し、`libdl`、`dlopen()`、`dlsym()`、`dlclose()`を正式経路から除去する。Soongの静的依存指定は対象treeでRust `rust_ffi_shared` からC/C++ static moduleを正しく最終linkできる形を選ぶ。`static_libs` と `whole_static_libs` のどちらを使うかはlink graphとdead-strip要件に従う実装詳細とし、不要な全archive強制取り込みを設計目的にしない。ただしlibaribcaption C APIとtransitiveな `libft2.nodep` 依存が最終JNI `.so` で全て解決されることをbuild gateで確認する。

`libaribcaption.so` は生成・配置・APK同梱を要求しない。したがって `libaribcaption.so` の存在、`libmaleicacid_arib_caption_jni.so` の `DT_NEEDED` に `libaribcaption.so` が出ること、`MaleicacidTvInput` の `jni_libs` に `libaribcaption` を追加すること、`libaribcaption.so` のexport symbolをreadelfで確認することを完了条件にしてはならない。`MaleicacidTvInput` はJNI libraryとして `libmaleicacid_arib_caption_jni` のみを取り込み、libaribcaption/FreeTypeのcodeは最終JNI `.so` へ静的に閉じる。

次は字幕対応宣言条件として認めない。

```text
- runtime `dlopen()` でlibaribcaptionを探索できることだけ
- decoder APIを呼べることだけ
- Canvas文字描画だけ
- renderer無効build
- provenanceとbuild optionが不明なout-of-graph prebuilt
```

build確認では次を必須とする。

```text
- m libaribcaption が通る。
- m libmaleicacid_arib_caption_jni が通る。
- m MaleicacidTvInput が通る。
- libmaleicacid_arib_caption_jni.so のundefined native symbolに未解決の aribcc_* が残らない。
- libmaleicacid_arib_caption_jni.so に libaribcaption.so への DT_NEEDED が存在しない。
- runtime経路に dlopen/dlsym が残らない。
- renderer C APIを実際に呼ぶ。
```

実機確認では字幕PES入力からlibaribcaption decoder/renderer、RGBA8888出力、TIS字幕overlay表示までを接続確認対象とする。renderer viewport、PTS/NoPTS、scheduler、decoder/renderer lifecycleのruntime意味論は `DESIGN_JA.md` を正とし、本書で独立に再定義しない。

libaribcaption rendererの設計正本は本節と`DESIGN_JA.md`に集約済みであり、旧shared-library前提の別future_work文書を維持しない。残る作業は本節のbuild gateと実機の字幕PES→decoder→renderer→RGBA8888→overlay確認で管理し、設計済み内容をfuture_workへ重複定義しない。

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

TIS は `directBootAware=true` を維持する。`AndroidManifest.xml` には `<uses-permission android:name="android.permission.RECEIVE_BOOT_COMPLETED" />` を宣言し、`EpgBootSyncReceiver` は `android:directBootAware="true"` とする。`EpgBootSyncReceiver` の intent filter は `ACTION_LOCKED_BOOT_COMPLETED` と `ACTION_BOOT_COMPLETED` を含める。`ACTION_USER_UNLOCKED` は manifest receiver の対象にしない。

`BootEpgSyncJobService` は `AndroidManifest.xml` に service として宣言し、`android.permission.BIND_JOB_SERVICE` で保護する。`EpgBootSyncReceiver`、`BootEpgSyncJobService`、`DirectBootGuard`、`BootEpgSyncScheduler` の実行時役割、ジョブ登録・再試行、保留解除、開始条件、ライブセッションとの優先順位は `DESIGN_JA.md` を正とし、本書では状態遷移を再定義しない。

## ARIB exceptional ratingのLive TV App標準extension統合

JPN parental rating raw `0x12..0xFF` はTISで年齢値へ推測変換せず、`com.maleicacid.tv.ratings / ARIB_EXCEPTIONAL / BROADCASTER_DEFINED`へ写像する。rating定義とTV Appへの発見経路は独立`AribContentRatings` APKがTIF標準providerとして所有し、blocked-rating policyのownerはLive TV Appとする。System TV App本体へ直接patchを当てる方式は採用しない。

Android 15 / LineageOS 22.1系の既存`ContentRatingLevelPolicy`は、TIF rating-provider XMLの`contentAgeHint`をpreset policyの入力として使う。`HIGH`は6以上、`MEDIUM`は12以上、`LOW`はrating system内の最大age hint以上をblocked候補へ含め、`NONE`は空集合にする。単一ratingである`BROADCASTER_DEFINED`は`contentAgeHint=12`で公開し、stock policyでは`HIGH/MEDIUM/LOW`の各presetでblocked候補になる。12はproduct preset policy分類用metadataであり、ARIB raw `0x12..0xFF`の年齢解釈ではない。`CUSTOM`ではstock TV Appの通常rating設定から同canonical ratingを追加・削除する。第二policy APK、TV App private state reader、`packages/apps/TV` source patchは追加しない。

製品buildでは`AribContentRatings`とstock `LiveTv`を組み込み、rating-provider receiverとXML metadataを発見可能にする。product treeではplatform-signed `AribContentRatingsTvAppIntegrationTests`を`LiveTv`へinstrumentし、canonical ratingについて`TvInputManager.addBlockedRating()` / `removeBlockedRating()` / `isRatingBlocked()`が同一authorityで動作し、試験後にblocked状態を復元できることを確認する。

```text
m AribContentRatings AribContentRatingsTvAppIntegrationTests
atest AribContentRatingsTvAppIntegrationTests
```

TISは引き続き`TvInputManager.isRatingBlocked()`だけをcurrent policy authorityとして扱う。既存のblocked-rating永続化、PIN認証後のsession-level `onUnblockContent()`、通常年齢rating、第三者custom ratingの扱いは変更しない。
## MediaSync Exact-mode platform統合

この節をLineageOS 22.1向けMediaSync platform patchの適用手順と確認項目の正本とし、patch配下へ別のREADMEや重複手順書を置かない。

TISは追加private APIを静的参照しない。private listener経路を呼び出せるplatformではExact modeを使用し、API不存在、reflection解決失敗、登録setter呼出し失敗などにより呼び出せない場合は公開`MediaCodec.OnFrameRenderedListener`を使うCompatibility modeで動作する。正規製品で`DESIGN_JA.md`のfinal-output成功意味論を満たす場合は、次の既存2patchをLineageOS 22.1 platform treeへ適用してprivate listener経路を提供する。patch本文はTIS側runtime変更とは独立した再現可能なplatform統合差分として維持する。

```text
tis/platform_patches/lineage-22.1/frameworks_av_mediasync_first_output.patch
tis/platform_patches/lineage-22.1/frameworks_base_mediasync_first_output.patch
```

Android build rootから、まず適用可能性を確認してから適用する。

```bash
git -C frameworks/av apply --check \
  "$ANDROID_BUILD_TOP/vendor/maleicacid/tv/tis/platform_patches/lineage-22.1/frameworks_av_mediasync_first_output.patch"
git -C frameworks/base apply --check \
  "$ANDROID_BUILD_TOP/vendor/maleicacid/tv/tis/platform_patches/lineage-22.1/frameworks_base_mediasync_first_output.patch"

git -C frameworks/av apply \
  "$ANDROID_BUILD_TOP/vendor/maleicacid/tv/tis/platform_patches/lineage-22.1/frameworks_av_mediasync_first_output.patch"
git -C frameworks/base apply \
  "$ANDROID_BUILD_TOP/vendor/maleicacid/tv/tis/platform_patches/lineage-22.1/frameworks_base_mediasync_first_output.patch"
```

対象baselineはLineageOS 22.1の次のplatform sourceである。

```text
frameworks/av:
  media/libstagefright/include/media/stagefright/MediaSync.h
  media/libstagefright/MediaSync.cpp

frameworks/base:
  media/java/android/media/MediaSync.java
  media/jni/android_media_MediaSync.h
  media/jni/android_media_MediaSync.cpp
```

追加callbackのlate-drop、`attachBuffer()` / `queueBuffer()`、one-shot arm、`armSequence`、mutex外配送などのruntime意味論は`DESIGN_JA.md`を正とし、本書では重複定義しない。public SDK、`@SystemApi`、`@TestApi`、Tuner AIDL/VINTFをこの統合のために変更しない。

patch適用後は少なくとも次をtarget buildする。対象treeのmodule分割でmodule名が異なる場合は、`frameworks/base`のmedia JNI / framework Java、`frameworks/av`のMediaSync、TISを実際に再コンパイルする同等Soong targetを使用する。

```bash
m framework-minus-apex
m libstagefright
m MaleicacidTvInput
```

実機ではExact modeが選択されること、late-dropではavailabilityが成立しないこと、current final outputへのqueue成功で一回だけ通知されること、re-arm後の旧sequence eventが棄却されることを確認する。TIS host CIはstock APIでの静的compileとrepository内契約を確認するものであり、このplatform patchのJava/JNI/native型接続やnative実行時意味論を代替しない。未パッチplatformのCompatibility modeだけをもって正規製品のfinal-output意味論を確認済みとは扱わない。これらは未決設計ではなく製品統合・検証gateなので`future_work/r53`へ重複配置しない。

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
