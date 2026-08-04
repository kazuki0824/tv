from pathlib import Path
import sys

root = Path(sys.argv[1]).resolve()
path = root / 'tuner_hal/DESIGN_JA.md'
text = path.read_text(encoding='utf-8')

old_restore = '''再選局には、明確に分離した2つの確定点を設ける。段階Aでは、入力検証と未稼働状態の準備を行い、フロントエンドのトランザクションロックを取得して、旧バックエンドを停止し、旧ワーカーを静止させる。確定Aでは、旧世代を終端として一括確定し、関連付け済みdemuxと組み立て処理の境界状態を初期化する。その後、新しい選局要求をバックエンドへ送る。要求に成功した場合は、確定Bで新世代を公開し、準備済みワーカーを有効化する。新要求だけが通常の受理失敗となり、旧要求snapshotが有効で、backend停止・世代fence・demux境界終端が全て確定している場合は、準備済み状態を解放して旧要求を正確に1回だけ再投入する。復元要求が受理された場合は、新要求の元の原因別エラーを返し、復元generationを`Tuning`として公開する。復元要求も拒否されたがbackend停止と境界終端を確認できる場合だけ`Untuned`へ移す。backend停止、世代遮断、境界終端、準備資源の解放、または復元後状態を確定できない場合は表19の原因別`Failed`または`Quarantined`へ移す。確定A自体が失敗または不明の場合は旧要求を復元しない。確定Aと確定Bを1つの確定処理として記述してはならず、境界状態の初期化はバックエンド要求より前の確定Aで行う。
'''
new_restore = '''再選局は表19およびAT-001の二分岐を正とする。`Locked`で正規化settings、typed selector、LNB/power条件が同一かつbackendとstream boundaryがhealthyな場合は、確定A/Bを通らない非破壊re-entryとし、`request_sequence`更新と現lockの`LOCKED`配送予約だけを確定する。stream generation、worker、backend要求、demux境界、AVは維持する。

それ以外のfull retuneには、明確に分離した2つの確定点を設ける。段階Aでは入力検証、必要資源、局所的なbackend受付可能性、失敗回収経路、未稼働状態の準備を完了し、frontend transaction lockを取得する。確定Aでは旧backend、旧worker、旧generationを終端し、関連済みdemuxとassemblerのstream boundaryを初期化する。その後に新しい選局要求をbackendへ送る。要求成功時だけ確定Bで新generationを公開し、準備済みworkerを有効化する。

確定A後に新要求が拒否された場合は、callerが要求していない旧要求を自動再投入しない。準備済み状態を解放し、backend停止と全demux境界終端を確認できれば`Untuned`、backend結果を確定できなければ`FailedBackend`、境界終端を確定できなければ`FailedBoundary`、旧generationのfenceを成立させられなければ`Quarantined`へ遷移する。旧TSを新サービス向けdemux/filter generationへ戻す経路を設けない。確定A自体の完了可否が不明な場合も旧要求を再投入せず、表19の原因別状態へ閉じる。確定Aと確定Bを1つの確定処理として記述してはならず、stream boundary初期化はbackend要求より前の確定Aで行う。
'''
if text.count(old_restore) != 1:
    raise SystemExit(f'restore paragraph matches={text.count(old_restore)}')
text = text.replace(old_restore, new_restore, 1)

old_deadline = '''`IFrontend.tune()` はbinder thread上でlock完了まで待ち続けない。前回tune/scanのworkerをgenerationで無効化し、backendへtune requestを投入し、非同期workerが`LOCKED`または`NO_SIGNAL`を終端通知する。現行`ProductProfile.tuneTerminalDeadlineMs`は4000 msとする。これはAIDL規定値ではなく、無応答backendを有限時間で終端する製品上のwatchdogである。backendからlockまたは明示失敗が来ないまま期限へ達した場合は、現generationだけを停止し、接続demuxへのデータを遮断して`NO_SIGNAL`を正確に1回通知し、状態を`Idle`へ移す。同一generationで期限とlockが競合した場合は、期限判定前にbackendの確定済みlockを再確認し、既に観測済みの`LOCKED`を優先する。`stopTune()`、`close()`、次回`tune()`、`scan()`は該当generationをcancelし、古いworkerからの通知を捨てる。Android 14 AIDL VTSへ結び付けるprofileは、実信号で`WAIT_TIMEOUT=3秒`より前に`LOCKED`を通知できることを受入条件とし、HAL watchdogがVTS待機期限より前に`NO_SIGNAL`を確定してはならない。
'''
new_deadline = '''`IFrontend.tune()` はbinder thread上でlock完了まで待ち続けない。前回tune/scanのworkerをgenerationで無効化し、backendへtune requestを投入し、非同期workerが`LOCKED`または`NO_SIGNAL`を終端通知する。無応答backendを有限時間で終端する製品watchdogはbackend別`ProductProfile.tuneTerminalDeadlineMs`を正とし、現行profileはearth_pt1=`4000 ms`、px4=`7000 ms`とする。これはAIDL規定値ではなく、正常なbackend処理列を期限前に打ち切らないための製品値である。backendからlockまたは明示失敗が来ないまま期限へ達した場合は、現generationだけを停止し、接続demuxへのデータを遮断して`NO_SIGNAL`を正確に1回通知し、状態を`Idle`へ移す。同一generationで期限とlockが競合した場合は、期限判定前にbackendの確定済みlockを再確認し、既に観測済みの`LOCKED`を優先する。`stopTune()`、`close()`、次回`tune()`、`scan()`は該当generationをcancelし、古いworkerからの通知を捨てる。Android 14 AIDL VTSへ結び付けるprofileは、実信号でVTSの`WAIT_TIMEOUT=3秒`より前に`LOCKED`を通知できることを別の受入条件とする。VTSの待機値を製品watchdogへ流用せず、backend別deadlineを3秒へ短縮しない。
'''
if text.count(old_deadline) != 1:
    raise SystemExit(f'deadline paragraph matches={text.count(old_deadline)}')
text = text.replace(old_deadline, new_deadline, 1)

stale = [
    '旧要求を正確に1回だけ再投入する',
    '復元generationを`Tuning`として公開する',
    '現行`ProductProfile.tuneTerminalDeadlineMs`は4000 msとする',
]
for item in stale:
    if item in text:
        raise SystemExit(f'stale text remains: {item}')
required = [
    'callerが要求していない旧要求を自動再投入しない',
    '旧TSを新サービス向けdemux/filter generationへ戻す経路を設けない',
    'earth_pt1=`4000 ms`、px4=`7000 ms`',
    'VTSの待機値を製品watchdogへ流用せず',
]
for item in required:
    if item not in text:
        raise SystemExit(f'missing text: {item}')

path.write_text(text, encoding='utf-8')
