# MaleicacidTvInput 統合手順

この文書は `tis/` の product 統合条件を固定する。Tuner HAL 側の統合手順は `tuner_hal2/INTEGRATION.md` を正とし、この文書には重複して記載しない。

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


## 録画・予約の除外

現行 product 統合では `rec/` 配下の予約録画 サービス / receiver / test module を product package または release確認条件へ入れない。TIS メタデータは `android:canRecord="false"` を維持し、`onCreateRecordingSession()` は `null` を返す状態を 現行仕様の正とする。

`MaleicacidRecScopeTests` は録画・予約作業で明示指定して使う範囲に限定し、現行 product の build / atest / VTS / 実機確認 gate へ混ぜない。

## Direct Boot と起動時の受信処理

TIS は `directBootAware=true` を維持する。`AndroidManifest.xml` には `<uses-permission android:name="android.permission.RECEIVE_BOOT_COMPLETED" />` を宣言し、`BootReceiver` は `android:directBootAware="true"` とする。`BootReceiver` は `ACTION_LOCKED_BOOT_COMPLETED` と `ACTION_BOOT_COMPLETED` の双方を受信対象にする。`ACTION_LOCKED_BOOT_COMPLETED` ではデバイス保護領域に `DirectBootEpgPending` だけを記録し、TvProvider、Tuner、JNI 経由の解析処理は起動しない。

`BootReceiver.onReceive()` は既知の起動通知を判別し、`DirectBootEpgPending` を確認して、必要なら Android 標準の `JobScheduler` に固定識別子の `BootEpgSyncJobService` を登録するところまでで終了する。EPG の収集、Tuner の使用、TvProvider への反映処理は `BroadcastReceiver.onReceive()` の寿命では実行しない。`BootEpgSyncJobService` は `AndroidManifest.xml` で `android.permission.BIND_JOB_SERVICE` により保護し、利用者のロック解除後だけ実行対象にする。

起動時 EPG 同期用の `JobInfo` は再起動をまたいで永続化しない。再起動をまたぐ正本はデバイス保護領域の `DirectBootEpgPending` だけとし、再起動後は起動通知から同じジョブ登録判定を行う。ジョブ識別子は起動時 EPG 同期用に固定し、`JobScheduler.getPendingJob()` で同じジョブが登録済みなら再登録しない。`BootEpgSyncJobService.onStartJob()` はロック解除、`DirectBootEpgPending`、開始条件を再確認し、処理を開始する場合は `BootEpgSyncCoordinator` へ引き渡して `true` を返す。`BootEpgSyncCoordinator` は同一プロセス内で `inputId` ごとの起動時 EPG 同期を一度に1件だけ実行する。処理完了時は `jobFinished()` で終了を通知し、成功時は再試行を要求せず、未完了または失敗で `DirectBootEpgPending` が残る場合は再試行を要求する。`JobScheduler` が処理を中断して `onStopJob()` を呼んだ場合は進行中の走査と Tuner 資源を停止・解放し、`DirectBootEpgPending` が残る限り再試行を要求する。

`ACTION_BOOT_COMPLETED` は利用者のロック解除後に起動時 EPG 同期へ入る正規の入口とするが、この通知単独を無条件の再開保証とはしない。Android の背景実行制限などで通知が遅延しても状態を失わないよう、`DirectBootEpgPending` をデバイス保護領域に維持する。`ACTION_BOOT_COMPLETED` では `UserManager.isUserUnlocked()==true` を確認し、`DirectBootEpgPending=true` なら同じ開始判定へ進む。`ACTION_USER_UNLOCKED` は `AndroidManifest.xml` に登録せず、プロセスが利用者のロック解除まで生存している場合に動的な受信から同じ開始判定を前倒しする補助経路に限定する。`MaleicacidTvInputService.onCreate()` は Direct Boot の保留処理、起動時の EPG 同期、定期保守を直接開始してはならない。

起動時の EPG 同期と定期保守を開始できるのは、ライブセッション、セッション作成中、設定用の走査、再生処理、走査管理処理がすべて存在しない場合だけとする。開始条件を満たさない場合は `DirectBootEpgPending` を維持して開始を見送る。ライブセッション終了、セッション作成終了、設定用走査終了、再生処理終了、走査管理処理終了など、開始を妨げる状態の更新後に全開始条件が不成立から成立へ変わった場合は、同じ `DirectBootEpgPending` を再評価する。保留中なら `BootEpgSyncCoordinator` へ同じ開始要求を出す。周期的な監視、新しい永続待ち行列、別の定期実行機構をこの再評価のために追加しない。すでに起動時の EPG 同期または定期保守が実行中の状態でライブセッション作成要求が来た場合は、当該処理を停止または延期し、ライブ視聴の選局を優先する。
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
- TvProvider 標準列投影、字幕トラック、視聴年齢制限、CAS 仮実装 境界、設定、scan、チャンネル登録 は MaleicacidTvInputAcceptanceTests の対象とする。
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
- システムTVアプリから設定画面 を起動できる。
- setup 後に少なくとも 1 つの 非スクランブル視聴可能チャンネルが TvContract.Channels に登録される。
- TIS は Tuner HAL binder を直接呼ばず、Tuner SDK API 経由で Tuner HAL にアクセスする。
- android:canRecord="false" を維持する。
```
