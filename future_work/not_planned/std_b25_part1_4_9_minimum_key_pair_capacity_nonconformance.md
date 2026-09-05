# STD-B25 Part 1 §4.9 最小鍵組容量の恒久非適合

## 位置付け

本書は、ARIB STD-B25 Part 1 §4.9 が受信機システムに要求する最小8鍵組容量について、本製品が採用している恒久的な非適合方針を `future_work/not_planned` の既知差分として記録する。

この文書は現行の規範値、Tuner HAL公開契約、capability、資源寿命、戻り値、状態遷移を独立に定義しない。製品レベルの規範判断は `開発規則.md` の「STD-B25 Part 1 §4.9 最小鍵組容量の製品適合方針」、Tuner HAL内部の `StdB25DecodeCapability`、`DescramblerCapacityPool`、受付・解放・失敗時契約は `tuner_hal/DESIGN_JA.md` を正とする。

## 既知の非適合

STD-B25 Part 1 §4.9 の受信機システム最小8鍵組容量は、本製品全体として恒久的に適合対象外とする。

したがって、本製品について次を宣言してはならない。

- STD-B25 Part 1 §4.9 への適合。
- 実鍵組数または実PID数だけを根拠とする Part 1 CAS-R 全体への適合。
- 限定した `StdB25DecodeCapability` を根拠とする STD-B25 全面準拠。

本製品が個別の物理tuner/backend復号経路について実際に成立させる TS payload decode 能力は、`tuner_hal/DESIGN_JA.md` の `StdB25DecodeCapability` に記録する限定された製品能力として扱い、§4.9 適合へ読み替えない。

## AOSP / Tuner HAL 公開境界との関係

Android Tuner AIDL の `DemuxCapabilities` には STD-B25 の同時鍵組数を公開する標準fieldがなく、`IDescrambler` の公開契約と §4.9 の受信機システム容量要求は同一契約ではない。

このため、本件を解消するために frozen AIDL へ vendor 独自fieldを追加したり、実際には存在しない鍵容量を capability として捏造したりしてはならない。Tuner HAL は実際の `StdB25DecodeCapability` と共有 `DescramblerCapacityPool` に基づき、実鍵組数・実PID数・pool共有単位を超える要求を `UNAVAILABLE` として拒否する現行契約を維持する。

AOSP/VTS契約を満たすために本件のARIB非適合を隠蔽してはならず、逆に本件のARIB非適合を理由としてAOSP公開契約を変更してはならない。

## 非採用理由

本製品では、対象hardware/backendで実証できる同時鍵組容量を超えて、STD-B25 Part 1 §4.9 の最小8鍵組容量を製品要件として保証しない方針を採用している。

実容量を超える仮想slot、成功no-op、既存slotの危険な再利用、別session間の鍵資源共有によって8鍵組相当を装うことは、実資源と公開・内部状態を乖離させるため採用しない。

## 再評価条件

次のいずれかが成立した場合は、この既知非適合を再評価する。

- 対象となる全ての製品復号経路で、STD-B25 Part 1 §4.9 が要求する最小8鍵組容量を実資源として保証できるようになった場合。
- 製品アーキテクチャの変更により、受信機システムとして同条項を満たす別の正当な資源構成が成立した場合。
- 適用するARIB規格の要求が変更され、本件の前提が変化した場合。

再評価で適合方針を変更する場合は、まず `開発規則.md` の製品レベル方針を更新し、必要に応じて `tuner_hal/DESIGN_JA.md` の能力・資源契約を同一変更で更新する。本書だけを変更して現行規範を変更した扱いにしてはならない。

## 監査上の扱い

本件は意図的に受容している既知のARIB compatibility deltaであり、現行設計の未知欠陥として扱わない。ただし、本ファイルへの記録は適合そのものを意味しない。

「`future_work` に記載された既知差分を除く」という条件で設計監査を行う場合に限り、本件を既知除外事項として扱う。STD-B25またはARIBへの全面適合を評価する監査では、本件を除外せず、§4.9非適合として明示する。
