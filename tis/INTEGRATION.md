# MaleicacidTvInput 統合手順

この文書は `tis/` の product 統合条件を固定する。Tuner HAL 側の統合手順は `tuner_hal/INTEGRATION.md` を正とし、この文書には重複して記載しない。

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

CAS HAL 仮実装 は r51 の TIS 初回 ビルド確認ゲート へ含めない。


## libaribcaption Soong / renderer 統合

ARIB字幕表示の product 統合では、repoで供給される `libaribcaption-android` の product fork を Soong graph に含め、renderer 有効の `libaribcaption.so` を生成する。`libmaleicacid_arib_caption_jni` はこの `libaribcaption` に明示依存し、`MaleicacidTvInput` は `libmaleicacid_arib_caption_jni` を JNI library として同梱する。

次は r51 字幕対応宣言条件として認めない。

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


## 録画・予約の除外

r51 の product 統合では `rec/` 配下の予約録画 サービス / receiver / test module を product package または release確認条件へ入れない。TIS メタデータは `android:canRecord="false"` を維持し、`onCreateRecordingSession()` は `null` を返す状態を r51 の正とする。

`MaleicacidRecScopeTests` は r53 の録画・予約作業で明示指定して使う範囲に限定し、r51 の build / atest / VTS / 実機確認 gate へ混ぜない。

## Direct Boot と boot receiver

TIS は `directBootAware=true` を維持する。`LOCKED_BOOT_COMPLETED` では device protected storage に pending flag だけを記録し、TvProvider、Tuner、JNI parser は user unlock 後にだけ起動する。

`ACTION_USER_UNLOCKED` は manifest receiver へ登録しない。Boot EPG sync / background maintenance は BootReceiver、UserUnlockReceiver、または明示的な maintenance scheduler からのみ起動する。`MaleicacidTvInputService.onCreate()` は Direct Boot pending drain、boot EPG sync、background maintenance を開始してはならない。

Boot EPG sync / background maintenance の開始条件は、active ライブセッション、session creation in progress、setup scan、playback pipeline、scan manager running がすべて存在しないこととする。ライブセッション 作成要求が来た時点で boot/background task が未開始なら defer する。boot/background task が既に running の場合、r51 では boot/background task を cancel/defer し ライブ tune を優先する。

## flash 後の確認

```bash
adb shell pm list features | grep android.software.live_tv
adb shell ls /product/etc/permissions/android.software.live_tv.xml
adb shell ls /product/etc/permissions/privapp-permissions-maleicacid-tvinput.xml
adb shell dumpsys tv_input | grep -i Maleicacid
```

## TIS discovery 確認

システムTVアプリから設定画面 を起動でき、setup 後に少なくとも 1 つの 非スクランブル視聴可能チャンネルが `TvContract.Channels` に登録されることを確認する。TIS は Tuner HAL binder を直接呼ばず、Tuner SDK API 経由で Tuner HAL にアクセスする。


## 視聴年齢制限 / CAS 代替処理 統合確認

- product の システムTVアプリ / レーティング definitions に `domain=com.android.tv`, `ratingSystem=ISDB`, `rating=ISDB_4..ISDB_20` が存在することを確認する。
- `TvProvider.Programs.COLUMN_CONTENT_RATING` に `com.android.tv/ISDB/ISDB_<age>` 相当の `TvContentRating.flattenToString()` が入ることを確認する。
- `Programs.COLUMN_INTERNAL_PROVIDER_DATA` に CAS 状態、`publishStateSource`、raw 視聴年齢制限 診断JSONが残ることを確認する。
- parental controls enabled + blocked レーティング で `notifyContentBlocked()` が発生し、parental block を理由に `notifyVideoUnavailable()` を呼ばずに AV再生が停止または開始抑止されることを確認する。
- `onUnblockContent()` 後は同一 `channelUri + serviceKey + eventId + ratingString` の 現在番組 / レーティングに限って playback retry が許可されることを確認する。start/end は現在表示中の Program row 照合用の補助条件であり、stable identity や provider-data `programKey` の構成要素ではないことを確認する。
- scrambled unsupported サービスは parental allowed でも playback success にせず、`notifyVideoUnavailable(TvInputManager.VIDEO_UNAVAILABLE_REASON_CAS_UNKNOWN)` を使うことを確認する。


## r51 ビルド・試験確認ゲート

この章は、tv 直下に作業メモを置かずに TIS / ARIB SI / ARIB字幕 JNI の r51 確認対象を固定するための統合手順である。

### Soong モジュールビルド

AOSP root で次を実行する。

```bash
source build/envsetup.sh
lunch <your_android_tv_14_product>-userdebug
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

### r51 仕様カバレッジ

```text
- provider-data JSON v1、descriptor 診断、未対応 codec 試験データは maleicacid_arib_si_engine_rs_test と MaleicacidTvInputAcceptanceTests で確認する。
- TvProvider 標準列投影、字幕トラック、視聴年齢制限、CAS 仮実装 境界、設定、scan、チャンネル登録 は MaleicacidTvInputAcceptanceTests の対象とする。
- 録画・予約は r51 の確認対象外とし、MaleicacidRecScopeTests は r53 で明示指定して使う。
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
- システムTVアプリから設定画面 を起動できる。
- setup 後に少なくとも 1 つの 非スクランブル視聴可能チャンネルが TvContract.Channels に登録される。
- TIS は Tuner HAL binder を直接呼ばず、Tuner SDK API 経由で Tuner HAL にアクセスする。
- android:canRecord="false" を維持する。
```
