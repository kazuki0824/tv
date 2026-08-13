#!/bin/bash
set -euo pipefail
cat .tmp/pr28-diff.parts/* > /tmp/pr28.patch
patch --batch --forward --fuzz=3 -p0 < /tmp/pr28.patch
python3 - <<'PY'
from pathlib import Path

pub_path = Path('tuner_hal/DESIGN_JA.md')
impl_path = Path('tuner_hal2/DESIGN_JA.md')
pub = pub_path.read_text(encoding='utf-8')
impl = impl_path.read_text(encoding='utf-8')

pub_lines = pub.splitlines()
out = []
for line in pub_lines:
    if line.startswith('| Filter `flush()` | `FilterFlushTxn` |'):
        out.append('| Filter / DVR `flush()` cleanup orchestration | `QueueCleanupTxn` | Filter/DVR固有stateを所有せず、公開`flush()`のcleanup対象調停・typed下位protocol呼出し・失敗集約だけを共通化 | `FilterProducerDrainGate` / `QueueEpochProtocol`内部状態の直接所有、API別cleanup orchestrationの複製 |')
        continue
    if line.startswith('| DVR `flush()` | `DvrFlushTxn` |'):
        continue
    if line.startswith('| worker infrastructure failure | `WorkerFailureClassifier` |'):
        out.append('| worker failure classification | `WorkerFailureClassifier` | stop/wake/join/EventFlag/Reaper/backend-control/callback等の失敗種別を共通typed分類し、ownerへ分類結果だけを返す | 停止順序、retry/cleanup、公開状態遷移の所有、API/worker別の文字列・errno再分類 |')
        continue
    if line.startswith('| `FilterProducerDrainGate` |'):
        out.append('| `FilterProducerDrainGate` | `demux/src/runtime/queue_runtime.rs` | Filter/SharedFilter producer、`QueueCleanupTxn`からのtyped drain request | permit、admitted count、Draining gate | FMQ内容、DVR token、flush全体 | admit → short nonblocking commit section → release / drain | panic/returnでもpermit解放、遮断不能だけFilter fail | data producer、`QueueCleanupTxn` | Binder callback/IO/joinをpermit内に保持、orchestratorの内部state直接変更 | drain race、panic/drop、bounded completion、共通orchestratorからのdrain |')
        continue
    if line.startswith('| `QueueEpochProtocol` |'):
        out.append('| `QueueEpochProtocol` | `demux/src/runtime/queue_runtime.rs` | DVR data path、`QueueCleanupTxn`からのtyped flush request | queue identity、epoch、read/write token、active count | Filter producer、DVR parser/stats、flush orchestration | begin → commit/cancel/drop → drain → epoch prepare/commit | stale token拒否、commit前失敗はepoch/position不変 | DVR data path、`QueueCleanupTxn` | Filter path、API別token state machine、orchestratorの内部state直接変更 | begin/commit/cancel/drop、flush race、stale token、identity ABA |')
        continue
    if line.startswith('| `FilterFlushTxn` |'):
        out.append('| `QueueCleanupTxn` | `service_runtime/src/queue_cleanup_txn.rs::QueueCleanupTxn` | Filter / DVR `flush()` use-case | cleanup orchestration plan、typed下位protocol呼出順序、共通失敗集約/result composition | Filter producer permit/state、DVR queue token/epoch、API固有eligibility/公開状態 | API ownerが対象確定 → typed drain/cleanup request → 全対象結果集約 → API ownerへtyped result返却 | 下位protocol失敗を成功へ丸めず全対象を試行し、API固有state transitionは各ownerへ返す | Filter/DVR flush use-case | 下位protocol内部stateの直接変更、non-flush API、API別orchestration複製 | Filter/DVR双方が同じorchestratorを通る、下位state独立、partial cleanup failure、result aggregation |')
        continue
    if line.startswith('| `DvrFlushTxn` |'):
        continue
    if line.startswith('| `WorkerFailureClassifier` |'):
        out.append('| `WorkerFailureClassifier` | `service_runtime/src/worker_failure_classifier.rs` | worker owner / cleanup managerからのtyped failure | stop/wake/join/EventFlag/Reaper/backend-control/callback等の失敗種別分類だけ | worker lifecycle、停止順序、retry/cleanup、quarantine、公開状態遷移 | typed/raw failure受理 → source/domainをtyped分類 → ownerへ分類結果返却 | 文字列推測・API別errno推測を禁止し、unknownもtyped分類として返す。分類器自身はstate mutationしない | worker owner、cleanup manager、callback/backend failureを扱うowner | classifierからdomain/public stateを直接変更すること、owner側で同型分類を再実装すること | stop/wake/join/EventFlag/Reaper/backend-control/callback分類、owner間同一分類、state不変 |')
        continue
    if line.startswith('ワーカー基盤の異常は`WorkerPanic`'):
        out.append('ワーカー関連の失敗種別は`WorkerFailureClassifier`だけがtyped分類する。対象にはstop/wake/join/EventFlag/Reaper/backend-control/callback等の発生源を含めるが、分類器が所有するのは分類結果だけであり、停止順序、retry、cleanup、quarantine、公開状態遷移は各worker owner/API契約に残す。FMQ payload commit後のEventFlag起床失敗についても、payload保持・再起床というdata-path状態機械はqueue runtimeが所有し、classifierは失敗種別を分類するだけとする。')
        continue
    if line.startswith('| AT-004 | Filter / DVR configure・start・flush |'):
        out.append('| AT-004 | Filter / DVR configure・start・flush | configure/startは各domain transaction、flushは`QueueCleanupTxn`が共通orchestrationと失敗集約を行い、Filter固有stateは`FilterProducerDrainGate`、DVR固有stateは`QueueEpochProtocol`へtyped委譲 | API別状態と対象queue/parser generationまたは実行状態を各API ownerの確定点で公開した時点 | commit前は状態不変。共通orchestratorは下位stateを二重所有しない | 当該FilterまたはDVR | 表1、表2、表6-A、0-S-3Bに従う | API別にcleanup orchestrationを複製せず、異なる下位state machineを統合しない |')
        continue
    if line.startswith('worker lifecycleの共通mechanismは既存`WorkerRuntime`'):
        out.append('worker lifecycleの共通mechanismは既存`WorkerRuntime` / `WorkerHandle`を唯一のownerとする。owner generation、stop predicate/signal、wake/cancel、join、generation fence、Reaper handoffだけを共通化し、Frontend/Filter/DVR/Playback固有のstart/stop state machineやbackend意味論を所有しない。別のgeneric `WorkerLifecycleProtocol`を設けない。失敗種別は`WorkerFailureClassifier`だけがstop/wake/join/EventFlag/Reaper/backend-control/callback等をtyped分類し、停止順序、retry/cleanup、公開状態遷移は各worker owner/API側が所有する。')
        continue
    if line.startswith('`FilterProducerDrainGate`はproducer admission/drainだけを所有する下位protocol'):
        out.append('`FilterProducerDrainGate`はproducer admission/drainだけを所有する下位protocolであり、Filter `flush()`全体のstate ownerではない。公開`flush()`の共通cleanup orchestrationと失敗集約は`QueueCleanupTxn`が担い、Filter固有stateの変更は同gateのtyped入口だけを通す。')
        continue
    if line.startswith('`QueueEpochProtocol`はDVR queue identity/epochとread/write tokenの下位protocol'):
        out.append('`QueueEpochProtocol`はDVR queue identity/epochとread/write tokenを所有する下位protocolであり、DVR `flush()`全体の共通orchestration ownerではない。公開`flush()`の共通cleanup orchestrationと失敗集約は`QueueCleanupTxn`が担い、DVR固有stateの変更は同protocolのtyped入口だけを通す。Filter側のproducer stateと統合しない。')
        continue
    out.append(line)
pub = '\n'.join(out) + ('\n' if pub.endswith('\n') else '')

impl_lines = impl.splitlines()
out = []
for line in impl_lines:
    if line.startswith('| Filter flush | `FilterFlushTxn` |'):
        out.append('| Filter / DVR flush cleanup orchestration | `QueueCleanupTxn` | Filter/DVR固有stateを所有せず、typed下位protocol呼出しと失敗集約だけを共通化 | API別orchestration複製、下位protocol内部stateの直接変更 |')
        continue
    if line.startswith('| DVR flush | `DvrFlushTxn` |'):
        continue
    if line.startswith('| worker infrastructure failure classification | `WorkerFailureClassifier` |'):
        out.append('| worker failure classification | `WorkerFailureClassifier` | stop/wake/join/EventFlag/Reaper/backend-control/callback等の生の失敗を共通typed分類する | 停止順序、retry/cleanup、公開状態遷移の所有、API別再分類 |')
        continue
    if line.startswith('| `FilterProducerDrainGate` |'):
        out.append('| `FilterProducerDrainGate` | 既存`demux/src/runtime/queue_runtime.rs` | Filter/SharedFilter data pathのproducer admission/finishと`QueueCleanupTxn`からのtyped drain入口 | `QueueCleanupTxn`がgate内部状態を直接変更する、DVR stateを吸収する |')
        continue
    if line.startswith('| `QueueEpochProtocol` |'):
        out.append('| `QueueEpochProtocol` | 既存`demux/src/runtime/queue_runtime.rs` | DVR data pathのbegin/commit/cancelと`QueueCleanupTxn`からのtyped flush入口 | `QueueCleanupTxn`がqueue token/epoch内部状態を直接変更する、Filter stateを吸収する |')
        continue
    if line.startswith('| `FilterFlushTxn` |'):
        out.append('| `QueueCleanupTxn` | 既存`service_runtime/src/queue_cleanup_txn.rs::QueueCleanupTxn`。Filter/DVR固有stateはそれぞれ`FilterProducerDrainGate` / `QueueEpochProtocol`が所有する | Filter/DVR `flush()` object use-case | 下位protocol内部stateを直接変更する、API別に同じorchestration/failure aggregationを再実装する |')
        continue
    if line.startswith('| `DvrFlushTxn` |'):
        continue
    if line.startswith('| `WorkerFailureClassifier` |'):
        out.append('| `WorkerFailureClassifier` | 既存`service_runtime/src/worker_failure_classifier.rs` | worker owner / cleanup manager / callback・backend失敗を扱うownerからtyped failureを入力し、分類結果だけを返す | classifierが停止順序、retry/cleanup、quarantine、公開/domain state transitionを直接変更する、owner側が文字列/errnoで再分類する |')
        continue
    if line.startswith('- `WorkerFailureClassifier`はworker infrastructure failureだけを分類する。'):
        out.append('- `WorkerFailureClassifier`はstop/wake/join/EventFlag/Reaper/backend-control/callback等の失敗種別のtyped分類だけを共通化する。停止順序、retry/cleanup、quarantine、公開状態遷移は各worker owner/API側に残す。FMQ data通知EventFlagのpayload保持・再起床state machineはqueue runtimeが所有し、classifierはそのfailure categoryだけを返す。')
        continue
    if line.startswith('- `FilterFlushTxn`は`FilterProducerDrainGate`'):
        out.append('- Filter/DVR `flush()`は`QueueCleanupTxn`を共通orchestratorとして使用し、cleanup対象調停と失敗集約だけを共通化する。Filter固有stateは`FilterProducerDrainGate`、DVR固有stateは`QueueEpochProtocol`が独立して所有し、`QueueCleanupTxn`はtyped入口だけを使用する。')
        continue
    if line.startswith('- `QueueCleanupTxn`をFilter/DVR共通flush transaction authorityとして置かない。'):
        out.append('- Filter/DVR `flush()`のcleanup orchestrationと失敗集約をAPI別に複製せず、`QueueCleanupTxn`のtyped入口を使用する。')
        continue
    if line.startswith('- `WorkerFailureClassifier`へbackend/device/callback/FMQ data通知EventFlag failureを入力しない。'):
        out.append('- worker owner/APIがstop/wake/join/EventFlag/Reaper/backend-control/callback等の同型失敗分類を個別実装せず、`WorkerFailureClassifier`のtyped結果を使用する。')
        continue
    out.append(line)
impl = '\n'.join(out) + ('\n' if impl.endswith('\n') else '')

for name, text in [('public', pub), ('implementation', impl)]:
    assert 'FilterFlushTxn' not in text, (name, 'FilterFlushTxn remains')
    assert 'DvrFlushTxn' not in text, (name, 'DvrFlushTxn remains')
    assert '`QueueCleanupTxn`' in text, (name, 'QueueCleanupTxn missing')
    assert '`WorkerFailureClassifier`' in text, (name, 'WorkerFailureClassifier missing')

assert '#### 0-S-3B. 共通部品の規範定義' in pub
assert '| Filter / DVR `flush()` cleanup orchestration | `QueueCleanupTxn` |' in pub
assert '| `QueueCleanupTxn` | `service_runtime/src/queue_cleanup_txn.rs::QueueCleanupTxn` |' in pub
assert 'stop/wake/join/EventFlag/Reaper/backend-control/callback等の失敗種別分類だけ' in pub
assert '| Filter / DVR flush cleanup orchestration | `QueueCleanupTxn` |' in impl
assert '| `QueueCleanupTxn` | 既存`service_runtime/src/queue_cleanup_txn.rs::QueueCleanupTxn`' in impl
assert 'stop/wake/join/EventFlag/Reaper/backend-control/callback等の失敗種別のtyped分類だけ' in impl

rows=[]; on=False
hdr='| 論理契約名 | 実装正本 | 公開入口 | 所有する状態 | 所有しない状態 | phase order | 失敗時処理 | 呼び出し許可層 | 呼び出し禁止層 | 最低テスト |'
for line in pub.splitlines():
    if line == hdr:
        on=True
        continue
    if on:
        if line.startswith('|---'):
            continue
        if not line.startswith('|'):
            break
        cells=[c.strip() for c in line.strip().strip('|').split('|')]
        assert len(cells)==10 and all(cells), line
        rows.append(cells)
assert len(rows)==19, len(rows)

pub_path.write_text(pub, encoding='utf-8')
impl_path.write_text(impl, encoding='utf-8')
PY

git diff --check
rm -f .tmp/pr28-final.patch.gz.b64 .tmp/pr28-runner.sh .github/workflows/pr28-canonical-commonization-v3.yml
rm -rf .tmp/pr28-diff.parts .tmp/pr28-patch.parts

git config user.name github-actions[bot]
git config user.email 41898282+github-actions[bot]@users.noreply.github.com
git add -A tuner_hal/DESIGN_JA.md tuner_hal2/DESIGN_JA.md .tmp .github/workflows/pr28-canonical-commonization-v3.yml
git diff --cached --check
git commit -m 'design: integrate reviewed commonization boundaries into canonical SSOTs'
git push origin HEAD:agent/fix-commonization-boundaries
