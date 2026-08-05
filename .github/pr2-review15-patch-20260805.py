from pathlib import Path


def replace_exact(path: Path, old: str, new: str) -> None:
    text = path.read_text(encoding="utf-8")
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{path}: expected exactly one match, found {count}")
    path.write_text(text.replace(old, new), encoding="utf-8")


hal = Path("tuner_hal/DESIGN_JA.md")
old_tune = """`IFrontend.tune()` はbinder thread上でlock完了まで待ち続けない。前回tune/scanのworkerをgenerationで無効化し、backendへtune requestを投入し、非同期workerが`LOCKED`または`NO_SIGNAL`を終端通知する。無応答backendを有限時間で終端する製品watchdogはbackend別`ProductProfile.tuneTerminalDeadlineMs`を正とし、現行profileはearth_pt1=`4000 ms`、px4=`7000 ms`とする。これはAIDL規定値ではなく、正常なbackend処理列を期限前に打ち切らないための製品値である。backendからlockまたは明示失敗が来ないまま期限へ達した場合は、現generationだけを停止し、接続demuxへのデータを遮断して`NO_SIGNAL`を正確に1回通知し、状態を`Idle`へ移す。同一generationで期限とlockが競合した場合は、期限判定前にbackendの確定済みlockを再確認し、既に観測済みの`LOCKED`を優先する。`stopTune()`、`close()`、次回`tune()`、`scan()`は該当generationをcancelし、古いworkerからの通知を捨てる。Android 14 AIDL VTSへ結び付けるprofileは、実信号でVTSの`WAIT_TIMEOUT=3秒`より前に`LOCKED`を通知できることを別の受入条件とする。VTSの待機値を製品watchdogへ流用せず、backend別deadlineを3秒へ短縮しない。"""
new_tune = """`IFrontend.tune()` はbinder thread上でlock完了まで待ち続けず、表19およびAT-001と同じ二分岐を正とする。前回状態が`Locked`で、正規化済みsettings、typed selector、LNB/power条件が同一であり、backendとstream boundaryの同値性・健全性を同一snapshotで証明できる場合は非破壊re-entryとする。`request_sequence`を更新し、現lockに対応する`LOCKED`を正確に1回配送するが、現stream generation、worker、backend要求、demux境界、接続filter/DVR、AV経路を維持し、旧workerまたはgenerationの無効化、backend再要求、demux boundary reset、AV中断を行わない。

旧tuneが未完了、条件が異なる、または同値性・健全性を証明できない場合だけfull retuneへ進む。prepareで新要求の検証、必要資源、callback経路、失敗回収経路を確定した後、前回tune/scanのgenerationを無効化して旧sessionを遮断し、backendへtune requestを投入し、新generationの非同期workerが`LOCKED`または`NO_SIGNAL`を終端通知する。破壊的commit後に新要求が拒否された場合は旧要求を自動再投入せず、表19の原因別状態へ遷移する。

無応答backendを有限時間で終端する製品watchdogはbackend別`ProductProfile.tuneTerminalDeadlineMs`を正とし、現行profileはearth_pt1=`4000 ms`、px4=`7000 ms`とする。これはAIDL規定値ではなく、正常なbackend処理列を期限前に打ち切らないための製品値である。backendからlockまたは明示失敗が来ないまま期限へ達した場合は、現generationだけを停止し、接続demuxへのデータを遮断して`NO_SIGNAL`を正確に1回通知し、状態を`Idle`へ移す。同一generationで期限とlockが競合した場合は、期限判定前にbackendの確定済みlockを再確認し、既に観測済みの`LOCKED`を優先する。`stopTune()`、`close()`、full retuneとなる次回`tune()`、`scan()`は該当generationをcancelし、古いworkerからの通知を捨てる。非破壊re-entryは現generationを維持する。Android 14 AIDL VTSへ結び付けるprofileは、実信号でVTSの`WAIT_TIMEOUT=3秒`より前に`LOCKED`を通知できることを別の受入条件とする。VTSの待機値を製品watchdogへ流用せず、backend別deadlineを3秒へ短縮しない。"""
replace_exact(hal, old_tune, new_tune)

hal2 = Path("tuner_hal2/DESIGN_JA.md")
old_row = """| frontend tune/scan | frontend session transaction | request検証・scan fingerprint確定 → worker/callback/rollback準備 → 旧session遮断 → 同一`LockedReported`のscan継続判定またはbackend要求 → 新generation commit | scan継続ではbackend再探索なしに新generationからENDを1回配送し、通常tune/scanは`../tuner_hal/DESIGN_JA.md`の表19と統合状態表に従う | worker、backend adapter、callback層がfrontend公開状態またはscan continuation stateを直接確定しない |"""
new_row = """| frontend tune/scan | frontend session transaction | request検証 → tuneでは同一条件・healthy snapshot判定、scanではrequest fingerprint確定 → worker/callback/rollback準備 → 非破壊tune re-entry、同一`LockedReported`のscan継続、または旧session遮断後のbackend要求・新generation commitへ分岐 | 同一健全tuneは`request_sequence`と現lockの`LOCKED`配送予約だけを確定し、現generation・worker・backend・demux境界・AVを維持する。scan継続は旧scan generationをfenceし、backend再探索なしに新callback generationからENDを1回配送する。それ以外のfull tune/scanだけが旧session遮断、backend要求、新generation commitへ進み、失敗時は`../tuner_hal/DESIGN_JA.md`の表19と統合状態表に従う | worker、backend adapter、callback層がfrontend公開状態、tune re-entry判定、またはscan continuation stateを直接確定しない |"""
replace_exact(hal2, old_row, new_row)

for path in (hal, hal2):
    text = path.read_text(encoding="utf-8")
    if text.count("```") % 2:
        raise SystemExit(f"{path}: unbalanced Markdown fences")

required = {
    hal: [
        "表19およびAT-001と同じ二分岐",
        "旧workerまたはgenerationの無効化、backend再要求、demux boundary reset、AV中断を行わない",
        "full retuneとなる次回`tune()`",
    ],
    hal2: [
        "非破壊tune re-entry",
        "同一健全tuneは`request_sequence`と現lockの`LOCKED`配送予約だけを確定",
    ],
}
for path, needles in required.items():
    text = path.read_text(encoding="utf-8")
    for needle in needles:
        if needle not in text:
            raise SystemExit(f"{path}: missing required text: {needle}")

forbidden = "前回tune/scanのworkerをgenerationで無効化し、backendへtune requestを投入し"
if forbidden in hal.read_text(encoding="utf-8"):
    raise SystemExit("stale unconditional tune description remains")
