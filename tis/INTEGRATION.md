# MaleicacidTvInput 統合手順

この文書は `tis/` の product 統合条件を固定する。Tuner HAL 側の統合手順は `tuner_hal2/INTEGRATION.md` を正とし、この文書には重複して記載しない。TIS の runtime 状態遷移、TvProvider 利用、Direct Boot の保留・再試行、ライブセッション優先順位、録画 API、視聴年齢制限、CAS 状態写像は `DESIGN_JA.md` を正とし、本書では再定義しない。

### JNI実装内部ログの通常レベルと詳細診断

SI JNIライブラリの`JNI_OnLoad`でAndroid標準`android_logger`を一度初期化し、Rust targetの`jni::`配下だけTRACE（logcatのVERBOSE）を整形・書込み前に抑止する。JNIのDEBUG以上と他Rust targetのTRACEは維持し、Javaの`MaleicacidTIS`診断には影響させない。初期化・選別の契約は`../arib_si_engine_rs/DESIGN_JA.md`を正とする。

Android 15の[標準Rust logger](https://github.com/aosp-mirror-neo/platform_external_rust_crates_android_logger/blob/android15-release/src/lib.rs)は長いmodule名を先頭21バイトと`..`の23バイトへ短縮してからliblogへ渡す。`jni::wrapper::objects..`はこの実タグであり、logcatの表示省略ではない。[LineageOSのIsLegalPropertyName](https://github.com/LineageOS/android_system_core/blob/lineage-22.1/init/util.cpp)は末尾ドットと連続ドットを拒否するため、同名の`log.tag.*`プロパティを製品へ追加してはならない。完全なmodule名のプロパティや親prefixのプロパティも短縮後タグとの完全一致にならず、代替にならない。

JNI内部TRACEを詳細調査する場合は`arib_si_engine_rs/src/jni_logging.rs`の標準フィルタ設定を調査用buildで`trace`へ変更してJNIライブラリを再buildする。`adb setprop`だけではこのRust側フィルタを解除できない。調査後は通常設定へ戻して再buildする。診断専用のruntime切替APIや全体`log.tag`設定は追加しない。

製品build・起動後に、選局から通常視聴・停止・再選局までフィルタなしlogcatを採取し、`jni::wrapper::objects..`等のJNI VERBOSEが消え、`MaleicacidTIS`のDEBUG・停止phase診断が残ることを確認する。性能比較は受信局、vCPU、ログ収集・Perfettoの有無を一致させ、Codec入力返却・出力、画面最終フレーム、CPU時間を比較する。ログ抑制のみで音声入力飽和やCodec停滞の解消を確約しない。

## Section受付容量の負荷検証（未完了）

現行16件の受付上限の位置付けは`DESIGN_JA.md`のController executor契約を正とする。固定TSの回帰証跡は[PR #123](https://github.com/kazuki0824/tv/pull/123)の`realTsBurstLossAndRetransmissionStayWithinCandidate`と同PRのfixture READMEを参照する。既存HAL section処理・SDK callback・SI JNI・候補別喪失snapshot・SI完成判定を実行し、owner占有時の複数Filter入力、喪失保持、再送回復、Filter交換・候補切替を確認する。試験成功は当該固定条件の機能回帰であり、実機の入力率に対する16件の十分性を表さない。

実機の容量判断には次の証跡を同一試行で揃える。未取得の間は容量の必要十分性を確認済みとしない。

- 日時、製品build/HEAD、機種、scan候補・実Filter数、入力TSのhashまたは放送局/RF条件、ログ採取条件。
- 既存ownerの保留ピーク、Section処理時間、control操作別占有時間。時間単位と観測区間を明記し、hostの意図的待機時間を実機値へ読み替えない。
- 各候補開始/終了の`inputDeliveryLoss`・`filterOverflow` snapshotと差分、required SIの不足集合、収集終端結果、同一候補の再送後完成結果。
- Filter交換・次候補への旧callback混入がないこと、およびロスがある候補をロスなしとして報告しないこと。

PR #192旧本文の15,409件は一次ログ・集計条件を確認できておらず、容量判断の証拠から除外する。16件維持または必要最小修正の判断は上記の実測後に行う。広告Filter数に比例した拡張、独自worker/queue、別の完成判定を未検証の数値から追加しない。

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

`MaleicacidTvInput` はTuner、TvProvider、privileged permissionを利用するplatform-coupled componentであるため、`/product`へ配置せず、`system_ext_specific: true`かつ`platform_apis: true`の`/system_ext` priv-appとして組み込む。MediaSync private listener patchは任意のplatform統合差分として扱い、TIS runtimeが最終出力観測の可否を選択する条件とavailabilityの意味は`DESIGN_JA.md`を正とする。本書ではpatchの有無によるruntime状態遷移を再定義しない。その他のplatform APIをreflectionやHAL binder直呼びへ一般化してはならない。

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

libaribcaption rendererのproduct統合は本節、viewport / PTS / scheduler / decoder・renderer lifecycle等のruntime設計は`DESIGN_JA.md`をそれぞれ正本とする。旧shared-library前提の別future_work文書を維持せず、残るbuild gateと実機の字幕PES→decoder→renderer→RGBA8888→overlay確認は本節の統合確認として管理する。

## 権限と priv-app

`MaleicacidTvInput` は `/system_ext` priv-app として組み込み、`privapp-permissions-maleicacid-tvinput` を同じ system_ext image に入れる。

Tuner SDK は `TunerResourceManagerService.registerClientProfile()` を経由してTRMへclientを登録するため、TISは `android.permission.TUNER_RESOURCE_ACCESS` をmanifestで要求し、同permissionを `privapp-permissions-maleicacid-tvinput.xml` で許可する。`ACCESS_TV_TUNER` と `ACCESS_TV_DESCRAMBLER` だけではTRM client登録条件を満たさない。Tuner HAL serviceと `android.hardware.tv.tuner` featureが存在していても、このpermissionが付与されていなければ `Tuner` の生成は `registerClientProfile()` の `SecurityException` で失敗する。

実機ではTIS起動前に次を確認する。

```text
cmd package check-permission android.permission.TUNER_RESOURCE_ACCESS com.maleicacid.tvinput
```

結果がgrant済みであることを確認し、Tuner生成時に `registerClientProfile()` のpermission拒否が発生しないことを確認する。

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

録画・予約のrelease境界は `../開発規則.md` を正とする。同正本で録画機能が有効化されていないreleaseでは、`rec/` 配下の予約録画サービス、receiver、test moduleをproduct packageまたはrelease確認gateへ入れず、TIS manifestの録画capabilityも `DESIGN_JA.md` のruntime契約に一致させる。

`MaleicacidRecScopeTests` は録画・予約機能を扱う作業で明示指定し、録画機能を有効化していないreleaseのbuild / atest / VTS / 実機確認gateへ混ぜない。

## setup scan後のchannel可視化

MaleicacidTvInputは`system_ext_specific: true`、`privileged: true`、platform certificateの製品appとして、setup初期可視性をTvProviderへ反映するため`com.android.providers.tv.permission.ACCESS_ALL_EPG_DATA`を`AndroidManifest.xml`と`privapp-permissions-maleicacid-tvinput.xml`で宣言・付与する。既存のchannel/program queryは引き続き`buildChannelsUriForInput(inputId)`等の所有row URIへ限定し、このpermissionをraw SQL selectionや他package row参照の理由に使わない。

TvProvider列の具体投影は`../ARIB_SI_EPG_TvProvider投影方針.md`、setup scanのstaging / commit / rollback / pending回収は`DESIGN_JA.md`を正とし、本書では列値やterminal state transitionを再定義しない。

実機では次を確認する。

```text
cmd package check-permission com.android.providers.tv.permission.ACCESS_ALL_EPG_DATA com.maleicacid.tvinput
content query --uri content://android.media.tv/channel --projection _id:input_id:display_number:display_name:browsable:internal_provider_flag1
```

permissionがgrant済みであることと、freshなTvProvider状態からsetup scanを実行した結果が、上記2正本で定義する通常channel / one-seg / pending / 既存hidden channelの投影およびruntime条件に一致することを確認する。未公開版相互の自動migrationは受入条件に含めない。

## Direct Boot の product 統合条件

TIS は `directBootAware=true` を維持する。`AndroidManifest.xml` には `<uses-permission android:name="android.permission.RECEIVE_BOOT_COMPLETED" />` を宣言し、`EpgBootSyncReceiver` は `android:directBootAware="true"` とする。`EpgBootSyncReceiver` の intent filter は `ACTION_LOCKED_BOOT_COMPLETED` と `ACTION_BOOT_COMPLETED` を含める。`ACTION_USER_UNLOCKED` は manifest receiver の対象にしない。

`BootEpgSyncJobService` は `AndroidManifest.xml` に service として宣言し、`android.permission.BIND_JOB_SERVICE` で保護する。`EpgBootSyncReceiver`、`BootEpgSyncJobService`、`DirectBootGuard`、`BootEpgSyncScheduler` の実行時役割、ジョブ登録・再試行、保留解除、開始条件、ライブセッションとの優先順位は `DESIGN_JA.md` を正とし、本書では状態遷移を再定義しない。

## ARIB exceptional ratingのLive TV App標準extension統合

ARIB raw ratingからAndroid canonical ratingへの写像は `../ARIB_SI_EPG_TvProvider投影方針.md`、`contentAgeHint`、stock Live TV preset / custom policy、blocking / temporary unblock等のruntime policyは`DESIGN_JA.md`をそれぞれ正とする。本書はそれらを再定義せず、独立`AribContentRatings` APKをTIF標準rating providerとして発見可能にするproduct統合だけを所有する。

製品buildでは`AribContentRatings`とstock `LiveTv`を組み込み、rating-provider receiverとXML metadataが正本設計どおり発見できることを確認する。System TV App本体へのARIB専用source patch、第二policy APK、TV App private state readerを追加しない。

```text
m AribContentRatings LiveTv
```

product tree / 実機ではprovider discoveryと、`DESIGN_JA.md`が要求する標準parental-control経路の確認を行う。具体的なrating値・preset別結果は同設計正本の期待値から導出し、本書へ重複表を持たない。

## 任意のMediaSync Exact-mode platform統合

この節をLineageOS 22.1向けMediaSync platform patchを任意に適用する場合の手順と確認項目の正本とし、patch配下へ別のREADMEや重複手順書を置かない。

TISの最終出力観測の可否選択条件と各modeのruntime意味は`DESIGN_JA.md`を正とする。本節はExact mode用private listenerを提供する任意patchの適用・build・実機確認だけを所有する。TIS APKは追加private APIを静的参照せず、patch本文はTIS側runtime契約とは独立した再現可能なplatform統合差分として維持する。

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

patch適用後は次のtargetをbuildする。対象treeのmodule分割でmodule名が異なる場合は、`frameworks/base`のmedia JNI / framework Java、`frameworks/av`のMediaSync、TISを実際に再コンパイルする同等Soong targetを使用する。

```bash
m framework-minus-apex
m libstagefright
m MaleicacidTvInput
```

patchを適用した構成の実機ではprivate listenerのJava/JNI/native型接続と、`DESIGN_JA.md`がExact modeに要求する観測を確認する。TIS host CIはstock APIでの静的compileとrepository内契約を確認するものであり、このplatform patchのnative実行時確認を代替しない。未パッチ構成では公開Codec callbackを近似Compatibilityとして使用する。利用不能なprivate callbackだけを理由に正常視聴を停止させず、最終出力の厳密な継続監視は表明しない。未パッチ構成のruntime結果をExact modeの確認結果として扱わない。これらは任意patchを採用する製品の統合・検証gateなので`future_work/r53`へ重複配置しない。

## 黒画面・停止不能・旧映像の診断採取

通常の理由ログは、再生不能理由・圧縮入力欠落・Codec error・停止段階を識別する。control待機超過時のowner Java stackはnative内部の待機相手を示さない。旧映像の所属世代を調べる場合も、Compatibility通知を最終表示の証拠にしてはならない。この節の採取は診断専用であり、再生期限・容量・回復方針を変更しない。

### 採取操作

LinuxホストのBash、adb、GNU timeoutと、`adb root`が使えるuserdebug端末を用いる。`ANDROID_SERIAL`で対象を選択する。開始前に視聴アプリを開き、スキャン終了後だけでなく、問題の選局より前から採取する。ログをclearせず全bufferを保存する。

```bash
adb root
adb wait-for-device
# vendor/maleicacid/tvをカレントディレクトリにする。
bash tis/diagnostics/capture_playback.sh "$PWD/playback-capture" 120
```

`perfetto.txt`の記録開始を確認してからMXワンセグ1とNHK総合1を交互に選局し、黒画面になるまで待つ。別ターミナルから、症状発生直後・次の選局操作より前に次を実行する。

```bash
bash tis/diagnostics/snapshot_playback.sh "$PWD/playback-capture" black
# 停止不能も発生した場合は別名で追加採取する。
bash tis/diagnostics/snapshot_playback.sh "$PWD/playback-capture" stuck
```

snapshotは症状marker、uptime、thread一覧、TIS・Live TV・mediatuner・system_server・software codec・audioserver・SurfaceFlinger・Tuner HALのnative backtraceを採る。`debuggerd -b`は対象を終了させないが、一時停止による観測影響がある。native採取を先に並行開始し、その後にdumpsysを保存する。各外部採取commandの20秒上限はホスト収集を打ち切るためだけのもので、製品の停止期限ではない。対象不在、権限拒否、非0終了、timeoutは個別ファイルに残し、採取済みと読み替えない。必要なら再現を分けて旧映像の撮影と停止時stack採取を行う。

旧映像が一瞬出る現象は外部カメラでも撮影し、切替前後のチャンネル名と症状markerの操作を同じ撮影に含める。TvViewのsecure Surfaceはscreenrecordに映らない場合があるため、screenrecord成功を映像採取成功としない。採取時間はホスト引数で最大600秒まで指定できる。診断propertyはscript終了時に元へ戻す。ホスト異常終了時は`log.tag.MaleicacidTIS`と`log.tag.MediaSync`を採取前の値へ戻す。端末traceは`device-trace-path.txt`の場所にも残る。

### 証拠の対応と採取成立条件

| 境界 | 対応付ける記録 | 判定できる範囲 |
|---|---|---|
| TIS世代→native MediaSync | 同一pid/tidの`PLAYBACK_SYNC_CREATE_BEGIN`→`PLAYBACK_SYNC_CREATE native`→`PLAYBACK_SYNC_CREATE_END` | session/generationとnative instanceの対応。pointerは破棄後に再利用され得るため生成区間も用いる。 |
| Filter→TIS owner | `PLAYBACK_CALLBACK_OWNER_LATENCY`のownerWaitUs、events、payloadBytes、reservedKiB、拒否時の`PLAYBACK_CALLBACK_BUDGET_REJECT` | callback待ちと16MiB製品受付予算の区別。予約量は実メモリ使用量ではない。 |
| TIS→Codec入力と返却 | `PLAYBACK_CODEC_INPUT_QUEUE`のqueueUs・pending・queued、`PLAYBACK_CODEC_INPUT_RETURN`のcallbackWaitUs・ownerUs・available、および世代別`CODEC_INPUT_AVAILABLE/RETURNED` | Codec.queue所要時間、空き入力index再通知、TIS owner滞留を区別。入力index返却はnative HALのMediaEvent解放成功そのものではない。 |
| Codec→MediaSync入力 | `PLAYBACK_VIDEO_SUBMIT/RETURN`のgeneration・PTS、`PLAYBACK_CODEC_OUTPUT_RETURN`のcallbackWaitUs・ownerUs、`PLAYBACK_SYNC_INPUT`のnative・PTS・buffer ID・input frame | Codec output取得と入力bufferの所属。`PLAYBACK_CODEC_FRAME`のcallback時刻は最終表示時刻ではない。 |
| MediaSync音声consume | `PLAYBACK_MEDIASYNC_AUDIO_CONSUMED`のbufferId・ownerWaitUs・outstanding、および世代別`MEDIASYNC_AUDIO_CONSUMED` | PCMバッファ消費callbackとTISでの返却進捗。音声PTSの離隔は資源超過ではない。 |
| MediaSync→出力queue | `PLAYBACK_SYNC_SCHEDULE`のmediaUs/targetUsと`PLAYBACK_SYNC_OUTPUT`のbuffer ID・input frame・nextOutputFrame・status | 成功時のoutput frame numberはnextOutputFrame−1。queue成功は画面へのpresent成功ではない。 |
| 最終合成・表示 | PerfettoのSurfaceFlinger layers/FrameTimelineとbuffer ID・output frame、外部撮影 | layerの可視性、保持buffer、新frameのlatch/presentとの対応。TISの世代番号を時刻だけで別Surfaceへ転用しない。 |
| 停止待機 | 同じ症状区間のJava owner stack、native backtrace、Binder/scheduler trace、HAL dump | API境界と待機相手の候補。native stackが欠けた場合は内部原因確定としない。 |

端末で`PLAYBACK_SYNC_CREATE`・`PLAYBACK_SYNC_INPUT`・`PLAYBACK_SYNC_OUTPUT`が出ること、対象SurfaceFlinger layerのbuffer記録がtraceに存在することを確認する。data source一覧に登録があるだけでは採取成立としない。PerfettoのTrace statsでbuffer overwrite・packet loss・ftrace overrunを確認し、生成から症状までの必要区間が欠けていれば時間短縮等で再採取する。圧縮時刻は世代ごとに同じ値から開始し得るので、PTS単独で世代を識別しない。

採取資料は出力ディレクトリ全体、外部撮影、ビルドrevision/差分を一組にする。native symbol解決が不足する場合は、同じ製品buildのsymbols（libstagefright、libmediandk等の該当ライブラリとHAL）も保持する。この採取で、旧bufferが残って見えた場合と新generationのbufferまで進んだ場合を切り分ける。後者の画素内容が既に誤っている場合、下流の表示traceだけで上流ES/復号の原因まで確定することはできない。ログ欠落・採取によるタイミング変化・表示装置内部の遅延も含め、全原因を一回で必ず特定できるとは保証しない。

## flash 後の確認

```bash
adb shell pm list features | grep android.software.live_tv
adb shell ls /product/etc/permissions/android.software.live_tv.xml
adb shell ls /product/app/AribContentRatings/AribContentRatings.apk
adb shell ls /system_ext/etc/permissions/privapp-permissions-maleicacid-tvinput.xml
adb shell ls /system_ext/priv-app/MaleicacidTvInput/MaleicacidTvInput.apk
adb shell cmd package check-permission android.permission.TUNER_RESOURCE_ACCESS com.maleicacid.tvinput
adb shell dumpsys tv_input | grep -i Maleicacid
```

## TIS discovery 確認

システムTVアプリから設定画面を起動でき、setup 後に少なくとも1つの非スクランブル視聴可能チャンネルが `TvContract.Channels` に登録されることを確認する。runtime の channel 登録条件と Tuner SDK API 利用境界は `DESIGN_JA.md` を正とする。

## 視聴年齢制限 / CAS の product 統合確認

製品統合では、AOSP system-defined ISDB ratingに加え、`AribContentRatings` のTIF標準rating-provider package / receiver / XML metadataがstock TV Appから発見可能であることを確認する。raw ARIB値からcanonical Android ratingへの写像は `../ARIB_SI_EPG_TvProvider投影方針.md`、`contentAgeHint`、preset / CUSTOM policy、PIN解除、`TvInputManager.isRatingBlocked()` とTIS通知のruntime意味は `DESIGN_JA.md` をそれぞれ正とし、本書では再掲しない。

System TV App本体へARIB専用source patchを追加せず、独立rating providerをproduct imageへ組み込むこと、stock TV Appからproviderが発見できること、設計正本が要求するrating metadataが実機で列挙できることを統合確認対象とする。

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

### 実TS結合試験のホストCI接続設計

この節は`DESIGN_JA.md`の「実TSによるホスト結合試験」をCIへ接続する際の構成を定める。試験本体は`host_ci/robolectric/src/test/kotlin`、HAL実行器は`tuner_hal2/host_ci/demux/src/bin/real_ts_sections.rs`に配置する。実装の存在とCIでの成功は別に確認する。

既存`.github/workflows/tis-robolectric-ci.yml`のホステッドランナーを使い、Android emulator、チューナー、特権container、self-hosted runnerを要求しない。既存3クラスの実行指定を維持したうえで、同じGradle実行に`com.maleicacid.tvinput.tis.RealTsHalSiIntegrationTest`を追加する。製品TISのAPKへ試験ファイル・試験専用実行器を含めない。

| 接続箇所 | 接続内容 |
|---|---|
| `tuner_hal2/host_ci/demux` | 製品demuxをlinkする実TS実行器をhost専用bin targetとして追加する |
| `arib_si_engine_rs/host_ci` | 既存のJNI `cdylib` targetをそのままbuildする。別のSI parserを追加しない |
| `tis/host_ci/robolectric` | ホスト専用test source directoryを追加し、試験用実行器・fixture directoryの絶対pathをGradleのTest taskへsystem propertyとして渡す。JNI出力directoryをTest JVMの起動時`java.library.path`へ設定する |
| `tis/tests/fixtures/real_ts` | `test.ts`と`expected.json`を直接参照する。既存の`main.assets`や製品Soongのasset/dataへ追加しない |
| workflowの対象path | 既存`tis/**`に加え、`tuner_hal2/**`と`arib_si_engine_rs/**`をpull_request／push両方に追加し、HAL／SIだけの変更でも結合試験を実行する |
| workflowの実行順 | checkout → Rust 1.81.0・JDK 17・Gradle 8.9の準備 → HAL実行器build → SI JNI build → AVC AU/PTS実TS試験を明示実行 → 既存Robolectric試験とSI実TS試験を実行 → JUnit XML・HTML・HAL診断・AVC試験ログを保存する |
| workflowの失敗 | build／リンク失敗、時間切れ、試験0件、対象試験のskipは失敗とする。`continue-on-error`を指定しない。診断とJUnit結果は失敗時にも保存する |

既存JNIのbuildコマンドは`cargo +1.81.0 build --manifest-path arib_si_engine_rs/host_ci/Cargo.toml --locked --lib`とする。HAL実行器にも同じtoolchainと`--locked`を使い、必要なCargo.lock変更は実装と同時にcommitする。試験クラスはホスト専用source setへ置き、`tis/tests/src`へ置いて未対応のhost subprocessを端末atestから呼ばせない。Robolectricの対象SDKは既存compileSdkと整合する35へ固定する。

この結合試験の実装完了条件は、`DESIGN_JA.md`の全判定がGitHub-hosted runnerで1件のJUnit成功として出力され、同じ実行で既存3クラスも成功することとする。設計・fixtureだけのPRで既存CIが成功しても、この条件の達成には数えない。ホスト試験結果はSoong build、端末atest、CTS／VTS、実機視聴の結果と分けて報告する。

### AVC AU・PTS実TS回帰のCI接続

同じhost workflowから、#116の\`tuner_hal2/demux/src/av/video_timestamp.rs\`にある\`captured_transport_frames_have_individual_timestamps\`を、\`AVC_CAPTURE_PATH="$PWD/tis/tests/fixtures/real_ts/test.ts"\`と\`cargo test --manifest-path tuner_hal2/host_ci/Cargo.toml --locked -p maleicacid-tuner-hal2-demux-host-ci --lib captured_transport_frames_have_individual_timestamps -- --ignored --nocapture\`で明示実行する。fixtureの長さ・SHA-256は\`expected.json\`と照合し、実行件数1件・成功1件・失敗0件・ignored0件・AU計数の存在を確認する。テストの規範値は#116に保持し、#123に別のAVC parserや試験専用の製品APIを追加しない。

\`set -euo pipefail\`でCargo試験失敗をCI失敗へ伝え、診断ログ\`tis/host_ci/robolectric/build/real-ts-diagnostics/avc-au-pts.log\`を既存の\`if: always()\` artifactへ保存する。PSI/SI結合試験の1件実行と既存Robolectric試験の検証も維持する。ローカルではリポジトリrootから同じ\`AVC_CAPTURE_PATH\`指定で実行できる。ホスト成功をSoong・device atest・CTS/VTS・実機表示成功へ読み替えない。

### 仕様カバレッジ

```text
- provider-data JSON v1、descriptor 診断、未対応 codec 試験データは maleicacid_arib_si_engine_rs_test と MaleicacidTvInputAcceptanceTests で確認する。
- TvProvider 標準列投影、字幕トラック、視聴年齢制限、CAS 仮実装境界、設定、scan、チャンネル登録は MaleicacidTvInputAcceptanceTests の対象とする。
- 録画・予約の確認対象は `../開発規則.md` のrelease境界から導出し、未有効化releaseでは `MaleicacidRecScopeTests` を通常gateへ含めない。
```

### 実機投入後の確認

```bash
adb shell pm list features | grep android.software.live_tv
adb shell ls /product/etc/permissions/android.software.live_tv.xml
adb shell ls /product/app/AribContentRatings/AribContentRatings.apk
adb shell ls /system_ext/etc/permissions/privapp-permissions-maleicacid-tvinput.xml
adb shell ls /system_ext/priv-app/MaleicacidTvInput/MaleicacidTvInput.apk
adb shell cmd package check-permission android.permission.TUNER_RESOURCE_ACCESS com.maleicacid.tvinput
adb shell dumpsys tv_input | grep -i Maleicacid
```

合格条件:

```text
- システムTVアプリから設定画面を起動できる。
- setup 後に少なくとも1つの非スクランブル視聴可能チャンネルが TvContract.Channels に登録される。
- system_ext image にTIS本体・privapp allowlist・TIS専用JNIが配置され、product image にlive_tv feature XMLとAribContentRatingsが配置される。
- `AribContentRatings`がstock TV Appから発見され、既存preset/custom policyを通じてARIB exceptional ratingのblock/unblockとPIN一時解除、第三者rating非干渉が成立する。
- android:canRecord="false" が product manifest metadata に反映される。
```

### 映像native入力待機の修正後確認

同一ビルドで、`stop && start`後と端末reboot後を分け、MXワンセグ1を選局する。各試行のtv/toolsのHEAD、選局時刻、症状時刻を記録し、上記の診断採取でlogcat・Perfetto・停止前のJava/native stackを同じ区間として保持する。特定のWARN出力だけを採取開始条件にせず、画面停滞を観測した時点で停止前のsnapshotを採る。

`PLAYBACK_CODEC_INPUT_QUEUE`の長いqueueUsが残る場合、映像workerでのnative待機と、共通playback owner上の`PLAYBACK_MEDIASYNC_AUDIO_CONSUMED`のownerWaitUs・制御処理を対応付ける。映像native入力待機中にも音声返却と制御が進むこと、選局解除・再選局で`PLAYBACK_STOP_COMPLETE`へ到達することを確認する。native入力の所要時間自体がゼロになることを合格条件にしない。`PENDING_QUEUE_FULL`・Codec例外・映像破損・黒画面が残る場合は、その区間を別の未解決として解析し、host回帰成功だけで実機改善へ読み替えない。

### 通常ログで入力待ちが満杯になった場合

`PLAYBACK_INPUT_LOSS`および再生不能理由ログの`video`/`audio`状態を確認する。`inboundInputCallbacks`は到着済みでownerが未処理のCodec入力callback、`queuedInputs`はCodecへ投入してまだ返却されていない入力、`nativeRenderPending`はnativeへの出力buffer返却呼出しが未完了の数を表す。これらは同じplayback ownerで読んだ資源状態であり、native内部の待機相手や最終表示を証明しない。

`pendingSamples=64`だけで件数上限が不適切とは判断しない。上記採取操作を選局前から実行し、詰まった直後・再選局より前にsnapshotを保存する。`PLAYBACK_VIDEO_SUBMIT/RETURN`、Codec入力返却、MediaSyncの入力・出力、native stackを対応付ける。Compatibility modeでは最終表示callbackがないこと自体を再生失敗にしない。private callbackの不在を理由に必要な通常ログ・native stackまで採取済みと扱わない。

### Sessionへ集約した障害後再開の統合確認

再開仕様は`DESIGN_JA.md`の「再生障害後の再開判断と予約」を正とする。対象stackを含む製品buildで、同じ選局の二回目以降の入力欠落からも再開できること、待機中の停止・再選局・release・Surface無効化・信号喪失・CAS未準備・視聴禁止が旧世代を再開させないことを確認する。音声障害後は映像のみで再開し、自動で音声を再有効化しないことも確認する。

停止完了、Sessionの障害受理、次の開始、世代番号、映像利用可能の根拠を既存ログで照合する。継続的な入力欠落時は300ms待機の実機負荷・再生不能時間を測り、再開したという事実だけで妥当と判定しない。ホスト試験の予約資格・解放順序の成功を、実Codec・最終描画・製品buildの成功へ読み替えない。VMのCPU数と視聴チャンネルは記録し、入力保持上限の変更と同時評価にしない。

### Tuner閉鎖失敗後のSDK/TRM後始末

Tunerの閉鎖は標準APIのcloseFrontend()を先に呼び、成功後にclose()へ進む。追加Framework cleanup API/patchは不要である。実機では(a)登録0件→scan開始→途中中止→HALの実回収完了→Frontend/TRM解放→SDK close成功→新Tunerで再scan→登録→視聴、(b)実際のFrontend close失敗はnative参照破棄により再試行不能として元原因を保持、(c)Frontend解放後にDemux等の後段closeが失敗しownerを保持、を別々に確認する。close専用期限による正常な回収途中の失敗はHAL側で除去し、TISで失敗後の公開API再試行を期待しない。hostのSDK/Shadow境界試験を実機でのHAL回収・TRM調停の証明と扱わない。
