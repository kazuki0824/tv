#!/usr/bin/env bash
set -euo pipefail

git config user.name "github-actions[bot]"
git config user.email "41898282+github-actions[bot]@users.noreply.github.com"
git fetch origin design/r52-contracts
test "$(git rev-parse origin/design/r52-contracts)" = "35e368573b46a09760194fdb1d3a3970839ebc0e"
git checkout -B r52-source-fix origin/design/r52-contracts

python3 - <<'PY'
from pathlib import Path
p = Path('tis/src/com/maleicacid/tvinput/tis/MaleicacidLiveSession.kt')
s = p.read_text()
old = '''    private fun stopPlaybackForCasWait() {
        currentPlaybackSignature = null
        pendingPlaybackSignature = null
        playbackStartGate.reset()
        tunerController.stopPlayback()
        currentPlaybackPipelineGeneration = -1L
        captionController.beginPlaybackGeneration(-1L, false)
    }
'''
new = '''    private fun stopPlaybackForCasWait() {
        playbackState = PlaybackStartState.Stopped
        tunerController.stopPlayback()
        captionController.beginPlaybackGeneration(-1L, false)
    }
'''
if old not in s:
    raise SystemExit('stale CAS-wait playback block not found')
p.write_text(s.replace(old, new, 1))
PY

git diff --check
grep -Fq 'playbackState = PlaybackStartState.Stopped' tis/src/com/maleicacid/tvinput/tis/MaleicacidLiveSession.kt
! grep -Fq 'currentPlaybackSignature = null' tis/src/com/maleicacid/tvinput/tis/MaleicacidLiveSession.kt
git add tis/src/com/maleicacid/tvinput/tis/MaleicacidLiveSession.kt
git commit -m 'fix(tis): align CAS wait with playback state owner'
git push --force origin HEAD:refs/heads/tmp/r52-source-fix-20260829
