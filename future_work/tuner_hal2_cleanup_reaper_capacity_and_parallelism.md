# Tuner HAL cleanup reaperの責務と同時実行制御を簡素化する

## 位置付け

PR #227の後続PR #230で扱う、#116由来の回収実行基盤の是正課題。**この文書は未完了作業の追跡用であり、実装済み設計・規範契約の正本ではない。** 現行仕様の正本は`TUNER_HAL_DESIGN_JA.md`、現行実装は`tuner_hal2/control/src/lib.rs`、世代付きcleanup義務は既存`ObjectCloseTxn`にある。決定した設計だけを正本へ反映し、本項目は削除する。

[PR #227の指摘](https://github.com/kazuki0824/tv/pull/227#discussion_r4504379799)に対応する。現行`max_lanes = capacity`と`ensure_lanes(pending_group_count())`はPR #116から存在し、予約group容量176から最大176本の物理workerを生成し得る。この数値の必要性は証明されていない。176が不適切だからという理由だけで、別の任意値4/8/16を適切と決めてもならない。

## 通常の実装から出発する

- **義務と権限**：既存`ObjectCloseTxn`だけがobject ID・generation・未完step・実行権限・完了／隔離を所有する。物理cleanupを同じ世代・親子関係で二重実行しない。
- **非同期受付**：キューへ渡すのは世代付きobject（または同一権限に属するグループ）の参照。予約は投入済み／実行中などschedulerに必要な情報に限定し、未完義務を複製しない。満杯時はBinder／Dropを待機させず、既存義務を保持し、既存監督から再受付する。
- **実行**：cleanup executorの並列度は、実際に互いに独立して実行可能なbackend処理、I/Oのブロック／キャンセル可能性から決める。予約容量とは結び付けない。単純な有界worker poolまたは物理backend単位の直列化を基準とし、必要な場合だけ並列性を増やす。新しい第二executor・cleanup正本は作らない。
- **監督と終了**：workerを停止不能なbackend I/O自身に監視させず、別の既存監督で期限・世代fence・報告を確認する。停止不能な処理はthread数を増やすだけでは解消しない。局所隔離と真の共有状態破損／有界再起動ポリシーを明示的に区別する。

## 周辺実装で確認した是正対象

下表は現行`tuner_hal2/control/src/lib.rs`の具体的なコード構造に基づく対応アイテム。**事実として確認した構造**と、**影響範囲が未証明の疑義**を区別する。どの項目も新しいcleanup正本や第二のexecutorを追加する理由にはしない。

| 対応 | 根拠となる現行実装 | 是正内容・試験条件 |
|---|---|---|
| **R1：予約数連動のworker増員を解消** | `WorkerRuntimeReaperQueue::start()`は`max_lanes = capacity`、`enqueue_with_reservation()`は`pending_group_count()`を`ensure_lanes()`へ渡す。予約しただけのgroupや、処理後に解放を待つgroupも増員理由となる。 | 実際に同時実行できるbackend I/O単位から実行上限を決め、待機予約数とスレッド数を結合しない。予約176でもスレッドを無条件に176本作らない。根拠のない固定4本への置換もしない。 |
| **R2：追加worker生成失敗で受付全体を拒否しない** | `enqueue_with_reservation()`は`ensure_lanes(demanded)`失敗時、既存laneが正常でキューに空きがあっても予約を返却して`Err`を返す。 | 正常な既存executorで進行可能な依頼は受付できる構造を優先する。追加スレッドを作れない注入下で、既存workerによる処理・予約解放・未完義務保持を検証。物理実行が必須なのに全workerが失陥した場合とは区別する。 |
| **R3：予約groupと`sync_channel`の二重容量を整理** | `WorkerRuntimeReaperPending::reserve_group()`が`groups.len() < capacity`を管理し、別途`sync_channel(capacity)`も同じ値で`Full`を判定する。投入済みjobが正規groupを1つ占有する前提では、二つ目の`Full`は独立した通常の容量上限として機能しないはずである。 | 「1予約groupにつきキュー投入jobは1つ」の対応を全経路で確認し、十分なら独立した二重上限・二重`Busy`判定を減らす。例外的に`Full`が到達可能なら、予約・キューの不変条件と試験を明記する。 |
| **R4：予約権限の取りこぼしを防ぐ** | `WorkerRuntimeReaperReservation`は`#[must_use]`を付けるが`Drop`実装がなく、`transfer(self)`も空処理。`reserve_pending()`で取得した未投入tokenがスコープから消えても、pendingの予約groupは自動解放されない。 | 予約から投入・取消までを単一の所有権移管として保証する。RAIIによる取消、または予約・投入の原子的操作等のより小さい案を選ぶ。失敗・早期return・panic時も、未投入予約を失効させず無期限占有しないことをテストする。 |
| **R5：同一予約内の重複キーを拒否する** | `reserve_group(reservations)`の重複検査は既存`state.entries`との照合だけで、同じ呼出し内の重複を検査しない。`[(key, v1), (key, v2)]`を渡すと、`entries`では後者が上書きされ、`groups`には重複キーが残る。 | 同一バッチ内の重複を副作用なしで拒否するか、APIを一意キーしか受けない形にする。重複バッチの拒否、失敗時に既存予約が不変であることを回帰に追加する。 |
| **R6：group数だけでなく保持キー数の境界を確定** | `reserve_group()`の`capacity`はgroup数のみを制限し、`attach_to_group()`は同じgroupへ世代付きキーを追加する際にキー総数の上限を検査しない。 | parent/child・旧世代の委譲を含む最大保持数が既存のobject生存制限で確実に有界となるか証明する。無上限となり得るなら、第二義務台帳を作らず正規ownerの論理上限／重複排除に従って制約する。 |
| **R7：単一worker終端を無条件に共有pool失陥としない** | `WorkerReaperFailureState::record_terminal()`は任意のlaneの`RuntimeFailure`・`PanicOrJoinFailure`を共有`terminal_failure`に記録し、`check_receiver()`は以後すべての予約操作を失敗させる。 | receiverロック汚染・共有台帳破損と、特定groupの実行異常を型付きで区別する。局所generation fence・隔離・未完義務保持が成立する場合に他ownerの進行を維持できるか検証する。**失敗した物理I/Oの影響が遮断できない場合のfail-stopは維持**し、全worker失陥を単純なretryで成功扱いしない。 |

### 是正時の優先順位

R2・R4・R5は、予約／投入の異常経路や入力検証の**具体的な是正候補**として先に処理する。R1・R3は実行資源と受付上限の**構造的な整理**、R6・R7は対象の影響範囲を証明してから必要な変更を決める。R7を含め、共有状態の整合性を証明できないケースでは既存のfail-closed契約を優先する。

**PR #227との境界**：既存の`ServiceCritical`判定期限、Binder再接続、実CTS/VTS未検証は#227の未解決事項として維持し、このPRへ複製しない。#230ではreaper自体が必要以上に全体失陥へ昇格するかを、共有executorの責務範囲に限定して扱う。

## 必要な最小差分を決める順序

1. 現行の`ObjectCloseTxn`義務、`WorkerRuntimeReaperPending`予約group、`sync_channel`の役割が重複する箇所を特定する。必要な親子委譲や実行権限まで削除しない。
2. 実際のbackend cleanup呼出しの同期性、共通ロック、ブロックの最大時間、停止・キャンセル可能性を確認する。**並列度の初期値は、独立した同時I/Oが必要な単位から決める**。既存値176や任意値4をデフォルトの根拠にしない。
3. 少ない同時実行数のpoolが無応答workerで満杯になったときの別ownerの進行、期限監督、局所隔離／HAL再起動を確認する。workerを無制限に増員する代替案は採用しない。
4. 容量1と製品容量176の予約、親子ownerの同一group委譲、世代再利用、group返却遅延、報告未着・worker永久停止・backend失陥の回帰を維持する。キュー満杯をサービス全体破損と同一視しない。
5. 動作可能な最小実装を`tuner_hal2/control/src/lib.rs`等の既存責務内で変更し、`TUNER_HAL_DESIGN_JA.md`・`tuner_hal2/CHANGELOG.md`を必要な範囲だけ更新する。Host CIとAndroid Soong/atestの証跡を区別する。

## 完了条件

完成時に説明できること：予約容量／実行数をそれぞれ何のために制御するか、なぜ別正本が不要か、数値の根拠、停止不能I/Oで他ownerへ及ぶ影響、AIDL close・generation fence・再起動時の振舞い。根拠のない上限値を新しい固定値へ置き換えただけでは完了としない。

**現段階**：不適切な`max_lanes = min(capacity, 4)`という試行は取り消し済み。現行の動作を保持したまま自然な構成を先に整理している。設計・実装の変更が確定したら本項目を削除し、正本に移す。
