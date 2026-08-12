from pathlib import Path

integration = Path("tis/INTEGRATION.md")
text = integration.read_text()
old = """`BootReceiver.onReceive()` は既知の起動通知を判別し、`DirectBootEpgPending` の確認と `BootEpgSyncCoordinator` への開始要求までで終了する。EPG の収集、Tuner の使用、TvProvider への反映処理の寿命を `BroadcastReceiver.onReceive()` に結びつけない。`BootEpgSyncCoordinator` は同一プロセス内で `inputId` ごとに実行中の起動時 EPG 同期を1件だけ許可する。すでに開始済みまたは実行中なら、後続の `ACTION_BOOT_COMPLETED`、動的に受信した `ACTION_USER_UNLOCKED`、開始条件の再評価からの要求を既存処理へ集約し、別の走査処理や別の Tuner 資源取得を開始しない。"""
new = """`BootReceiver.onReceive()` は既知の起動通知を判別し、`DirectBootEpgPending` を確認して、必要なら Android 標準の `JobScheduler` に固定識別子の `BootEpgSyncJobService` を登録するところまでで終了する。EPG の収集、Tuner の使用、TvProvider への反映処理は `BroadcastReceiver.onReceive()` の寿命では実行しない。`BootEpgSyncJobService` は `AndroidManifest.xml` で `android.permission.BIND_JOB_SERVICE` により保護し、利用者のロック解除後だけ実行対象にする。

起動時 EPG 同期用の `JobInfo` は再起動をまたいで永続化しない。再起動をまたぐ正本はデバイス保護領域の `DirectBootEpgPending` だけとし、再起動後は起動通知から同じジョブ登録判定を行う。ジョブ識別子は起動時 EPG 同期用に固定し、`JobScheduler.getPendingJob()` で同じジョブが登録済みなら再登録しない。`BootEpgSyncJobService.onStartJob()` はロック解除、`DirectBootEpgPending`、開始条件を再確認し、処理を開始する場合は `BootEpgSyncCoordinator` へ引き渡して `true` を返す。`BootEpgSyncCoordinator` は同一プロセス内で `inputId` ごとの起動時 EPG 同期を一度に1件だけ実行する。処理完了時は `jobFinished()` で終了を通知し、成功時は再試行を要求せず、未完了または失敗で `DirectBootEpgPending` が残る場合は再試行を要求する。`JobScheduler` が処理を中断して `onStopJob()` を呼んだ場合は進行中の走査と Tuner 資源を停止・解放し、`DirectBootEpgPending` が残る限り再試行を要求する。"""
if old not in text:
    raise SystemExit("tis/INTEGRATION.md: 置換対象が見つからない")
integration.write_text(text.replace(old, new, 1))

design = Path("tis/DESIGN_JA.md")
text = design.read_text()
old = """`BootReceiver.onReceive()` は保留状態の確認と `BootEpgSyncCoordinator` への開始要求までで終了し、EPG の収集、Tuner の使用、TvProvider への反映処理を `BroadcastReceiver` の実行時間へ結びつけない。`BootEpgSyncCoordinator` は同一プロセス内で `inputId` ごとの起動時 EPG 同期を一度に1件だけ実行する。すでに開始済みまたは実行中の `inputId` に対する後続要求は既存処理へ集約し、別の走査処理や別の Tuner 資源取得を開始しない。起動時 EPG 同期を開始できなかった場合、および TvProvider への反映が正常終了する前は `DirectBootEpgPending` を維持し、前節で定めた反映処理が正常に確定した場合だけ解除する。"""
new = """`BootReceiver.onReceive()` は保留状態を確認し、必要なら Android 標準の `JobScheduler` に固定識別子の `BootEpgSyncJobService` を登録するところまでで終了する。EPG の収集、Tuner の使用、TvProvider への反映処理は `BroadcastReceiver` の実行時間へ結びつけず、`android.permission.BIND_JOB_SERVICE` で保護した `BootEpgSyncJobService` の実行寿命下で行う。起動時 EPG 同期用の `JobInfo` は再起動をまたいで永続化せず、再起動をまたぐ正本は `DirectBootEpgPending` だけとする。`JobScheduler.getPendingJob()` で同じ固定識別子のジョブが登録済みなら再登録しない。

`BootEpgSyncJobService.onStartJob()` は利用者のロック解除、`DirectBootEpgPending`、開始条件を再確認し、処理を開始する場合は `BootEpgSyncCoordinator` へ引き渡す。`BootEpgSyncCoordinator` は同一プロセス内で `inputId` ごとの起動時 EPG 同期を一度に1件だけ実行する。処理完了時は `jobFinished()` で終了を通知し、成功時は再試行を要求しない。未完了または失敗で `DirectBootEpgPending` が残る場合、または `JobScheduler` による中断で `onStopJob()` が呼ばれた場合は、進行中の走査と Tuner 資源を停止・解放したうえで再試行を要求する。起動時 EPG 同期を開始できなかった場合、および TvProvider への反映が正常終了する前は `DirectBootEpgPending` を維持し、前節で定めた反映処理が正常に確定した場合だけ解除する。"""
if old not in text:
    raise SystemExit("tis/DESIGN_JA.md: 置換対象が見つからない")
design.write_text(text.replace(old, new, 1))
