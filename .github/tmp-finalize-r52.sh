#!/usr/bin/env bash
set -euo pipefail

git config user.name "github-actions[bot]"
git config user.email "41898282+github-actions[bot]@users.noreply.github.com"
git config core.quotePath false
git fetch origin docs/future-work-design-completion design/r52-contracts tmp/r52-old-base-20260829 tmp/r52-old-head-20260829
test "$(git rev-parse origin/docs/future-work-design-completion)" = "43b8ac5a7e431d2d4ef274ad2907d819364a9ccf"
test "$(git rev-parse origin/design/r52-contracts)" = "c67a4f2142bb04afd3a70ffaf5ef22909f069a3b"
git checkout -B rebased-r52 origin/tmp/r52-old-head-20260829

set +e
GIT_EDITOR=true git rebase --onto origin/docs/future-work-design-completion origin/tmp/r52-old-base-20260829 origin/tmp/r52-old-head-20260829
rc=$?
set -e
test "$rc" -ne 0
test "$(git rev-parse REBASE_HEAD)" = "e9a283a00f9e2682bb51215ae1097ef14cf97494"

python3 - <<'PY'
from pathlib import Path

def one(text):
    a=text.index('<<<<<<< HEAD\n'); b=text.index('=======\n',a); c=text.index('>>>>>>> ',b); d=text.index('\n',c)+1
    return a,d,text[a+len('<<<<<<< HEAD\n'):b],text[b+len('=======\n'):c]

p=Path('tis/CHANGELOG.md'); s=p.read_text(); a,d,ours,theirs=one(s)
p.write_text(s[:a]+theirs.rstrip()+'\n\n'+ours.lstrip()+s[d:])

p=Path('tis/src/com/maleicacid/tvinput/tis/TunerController.kt'); s=p.read_text(); a,d,ours,theirs=one(s)
p.write_text(s[:a]+ours+s[d:])

p=Path('tis/src/com/maleicacid/tvinput/tis/TunerSelectionPolicy.kt'); s=p.read_text()
old='    private val videoStreamTypes = setOf(0x02, 0x1b)\n'
new='    private val r51VideoStreamTypes = setOf(0x02, 0x1b)\n    private val videoStreamTypes = r51VideoStreamTypes + 0x24\n'
if old not in s: raise SystemExit('TunerSelectionPolicy video type anchor missing')
s=s.replace(old,new,1)
anchor='''    fun hasSupportedVideo(streams: List<AribElementaryStream>): Boolean =
        streams.any { isSupportedVideoStreamType(it.streamType) }
'''
extra=anchor+'''
    fun isR51SupportedVideoStreamTypeForTest(streamType: Int): Boolean = streamType in r51VideoStreamTypes

    fun selectR51VideoForTest(streams: List<AribElementaryStream>): AribElementaryStream? =
        streams.firstOrNull { it.streamType in r51VideoStreamTypes }

    fun hasR51SupportedVideoForTest(streams: List<AribElementaryStream>): Boolean =
        streams.any { it.streamType in r51VideoStreamTypes }
'''
if anchor not in s: raise SystemExit('TunerSelectionPolicy helper anchor missing')
p.write_text(s.replace(anchor,extra,1))

p=Path('tis/tests/src/com/maleicacid/tvinput/tis/TisR51FixedPlanAcceptanceTest.kt'); x=p.read_text()
replacements={
    'TunerSelectionPolicy.isSupportedVideoStreamType(0x24)':'TunerSelectionPolicy.isR51SupportedVideoStreamTypeForTest(0x24)',
    'TunerSelectionPolicy.isSupportedVideoStreamType(0x02)':'TunerSelectionPolicy.isR51SupportedVideoStreamTypeForTest(0x02)',
    'TunerSelectionPolicy.isSupportedVideoStreamType(0x1b)':'TunerSelectionPolicy.isR51SupportedVideoStreamTypeForTest(0x1b)',
    'TunerSelectionPolicy.selectVideo(listOf(es(TsPid(0x120), 0x24)))':'TunerSelectionPolicy.selectR51VideoForTest(listOf(es(TsPid(0x120), 0x24)))',
    'TunerSelectionPolicy.selectVideo(service.streams) == null':'TunerSelectionPolicy.selectR51VideoForTest(service.streams) == null',
    'TunerSelectionPolicy.selectVideo(listOf(es(TsPid(0x200), 0x24), es(TsPid(0x201), 0x1b)))':'TunerSelectionPolicy.selectR51VideoForTest(listOf(es(TsPid(0x200), 0x24), es(TsPid(0x201), 0x1b)))',
}
for old,new in replacements.items():
    if old not in x: raise SystemExit(f'R51 test anchor missing: {old}')
    x=x.replace(old,new,1)
