from pathlib import Path

ROOT = Path.cwd()


def replace_exact(path: Path, old: str, new: str) -> None:
    text = path.read_text(encoding="utf-8")
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{path}: expected exactly one match, found {count}")
    path.write_text(text.replace(old, new), encoding="utf-8")


hal = ROOT / "tuner_hal/DESIGN_JA.md"
old_scan = """`IFrontend.scan()` は、同一条件の再 scan であっても成功扱いの無処理 にしない。AOSP 契約に従い、未完了の scan がある場合は既存 scan generation を停止し、新しい scan generation を開始する。既存 scan の callback から来る古い terminal event は generation mismatch として捨てる。"""
new_scan = """`IFrontend.scan()` は、同一条件の再 scan であっても成功扱いの無処理にしない。対象LineageOS 21 / Android 14 VTSは、最初の `scan(K)` で `LOCKED` を受け取ると同じsettingsとscan typeで `scan(K)` を再度呼び、その後の `END` を待つ。この継続契約を満たすため、frontend scan sessionは `Idle`、`Running(generation, request_fingerprint)`、`LockedReported(generation, request_fingerprint)` を区別する。`request_fingerprint` は正規化済み `FrontendSettings` と `FrontendScanType` から決定し、object identityやdriver固有表現へ依存させない。

`Running(g, K)` で `LOCKED` のcallback配送が成功した場合は `LockedReported(g, K)`へ確定する。同じKで次の `scan()` が呼ばれた場合も、AOSP契約どおり旧generationを先に終端し、新しいcallback generationを発行する。ただし、同一要求について既にlock報告済みであるためbackendを再探索せず、新generationから `END` を正確に1回配送する。これは新generationとterminal callbackを持つ継続stepであり、成功扱いの無処理ではない。旧generationから遅延到着したcallbackはgeneration mismatchとして捨てる。

異なるrequest fingerprintの `scan()`、`stopScan()`、`tune()`、`close()`では `LockedReported` を破棄する。異なるrequestは通常の新scanとしてbackend探索を開始する。同一requestの継続で `END` 配送が失敗した場合は、scanのterminal reasonとend delivery outcomeを分離する既存契約に従い、backend再探索または二重 `LOCKED` で補償しない。最低試験は `scan(K) → LOCKED(g1) → scan(K) → END(g2)` を満たし、2回目にbackend探索と再度の `LOCKED` がないこと、`scan(K2)`、`stopScan()`、`tune()`、`close()`で継続状態が失効することを確認する。"""
replace_exact(hal, old_scan, new_scan)

old_at008 = """| AT-008 | `IFrontend.scan()` / `stopScan()` | 入力検証・worker枠とcallback経路の準備・旧scan世代の終端・新世代の確定 | backend受理、新世代、worker、callback許可を一括で公開した時点 | 旧世代終端前は状態不変。終端後は旧scanを復元しない | frontend、scan worker、callback経路 | scan終了理由とcallback配送結果の規則に従う | scan終了理由とEND通知結果を分離する |"""
new_at008 = """| AT-008 | `IFrontend.scan()` / `stopScan()` | 入力検証・request fingerprint確定・worker/callback経路準備 → 旧scan世代終端 → 同一`LockedReported`なら新generationのEND step、それ以外はbackend要求と新scan世代確定 | 同一lock報告済みrequestの継続は新generationとEND配送権限を一括で公開した時点。通常scanはbackend受理、新世代、worker、callback許可を一括で公開した時点 | 旧世代終端前は状態不変。終端後は旧scanを復元しない。同一request継続のEND失敗をbackend再探索または二重LOCKEDで補償しない | frontend、scan worker、callback経路、scan continuation state | scan終了理由とcallback配送結果の規則に従う | `scan(K)→LOCKED→scan(K)→END`を新旧generationのfence付きで成立させ、異なるrequest・stopScan・tune・closeで継続状態を破棄する |"""
replace_exact(hal, old_at008, new_at008)

hal2 = ROOT / "tuner_hal2/DESIGN_JA.md"
old_contract = """| frontend tune/scan | frontend session transaction | request検証 → worker/callback/rollback準備 → 旧session遮断 → backend要求 → 新generation commit | `../tuner_hal/DESIGN_JA.md`の表19と統合状態表に従う | worker、backend adapter、callback層がfrontend公開状態を直接確定しない |"""
new_contract = """| frontend tune/scan | frontend session transaction | request検証・scan fingerprint確定 → worker/callback/rollback準備 → 旧session遮断 → 同一`LockedReported`のscan継続判定またはbackend要求 → 新generation commit | scan継続ではbackend再探索なしに新generationからENDを1回配送し、通常tune/scanは`../tuner_hal/DESIGN_JA.md`の表19と統合状態表に従う | worker、backend adapter、callback層がfrontend公開状態またはscan continuation stateを直接確定しない |"""
replace_exact(hal2, old_contract, new_contract)

old_migration = """| frontend tune/scan、再選局、終端deadline | 設計済み・実装未適用 | `service_runtime/`、`device/`、callback配送 | AOSP callback契約を満たす終端、安定同一条件の非破壊re-entry、full retuneでの旧session遮断、破壊的commit後に旧要求を再投入しないこと、旧TSが新demux/filter世代へ混入しないこと、原因別の`Untuned`／`FailedBackend`／`FailedBoundary`／`Quarantined`遷移、deadlineの試験が合格 |"""
new_migration = """| frontend tune/scan、再選局、終端deadline | 設計済み・実装未適用 | `service_runtime/`、`device/`、callback配送 | AOSP callback契約を満たす終端、`scan(K)→LOCKED(g1)→scan(K)→END(g2)`で2回目のbackend探索・LOCKED再配送がないこと、異なるscan request・stopScan・tune・closeで継続状態が失効すること、安定同一条件の非破壊re-entry、full retuneでの旧session遮断、破壊的commit後に旧要求を再投入しないこと、旧TSが新demux/filter世代へ混入しないこと、原因別の`Untuned`／`FailedBackend`／`FailedBoundary`／`Quarantined`遷移、deadlineの試験が合格 |"""
replace_exact(hal2, old_migration, new_migration)

for path in (hal, hal2):
    text = path.read_text(encoding="utf-8")
    if text.count("```") % 2:
        raise SystemExit(f"{path}: unbalanced Markdown fences")

required = {
    hal: [
        "LockedReported(generation, request_fingerprint)",
        "scan(K) → LOCKED(g1) → scan(K) → END(g2)",
        "2回目にbackend探索と再度の `LOCKED` がない",
    ],
    hal2: [
        "同一`LockedReported`のscan継続判定",
        "scan(K)→LOCKED(g1)→scan(K)→END(g2)",
    ],
}
for path, needles in required.items():
    text = path.read_text(encoding="utf-8")
    for needle in needles:
        if needle not in text:
            raise SystemExit(f"{path}: missing required text: {needle}")

if "同一条件の再 scan であっても成功扱いの無処理 にしない" in hal.read_text(encoding="utf-8"):
    raise SystemExit("stale scan text remains")
