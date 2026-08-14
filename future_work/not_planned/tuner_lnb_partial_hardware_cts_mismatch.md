# Tuner LNB 部分ハードウェア能力と CTS 基礎操作の不一致

## 位置付け

この文書は、`earth_pt1` / `px4_drv` が実機・driverとして提供する LNB 制御能力と、Android CTS が non-null の `Lnb` に要求する基礎操作一式の粒度が一致しない問題について、採用理由、既知 compatibility delta、再評価条件を記録する。部分LNB能力を公開してCTS/CDD差分を受容するproduct-level判断は `開発規則.md` の「LNB部分能力公開の製品例外」を正とし、LNB の公開可否、各 operation / value の成功可否、資源寿命、失敗時遷移のTuner HAL公開契約は `tuner_hal/DESIGN_JA.md` を正とする。この文書はどちらの並行SSOTにもならない。

`開発規則.md` の決定により、本製品ではAndroid 14 CTSのLNB試験合格より、hardware / driverが実処理できることを証跡で確認したLNB operation / valueの公開を優先する。`aidl_baseline_eligible` の意味と公開gateとの関係は `tuner_hal/DESIGN_JA.md` を正とし、本書では再定義しない。

したがって、対象 hardware / driver が実処理できる LNB 制御は canonical capability 対応表に従って公開経路へ接続する。反対に、hardware / driver が実処理できない tone、satellite position、DiSEqC 等を成功 no-op、擬似成功、callback echo で実装済みに見せない。有効だが対象 backend で未対応の operation / value は、副作用なしの typed failure として `UNAVAILABLE` を返す。

## AOSP / CTS 側の事実

Android CTS の `android.media.tv.tuner.cts.TunerTest#testLnb()` は、`Tuner.openLnb(...)` が `null` を返した場合は LNB 試験を終了する。一方、non-null の `Lnb` が得られた場合は、同一 LNB に対して次を連続して要求する。

1. `setVoltage(targetLnbVoltage)` が `Tuner.RESULT_SUCCESS`
2. `setTone(Lnb.TONE_NONE)` が `Tuner.RESULT_SUCCESS`
3. `setSatellitePosition(Lnb.POSITION_A)` が `Tuner.RESULT_SUCCESS`
4. `sendDiseqcMessage(new byte[] {1, 2})`

さらに `testLnbAddAndRemoveCallback()` は同じ基礎操作を実行したうえで、`sendDiseqcMessage()` 後に `LnbCallback.onDiseqcMessage()` が呼ばれたことを確認する。

したがって、電圧制御のみ、または上記の一部だけを実処理できる LNB endpoint を non-null で公開すると、対応可能な操作自体が正しく動作していても Android 14 CTS の LNB 試験全体は合格しない。本製品はこの不一致を既知差分として受容し、部分能力の公開を理由に CTS LNB 適合を宣言しない。

参照:

- AOSP CTS `TunerTest.java`: https://android.googlesource.com/platform/cts/+/105d6f1ab8b916880af25847d71f01d5acc930e3/tests/tests/tv/src/android/media/tv/tuner/cts/TunerTest.java
- AOSP Tuner AIDL `ILnb.aidl`: https://android.googlesource.com/platform/hardware/interfaces/+/2caf529bdcf4ff02ad941f77f158b680f3a5a4dc/tv/tuner/aidl/android/hardware/tv/tuner/ILnb.aidl

## 本製品の採用方針

- `earth_pt1` / `px4_drv` について、hardware / driver の証跡で実処理可能と確認した LNB operation / value は canonical capability 対応表に従って公開経路へ接続する。
- `aidl_baseline_eligible` は CTS baseline 適合分類として保持できるが、`false` であることだけを理由に、実処理可能な operation / value を持つ LNB endpoint 全体を非公開にしない。
- 実処理できない LNB operation / value を CTS 合格目的の成功 no-op にしない。有効だが対象 backend で未対応の要求は、副作用なしで `UNAVAILABLE` とする。
- 不明な列挙値、空メッセージ、宣言済み backend 上限を超える要求など、AIDL / canonical 契約上の不正要求は `INVALID_ARGUMENT` とし、backend 未対応とは区別する。
- 各 API / 列挙値の成功可否は backend capability と公開契約の対応表で明示し、証跡のない能力を生成しない。
- 固定 LNB 給電と公開 `ILnb` 操作を混同しない。backend が固定給電だけを提供する場合と、caller が `ILnb.setVoltage()` で制御できる場合は別能力として扱う。
- `openLnbByName()` は安定した configured name mapping が canonical profile に定義されない限り `UNAVAILABLE` としてよい。これは operation / value capability の部分公開とは別契約とする。

## not planned とする内容

次は採用しない。

- CTS の `testLnb()` を通すためだけに、未対応の `setTone(TONE_NONE)` を成功扱いにすること。
- CTS の `testLnb()` を通すためだけに、未対応の `setSatellitePosition(POSITION_A)` を成功扱いにすること。
- CTS / callback 試験を通すためだけに、送信していない DiSEqC message を成功扱いにし、入力 message をそのまま `onDiseqcMessage()` へ返すこと。
- Android 14 CTS が操作別 capability 問い合わせを持たないことだけを理由に、対象 hardware / driver が実際に提供する LNB 制御まで一律に非公開化すること。
- `aidl_baseline_eligible=false` を、公開 `ILnb` endpoint 全体を隠すための product publication gate として用いること。

## 製品・試験上の扱い

部分 LNB 能力を公開する本製品では、Android 14 CTS の LNB 試験が失敗し得ることを既知差分として扱う。製品側の受け入れ確認では、backend ごとに公開した operation / value が実機へ反映されること、未対応要求が成功 no-op にならないこと、失敗時に canonical 契約どおり副作用が残らないことを個別に確認する。

この既知差分は AOSP / CTS の意味論を書き換える根拠にはしない。AOSP 側に LNB の operation 別 capability 表現が追加された場合、または対象 hardware / driver が CTS の基礎操作一式を実処理できるようになった場合は、`aidl_baseline_eligible` 分類と CTS 不一致の前提を再評価する。
