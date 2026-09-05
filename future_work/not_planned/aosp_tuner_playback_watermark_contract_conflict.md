# AOSP Tuner Playback DVR watermark 契約競合

## 位置付け

本書は、Android 14 Tuner HAL の Playback DVR watermark について、frozen Stable AIDL の `PlaybackSettings` 文面と、AOSP default HAL / VTS が前提とする実挙動を同時には満たせない上流契約競合を `future_work/not_planned` の既知差分として記録する。

この文書は Playback DVR の公開status、threshold値、queue状態、callback順序を独立に定義しない。現行製品で採用する具体的なPlayback status判定は `tuner_contract/DESIGN_JA.md` の「DVR playback status のAOSP互換判定」と `DvrSettings configure 完全契約` を正とする。

## 上流契約の競合

Android 14 Stable AIDL `PlaybackSettings.aidl` は `lowThreshold` / `highThreshold` の双方を playback の **unused space size in bytes** と記載し、それぞれ `SPACE_ALMOST_EMPTY` / `SPACE_ALMOST_FULL` のtriggerに使用すると定義している。

一方、AOSP default Tuner HAL のPlayback status判定は、FMQの `availableToRead` をthreshold比較へ使用し、`availableToWrite == 0` を `SPACE_FULL` とする。さらにAOSP VTS callbackは `SPACE_EMPTY` / `SPACE_ALMOST_EMPTY` を受けるとPlayback FMQへの書込みを継続し、`SPACE_ALMOST_FULL` / `SPACE_FULL` を受けると書込みを停止する。これはstatusをqueue内データ量のempty/fullとして扱う挙動であり、`PlaybackSettings` の「unused space」という文面をそのままfree-space量として解釈した挙動とは一致しない。

Android 14向けsample VTS configurationもPlayback DVRに `statusMask=15`、`lowThreshold=4096`、`highThreshold=32767` を与え、VTS callbackによる上記書込み制御を使用する。

したがって、製品側で単に `lowThreshold` / `highThreshold` の測定量を `availableToWrite` へ変更するだけでは、Stable AIDL文面の一部を採る代わりにdefault HAL / VTSのstatus意味と逆向きの挙動を導入し得る。逆に現行の `availableToRead` 基準を維持すると、Stable AIDLの「unused space size」という明文とは一致しない。

## 現行製品方針

本製品のAndroid 14 profileでは、実際のframework/VTSとの相互運用を壊さないため、AOSP default HAL / VTSの歴史的なPlayback status挙動を維持する。

ただし、この方針を frozen Stable AIDL の `PlaybackSettings.lowThreshold/highThreshold` 文面にも完全適合していると宣言してはならない。本件はAOSP上流の仕様文と参照実装・試験挙動の競合として扱い、どちらか一方を根拠に他方も満たしたとみなさない。

また、本件を隠すために次を行ってはならない。

- vendor独自のAIDL fieldまたはstatusを追加して標準契約を置換する。
- statusMaskで要求されたALMOST statusを成功no-opとして永久に抑止する。
- queue snapshotやthresholdを恣意的に変換し、AIDL文面とVTS挙動の双方を満たしたように見せる。
- VTSだけを通す特殊分岐を製品runtimeへ追加する。

## AOSP根拠

監査時は少なくとも次のAndroid 14系AOSP一次資料を突き合わせる。

- `hardware/interfaces/tv/tuner/aidl/android/hardware/tv/tuner/PlaybackSettings.aidl`: `lowThreshold` / `highThreshold` をunused space sizeと記載するfrozen Stable AIDL。
- `hardware/interfaces/tv/tuner/aidl/default/Dvr.cpp`: AOSP default HALのPlayback status判定。
- Tuner VTSの`onPlaybackStatus()`処理: `SPACE_EMPTY` / `SPACE_ALMOST_EMPTY` で入力継続、`SPACE_ALMOST_FULL` / `SPACE_FULL` で入力停止する挙動。
- `hardware/interfaces/tv/tuner/config/sample_tuner_vts_config_aidl_V1.xml`: Playback DVRのstatusMask / thresholdを含む試験設定例。

AOSP branch、tagまたはcommitが変わった場合は、同じ相対path名だけを根拠にせず、その対象版の本文とVTS挙動を再確認する。

## 再評価条件

次のいずれかが成立した場合に本件を再評価する。

- 対象Android 14互換性契約について、Google/AOSPがStable AIDL文面とdefault HAL / VTS挙動のどちらを規範とするか明示した場合。
- Android 14向けcompatibility testまたは公式backportで、Playback thresholdの測定量と比較方向が一意に固定された場合。
- 製品の対象Android世代を変更し、その世代ではAIDL文面・参照HAL・VTSが同じ意味へ収束していることを確認できた場合。

再評価で製品判定式を変更する場合は、まず `tuner_contract/DESIGN_JA.md` のPlayback watermark正本を更新し、`tuner_hal2/DESIGN_JA.md` はその公開意味論を複製せず実装接続だけを追従させる。

## 監査上の扱い

本件は製品側だけでは解消不能な既知のAOSP upstream contract conflictである。

「`future_work` に記載された既知差分を除く」という条件で設計監査を行う場合に限り、本件を既知除外事項として扱う。Stable AIDL文面への逐語的適合、AOSP default HAL互換性、VTS相互運用性の全てを同時に要求する監査では、本件を除外せず上流競合として明示する。