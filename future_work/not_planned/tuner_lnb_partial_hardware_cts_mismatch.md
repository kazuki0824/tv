# Tuner LNB 部分ハードウェア能力と CTS 基礎操作の不一致

## 位置付け

この文書は、`earth_pt1` / `px4_drv` が実機・driverとして提供する LNB 制御能力と、Android CTS が non-null の `Lnb` に要求する基礎操作一式の粒度が一致しない問題を、既知の not planned 項目として記録する。Android互換性を満たすcanonical契約の正本ではなく、`tuner_hal/DESIGN_JA.md`の互換性契約を上書きしない。

Android互換性を維持するproduct profileでは、canonicalの`aidl_baseline_eligible`条件を適用し、CTS基礎操作一式を成立させられない部分LNB endpointを公開しない。以下の部分LNB公開方針は、CTS/CDD互換性を意図的に放棄する非互換product variantを選択する場合の既知差分としてのみ記録する。

非互換product variantでは、対象 hardware / driver が実際に対応する LNB 制御を、CTS 合格のためだけに隠さない。反対に、hardware / driver が実処理できない tone、satellite position、DiSEqC 等を成功 no-op、擬似成功、callback echo で実装済みに見せない。

## AOSP / CTS 側の事実

Android CTS の `android.media.tv.tuner.cts.TunerTest#testLnb()` は、`Tuner.openLnb(...)` が `null` を返した場合は LNB 試験を終了する。一方、non-null の `Lnb` が得られた場合は、同一 LNB に対して次を連続して要求する。

1. `setVoltage(targetLnbVoltage)` が `Tuner.RESULT_SUCCESS`
2. `setTone(Lnb.TONE_NONE)` が `Tuner.RESULT_SUCCESS`
3. `setSatellitePosition(Lnb.POSITION_A)` が `Tuner.RESULT_SUCCESS`
4. `sendDiseqcMessage(new byte[] {1, 2})`

さらに `testLnbAddAndRemoveCallback()` は同じ基礎操作を実行したうえで、`sendDiseqcMessage()` 後に `LnbCallback.onDiseqcMessage()` が呼ばれたことを確認する。

したがって、電圧制御のみ、または上記の一部だけを実処理できる LNB endpoint を non-null で公開すると、対応可能な操作自体が正しく動作していても CTS の LNB 試験全体は合格しない。

参照:

- AOSP CTS `TunerTest.java`: https://android.googlesource.com/platform/cts/+/105d6f1ab8b916880af25847d71f01d5acc930e3/tests/tests/tv/src/android/media/tv/tuner/cts/TunerTest.java
- AOSP Tuner AIDL `ILnb.aidl`: https://android.googlesource.com/platform/hardware/interfaces/+/2caf529bdcf4ff02ad941f77f158b680f3a5a4dc/tv/tuner/aidl/android/hardware/tv/tuner/ILnb.aidl

## 非互換product variantを選択する場合の方針

- `earth_pt1` / `px4_drv` について、hardware / driver の証跡で実処理可能と確認した LNB 制御は公開経路へ接続する。
- 実処理できない LNB 操作を CTS 合格目的の成功 no-op にしない。
- CTS 合格目的だけで、実処理可能な LNB 制御まで一律に非公開化しない。
- 各 API / 列挙値の成功可否は backend capability と公開契約の対応表で明示し、対応不能要求は副作用なしの typed failure とする。
- 固定 LNB 給電と公開 `ILnb` 操作を混同しない。backend が固定給電だけを提供する場合と、caller が `ILnb.setVoltage()` で制御できる場合は別能力として扱う。

## not planned とする内容

次は採用しない。

- CTS の `testLnb()` を通すためだけに、未対応の `setTone(TONE_NONE)` を成功扱いにすること。
- CTS の `testLnb()` を通すためだけに、未対応の `setSatellitePosition(POSITION_A)` を成功扱いにすること。
- CTS / callback 試験を通すためだけに、送信していない DiSEqC message を成功扱いにし、入力messageをそのまま `onDiseqcMessage()` へ返すこと。
- 対象 hardware / driver が実際に提供する LNB 制御を、CTS の capability 粒度が粗いことだけを理由に全て隠すこと。

## 製品・試験上の扱い

部分 LNB 能力を公開する非互換product variantでは、CTS LNB 試験が失敗し得ることを既知差分として扱う。製品側の受け入れ確認では、backend ごとに公開した操作と列挙値が実機へ反映されること、未対応操作が成功 no-op にならないことを個別に確認する。

この既知差分は AOSP / CTS の意味論を書き換える根拠にはしない。AOSP 側に LNB の操作別 capability 表現が追加された場合、または対象 hardware / driver が CTS の基礎操作一式を実処理できるようになった場合は、本項目の前提を再評価する。
