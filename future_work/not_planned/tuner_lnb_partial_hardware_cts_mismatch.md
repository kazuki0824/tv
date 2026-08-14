# Tuner LNB 部分ハードウェア能力と CTS 基礎操作の不一致

## 位置付け

この文書は、`earth_pt1` / `px4_drv` が実機・driverとして提供する LNB 制御能力と、Android CTS が non-null の `Lnb` に要求する基礎操作一式の粒度が一致しない問題について、採用理由、比較した代替案、既知 compatibility delta、再評価条件を記録する。

部分 LNB 能力を公開する product-level 判断の規範正本は `開発規則.md` の「LNB部分能力公開の製品例外」とする。LNB endpoint の公開条件、operation / value ごとの能力、戻り値、資源寿命、callback、rollback / cleanup その他の Tuner HAL 公開契約の規範正本は `tuner_hal/DESIGN_JA.md` とする。この文書はそれらの規範値・状態遷移・製品判断を独立に再定義しない。

## AOSP / CTS 側の事実

Android CTS の `android.media.tv.tuner.cts.TunerTest#testLnb()` は、`Tuner.openLnb(...)` が `null` を返した場合は LNB 試験を終了する。一方、non-null の `Lnb` が得られた場合は、同一 LNB に対して次を連続して要求する。

1. `setVoltage(targetLnbVoltage)` が `Tuner.RESULT_SUCCESS`
2. `setTone(Lnb.TONE_NONE)` が `Tuner.RESULT_SUCCESS`
3. `setSatellitePosition(Lnb.POSITION_A)` が `Tuner.RESULT_SUCCESS`
4. `sendDiseqcMessage(new byte[] {1, 2})`

さらに `testLnbAddAndRemoveCallback()` は同じ基礎操作を実行したうえで、`sendDiseqcMessage()` 後に `LnbCallback.onDiseqcMessage()` が呼ばれたことを確認する。

したがって、上記の一部だけを実処理できる LNB endpoint を non-null で公開する構成では、実処理できる個別機能が正しく動作していても Android 14 CTS の LNB 試験全体は合格しない。この不一致が、本件で記録する compatibility delta である。

参照:

- AOSP CTS `TunerTest.java`: https://android.googlesource.com/platform/cts/+/105d6f1ab8b916880af25847d71f01d5acc930e3/tests/tests/tv/src/android/media/tv/tuner/cts/TunerTest.java
- AOSP Tuner AIDL `ILnb.aidl`: https://android.googlesource.com/platform/hardware/interfaces/+/2caf529bdcf4ff02ad941f77f158b680f3a5a4dc/tv/tuner/aidl/android/hardware/tv/tuner/ILnb.aidl

## 採用理由の記録

Product Owner は、対象 hardware / driver が実際に有する LNB 制御能力を framework から利用可能にすることを、Android 14 CTS の LNB 試験合格より優先する判断を採用した。規範としての当該 product-level 判断は `開発規則.md` を参照する。

この判断で重視した点は、hardware / driver の実能力と framework から観測できる能力の乖離を避けること、および CTS 合格のためだけに実処理していない操作を成功したように見せないことである。個々の operation / value をどのように公開し、どの結果を返すかは本書では定義せず、`tuner_hal/DESIGN_JA.md` を参照する。

## 採用時に比較した代替案

次の案を比較した。

- CTS の LNB 試験を優先し、基礎操作一式を満たさない LNB endpoint を framework から一律に隠す案。
- hardware / driver が実処理しない操作を成功扱いにして CTS の期待へ合わせる案。
- hardware / driver の実能力を公開し、CTS との不一致を既知 compatibility delta として管理する案。

採用された案は `開発規則.md` の「LNB部分能力公開の製品例外」に記録されている。本書では、未採用案を将来の公開契約として規範化せず、判断時に比較した選択肢としてのみ記録する。

## 既知 compatibility delta

採用された product-level 判断により、対象製品では Android 14 CTS の LNB 試験が失敗し得る。これは AOSP / CTS の意味論を変更したと解釈しない。また、この記録だけを根拠に CTS / CDD 適合を宣言しない。

## 再評価条件

次のいずれかが生じた場合は、この compatibility delta の前提と Product Owner 判断を再評価する。

- AOSP 側に LNB の operation / value 単位の capability 表現が追加され、部分能力公開と CTS 要求を両立できる契約になった場合。
- 対象 hardware / driver が Android CTS の LNB 基礎操作一式を実処理できるようになった場合。
- 対象 CTS / CDD の LNB 要求が変更され、現在記録している不一致が解消または変質した場合。

再評価で product-level 判断を変更する場合は `開発規則.md` を更新し、公開 API 契約を変更する場合は `tuner_hal/DESIGN_JA.md` を更新する。本書はその決定理由と compatibility delta の記録を追従させる。
