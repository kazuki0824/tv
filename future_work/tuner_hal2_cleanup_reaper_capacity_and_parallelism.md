# Tuner HAL cleanup reaperの責務と同時実行制御を簡素化する

## 位置付け

PR #227の後続PR #230で扱う、#116由来の回収実行基盤の是正課題。**この文書は未完了作業の追跡用であり、実装済み設計・規範契約の正本ではない。** 現行仕様の正本は`TUNER_HAL_DESIGN_JA.md`、現行実装は`tuner_hal2/control/src/lib.rs`、世代付きcleanup義務は既存`ObjectCloseTxn`にある。決定した設計だけを正本へ反映し、本項目は削除する。

[PR #227の指摘](https://github.com/kazuki0824/tv/pull/227#discussion_r4504379799)に対応する。現行`max_lanes = capacity`と`ensure_lanes(pending_group_count())`はPR #116から存在し、予約group容量176から最大176本の物理workerを生成し得る。この数値の必要性は証明されていない。176が不適切だからという理由だけで、別の任意値4/8/16を適切と決めてもならない。

## 通常の実装から出発する

- **義務と権限**：既存`ObjectCloseTxn`だけがobject ID・generation・未完step・実行権限・完了／隔離を所有する。物理cleanupを同じ世代・親子関係で二重実行しない。
- **非同期受付**：キューへ渡すのは世代付きobject（または同一権限に属するグループ）の参照。予約は投入済み／実行中などschedulerに必要な情報に限定し、未完義務を複製しない。満杯時はBinder／Dropを待機させず、既存義務を保持し、既存監督から再受付する。
- **実行**：cleanup executorの並列度は、実際に互いに独立して実行可能なbackend処理、I/Oのブロック／キャンセル可能性から決める。予約容量とは結び付けない。単純な有界worker poolまたは物理backend単位の直列化を基準とし、必要な場合だけ並列性を増やす。新しい第二executor・cleanup正本は作らない。
- **監督と終了**：workerを停止不能なbackend I/O自身に監視させず、別の既存監督で期限・世代fence・報告を確認する。停止不能な処理はthread数を増やすだけでは解消しない。局所隔離と真の共有状態破損／有界再起動ポリシーを明示的に区別する。

## 必要な最小差分を決める順序

1. 現行の`ObjectCloseTxn`義務、`WorkerRuntimeReaperPending`予約group、`sync_channel`の役割が重複する箇所を特定する。必要な親子委譲や実行権限まで削除しない。
2. 実際のbackend cleanup呼出しの同期性、共通ロック、ブロックの最大時間、停止・キャンセル可能性を確認する。**並列度の初期値は、独立した同時I/Oが必要な単位から決める**。既存値176や任意値4をデフォルトの根拠にしない。
3. 少ない同時実行数のpoolが無応答workerで満杯になったときの別ownerの進行、期限監督、局所隔離／HAL再起動を確認する。workerを無制限に増員する代替案は採用しない。
4. 容量1と製品容量176の予約、親子ownerの同一group委譲、世代再利用、group返却遅延、報告未着・worker永久停止・backend失陥の回帰を維持する。キュー満杯をサービス全体破損と同一視しない。
5. 動作可能な最小実装を`tuner_hal2/control/src/lib.rs`等の既存責務内で変更し、`TUNER_HAL_DESIGN_JA.md`・`tuner_hal2/CHANGELOG.md`を必要な範囲だけ更新する。Host CIとAndroid Soong/atestの証跡を区別する。

## 完了条件

完成時に説明できること：予約容量／実行数をそれぞれ何のために制御するか、なぜ別正本が不要か、数値の根拠、停止不能I/Oで他ownerへ及ぶ影響、AIDL close・generation fence・再起動時の振舞い。根拠のない上限値を新しい固定値へ置き換えただけでは完了としない。

**現段階**：不適切な`max_lanes = min(capacity, 4)`という試行は取り消し済み。現行の動作を保持したまま自然な構成を先に整理している。設計・実装の変更が確定したら本項目を削除し、正本に移す。
