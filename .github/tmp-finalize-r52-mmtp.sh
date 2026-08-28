#!/usr/bin/env bash
set -euo pipefail

git config user.name "github-actions[bot]"
git config user.email "41898282+github-actions[bot]@users.noreply.github.com"
git config core.quotePath false
git fetch origin design/r52-contracts tmp/r52-mmtp-old-base-20260829 tmp/r52-mmtp-old-head-20260829
test "$(git rev-parse origin/design/r52-contracts)" = "35e368573b46a09760194fdb1d3a3970839ebc0e"
git checkout -B rebased-r52-mmtp origin/tmp/r52-mmtp-old-head-20260829

python3 - <<'PY'
from pathlib import Path
p=Path('ARIB_SI_EPG_TvProvider投影方針.md')
s=p.read_text()
marker='### MMT/TLV transport identity の TvProvider 投影境界'
pos=s.find(marker)
if pos < 0:
    raise SystemExit('MMT/TLV projection section missing in original #66')
Path('/tmp/mmtp-tail.md').write_text(s[pos:].strip()+'\n')
PY

set +e
GIT_EDITOR=true git rebase --onto origin/design/r52-contracts origin/tmp/r52-mmtp-old-base-20260829 origin/tmp/r52-mmtp-old-head-20260829
rc=$?
set -e
test "$rc" -ne 0
mapfile -t conflicts < <(git diff --name-only --diff-filter=U)
test "${#conflicts[@]}" -eq 1
test "${conflicts[0]}" = "ARIB_SI_EPG_TvProvider投影方針.md"

git checkout --ours 'ARIB_SI_EPG_TvProvider投影方針.md'
printf '\n' >> 'ARIB_SI_EPG_TvProvider投影方針.md'
cat /tmp/mmtp-tail.md >> 'ARIB_SI_EPG_TvProvider投影方針.md'
git add 'ARIB_SI_EPG_TvProvider投影方針.md'
GIT_EDITOR=true git rebase --continue

test -z "$(git status --porcelain)"
git diff --check origin/design/r52-contracts..HEAD
test "$(git rev-list --count origin/design/r52-contracts..HEAD)" -eq 1
mapfile -t changed < <(git diff --name-only origin/design/r52-contracts..HEAD | sort)
expected=(
  'ARIB_SI_EPG_TvProvider投影方針.md'
  'arib_si_engine_rs/DESIGN_JA.md'
  'future_work/r52/japan_advanced_broadcast_mmtp_tlv_support.md'
  'tis/DESIGN_JA.md'
  'tuner_hal/DESIGN_JA.md'
  'tuner_hal2/DESIGN_JA.md'
  '開発規則.md'
)
mapfile -t expected_sorted < <(printf '%s\n' "${expected[@]}" | sort)
test "$(printf '%s\n' "${changed[@]}")" = "$(printf '%s\n' "${expected_sorted[@]}")"

git push --force origin HEAD:refs/heads/tmp/rebased-r52-mmtp-20260829
