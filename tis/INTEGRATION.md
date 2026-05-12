# MaleicacidTvInput 統合手順

この文書は `tis/` の product 統合条件を固定する。Tuner HAL 側の統合手順は `tuner_hal/INTEGRATION.md` を正とし、この文書には重複して記載しない。

## product package

product makefile に次を追加する。

```make
PRODUCT_PACKAGES += \
    MaleicacidTvInput \
    privapp-permissions-maleicacid-tvinput \
    libmaleicacid_arib_si_engine_jni

PRODUCT_COPY_FILES += \
    frameworks/native/data/etc/android.software.live_tv.xml:$(TARGET_COPY_OUT_PRODUCT)/etc/permissions/android.software.live_tv.xml
```

`MaleicacidTvInput` の `<uses-feature android:name="android.software.live_tv" />` は APK の要求であり、device feature 宣言の代替ではない。TIF 対応 product では上記 feature XML を product image へ配置する。

CAS HAL placeholder は r51 の TIS 初回 build gate へ含めない。

## 権限と priv-app

`MaleicacidTvInput` は product priv-app として組み込み、`privapp-permissions-maleicacid-tvinput` を同じ product image に入れる。

確認対象は次のとおりとする。

```text
/product/priv-app/MaleicacidTvInput/MaleicacidTvInput.apk
/product/etc/permissions/privapp-permissions-maleicacid-tvinput.xml
/product/etc/permissions/android.software.live_tv.xml
```

## Direct Boot と boot receiver

TIS は `directBootAware=true` を維持する。`LOCKED_BOOT_COMPLETED` では device protected storage に pending flag だけを記録し、TvProvider、Tuner、JNI parser は user unlock 後にだけ起動する。

`ACTION_USER_UNLOCKED` は manifest receiver へ登録しない。実行中 component がある場合だけ、`MaleicacidTvInputService` または `SetupActivity` が context-registered receiver を使って unlock 後 drain を行う。

## flash 後の確認

```bash
adb shell pm list features | grep android.software.live_tv
adb shell ls /product/etc/permissions/android.software.live_tv.xml
adb shell ls /product/etc/permissions/privapp-permissions-maleicacid-tvinput.xml
adb shell dumpsys tv_input | grep -i Maleicacid
```

## TIS discovery 確認

system TV app から setup activity を起動でき、setup 後に少なくとも 1 つの clear-viewable channel が `TvContract.Channels` に登録されることを確認する。TIS は Tuner HAL binder を直接呼ばず、Tuner SDK API 経由で Tuner HAL にアクセスする。


## r50bi parental rating / CAS fallback integration check

- product の system TV app / rating definitions に `domain=com.android.tv`, `ratingSystem=ISDB`, `rating=ISDB_4..ISDB_20` が存在することを確認する。
- `TvProvider.Programs.COLUMN_CONTENT_RATING` に `com.android.tv/ISDB/ISDB_<age>` 相当の `TvContentRating.flattenToString()` が入ることを確認する。
- `Programs.COLUMN_INTERNAL_PROVIDER_DATA` に CAS 状態、`publishStateSource`、raw parental rating 診断JSONが残ることを確認する。
- parental controls enabled + blocked rating で `notifyContentBlocked()` が発生し、parental block を理由に `notifyVideoUnavailable()` を呼ばずに AV再生が停止または開始抑止されることを確認する。
- `onUnblockContent()` 後は同一 `channelUri + serviceKey + eventId + start/end + ratingString` の current program / rating に限って playback retry が許可されることを確認する。
- scrambled unsupported service は parental allowed でも playback success にせず、`notifyVideoUnavailable(TvInputManager.VIDEO_UNAVAILABLE_REASON_CAS_UNKNOWN)` を使うことを確認する。