marker='    @Test fun hevcOnlyServiceIsRejectedBeforePlaybackStart() {'
pos=x.find(marker)
if pos < 0: raise SystemExit('r51 HEVC rejection test missing')
end=x.find('\n    @Test ',pos+len(marker))
if end < 0: end=len(x)
block=x[pos:end]
if 'TunerSelectionPolicy.selectVideo(streams)' not in block or 'TunerSelectionPolicy.hasSupportedVideo(streams)' not in block:
    raise SystemExit('r51 HEVC rejection assertions changed unexpectedly')
block=block.replace('TunerSelectionPolicy.selectVideo(streams)','TunerSelectionPolicy.selectR51VideoForTest(streams)')
block=block.replace('TunerSelectionPolicy.hasSupportedVideo(streams)','TunerSelectionPolicy.hasR51SupportedVideoForTest(streams)')
p.write_text(x[:pos]+block+x[end:])
PY

# Strip workflow changes from the Actions-pushed commit series. They are restored as one API commit later.
git rm -f .github/workflows/cas-host-ci.yml
git checkout origin/docs/future-work-design-completion -- .github/workflows/tuner-hal2-host-rust-ci.yml

git add tis/CHANGELOG.md \
  tis/src/com/maleicacid/tvinput/tis/TunerController.kt \
  tis/src/com/maleicacid/tvinput/tis/TunerSelectionPolicy.kt \
  tis/tests/src/com/maleicacid/tvinput/tis/TisR51FixedPlanAcceptanceTest.kt \
  .github/workflows/tuner-hal2-host-rust-ci.yml

test -z "$(git diff --name-only --diff-filter=U)"
if git diff --cached --name-only | grep -q '^.github/workflows/'; then
  echo 'workflow change remains in first rebased commit' >&2
  git diff --cached --name-only >&2
  exit 1
fi

set +e
GIT_EDITOR=true git rebase --continue
rc2=$?
set -e
test "$rc2" -ne 0
test "$(git rev-parse REBASE_HEAD)" = "bb1f585a563757c6e020b7fbb9a696c4a94f3a7c"
test "$(git diff --name-only --diff-filter=U)" = "tis/src/com/maleicacid/tvinput/tis/MaleicacidLiveSession.kt"

python3 - <<'PY'
from pathlib import Path
p=Path('tis/src/com/maleicacid/tvinput/tis/MaleicacidLiveSession.kt'); s=p.read_text()
for choice in ['theirs','ours','ours','ours']:
    a=s.index('<<<<<<< HEAD\n'); b=s.index('=======\n',a); c=s.index('>>>>>>> ',b); d=s.index('\n',c)+1
    ours=s[a+len('<<<<<<< HEAD\n'):b]; theirs=s[b+len('=======\n'):c]
    s=s[:a]+({'ours':ours,'theirs':theirs}[choice])+s[d:]
if '<<<<<<<' in s or '>>>>>>>' in s: raise SystemExit('extra LiveSession conflict')
p.write_text(s)
PY

git add tis/src/com/maleicacid/tvinput/tis/MaleicacidLiveSession.kt
set +e
GIT_EDITOR=true git rebase --continue
rc3=$?
set -e
test "$rc3" -ne 0
test "$(git rev-parse REBASE_HEAD)" = "c49b4bee310907b4a04a7061008b8fcee7322648"
test "$(git diff --name-only --diff-filter=U)" = ".github/workflows/tis-host-ci.yml"
git checkout --ours .github/workflows/tis-host-ci.yml
git add .github/workflows/tis-host-ci.yml
GIT_EDITOR=true git rebase --skip

test -z "$(git status --porcelain)"
test -z "$(git diff --name-only origin/docs/future-work-design-completion..HEAD | grep '^.github/workflows/' || true)"
git diff --check origin/docs/future-work-design-completion..HEAD
grep -Fq 'private val videoStreamTypes = r51VideoStreamTypes + 0x24' tis/src/com/maleicacid/tvinput/tis/TunerSelectionPolicy.kt
grep -Fq 'isR51SupportedVideoStreamTypeForTest' tis/src/com/maleicacid/tvinput/tis/TunerSelectionPolicy.kt
grep -Fq 'CasController.Readiness.READY' tis/src/com/maleicacid/tvinput/tis/MaleicacidLiveSession.kt
git log --oneline origin/docs/future-work-design-completion..HEAD

git push --force origin HEAD:refs/heads/tmp/rebased-r52-code-20260829
