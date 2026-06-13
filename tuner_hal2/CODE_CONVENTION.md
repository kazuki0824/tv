# Tuner HAL2 コーディング規則

この文書は `tuner_hal2` 固有の実装規約だけを書く。公開契約、状態遷移、戻り値、資源寿命、WorkerExit / WorkerFailureClassifier / ScanSessionTxn 論理契約は既存 `tuner_hal/DESIGN_JA.md` と `tuner_hal2/DESIGN_JA.md` の構造差分を正とする。

- 状態遷移、終了分類、失敗分類に自由文字列を使わない。
- 公開API相当の成功条件は、`validate -> reserve -> prepare -> apply -> commit` の各段階へ分ける。
- commit前失敗は必ずrollbackまたはquarantineへ接続する。
- cleanup、stop、join、callback、rollback の失敗を `let _ =` で捨ててはならない。
- Dropは公開closeの代替にしない。
- テストは公開関数、戻り値、状態、診断を直接検査し、同じソースファイルの文字列検索で完了判定しない。
