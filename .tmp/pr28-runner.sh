#!/bin/bash
set -euo pipefail
base64 -d .tmp/pr28-final.patch.gz.b64 | gzip -d > /tmp/pr28.patch
patch --batch --forward --fuzz=3 -p0 < /tmp/pr28.patch
python3 - <<'PY'
from pathlib import Path
text=Path('tuner_hal/DESIGN_JA.md').read_text(encoding='utf-8')
req=['#### 0-S-3B. 共通部品の規範定義','`DemuxFrontendSourceTxn`','`CallbackRegistrationUseCase`','`LnbControlTxn`','`DescramblerPidTxn`','`RecordDvrFilterRelationTxn`','`FilterFlushTxn`','`DvrFlushTxn`','`AvSyncRegistry`','`PcrClockAnchorStore`','`WorkerRuntime` / `WorkerHandle`']
for x in req:
    assert x in text, x
bad=['未 close または cleanup 未完了を DropLeakTxn に記録','| Filter / DVR `flush()` の queue cleanup | `QueueCleanupTxn` |','ワーカー制御失敗、コールバック失敗、backend failure を enum / domain error で分類','Dropでは通常後片付けを再試行しない。DropLeakTxnへ未完診断を記録']
for x in bad:
    assert x not in text, x
rows=[]; on=False
hdr='| 論理契約名 | 実装正本 | 公開入口 | 所有する状態 | 所有しない状態 | phase order | 失敗時処理 | 呼び出し許可層 | 呼び出し禁止層 | 最低テスト |'
for line in text.splitlines():
    if line==hdr: on=True; continue
    if on:
        if line.startswith('|---'): continue
        if not line.startswith('|'): break
        cells=[c.strip() for c in line.strip().strip('|').split('|')]
        assert len(cells)==10 and all(cells), line
        rows.append(cells)
assert len(rows)==20, len(rows)
PY
git diff --check
rm .tmp/pr28-final.patch.gz.b64 .tmp/pr28-runner.sh .github/workflows/pr28-canonical-commonization-v3.yml
git config user.name github-actions[bot]
git config user.email 41898282+github-actions[bot]@users.noreply.github.com
git add tuner_hal/DESIGN_JA.md .tmp/pr28-final.patch.gz.b64 .tmp/pr28-runner.sh .github/workflows/pr28-canonical-commonization-v3.yml
git commit -m 'design: integrate commonization boundaries into canonical public SSOT'
git push origin HEAD:agent/fix-commonization-boundaries
