from pathlib import Path
import re
import subprocess

BASE = "630c21d9b595a35fbf2d1e2e7c8f5d9f557a3a12"
PATH = "tuner_hal/DESIGN_JA.md"

p = Path(PATH)
cur = p.read_text()
old = subprocess.check_output(["git", "show", f"{BASE}:{PATH}"], text=True)


def extract_between(text: str, start: str, end: str) -> str:
    a = text.index(start)
    b = text.index(end, a)
    return text[a:b]


def line_with_prefix(text: str, prefix: str) -> str:
    for line in text.splitlines():
        if line.startswith(prefix):
            return line
    raise RuntimeError(f"missing line prefix: {prefix}")

# Restore the exact pre-review first-instance one-shot contract, while keeping
# unrelated later raw Section/PES callback changes that follow this block.
old_tableinfo = extract_between(
    old,
    "- セクションフィルターの`repeat=false`は重複抑止ではなく",
    "- PES `streamId`は",
)
cur, n = re.subn(
    r"#### TableInfo / SectionBits repeat=false one-shot契約\n.*?(?=#### raw section / raw PES event 生成契約)",
    old_tableinfo + "\n",
    cur,
    count=1,
    flags=re.S,
)
if n != 1:
    raise RuntimeError(f"TableInfo contract replacement count={n}")

# Restore the complete pre-review T-SEC-14..14i test contract.
old_tests = re.search(
    r"^\| T-SEC-14 \|.*?(?=^\| T-SEC-15 \|)",
    old,
    flags=re.M | re.S,
).group(0)
cur, n = re.subn(
    r"^\| T-SEC-14 \|.*?(?=^\| T-SEC-15 \|)",
    old_tests,
    cur,
    count=1,
    flags=re.M | re.S,
)
if n != 1:
    raise RuntimeError(f"T-SEC-14 replacement count={n}")

# Restore the resource model that follows from one finite target per Filter.
for prefix in ("| filter main type / FMQ |", "| FILTER_SECTION |"):
    desired = line_with_prefix(old, prefix)
    cur, n = re.subn(
        rf"^{re.escape(prefix)}.*$",
        lambda _m, d=desired: d,
        cur,
        count=1,
        flags=re.M,
    )
    if n != 1:
        raise RuntimeError(f"resource row replacement failed: {prefix}, count={n}")

# The section-level tracking summary was not changed by the bad review and must
# remain identical to the pre-review wording.
summary_line = line_with_prefix(old, "サービスオブジェクトの公開個数、FMQ・PES・AVの各byte上限とSECTION one-shot追跡上限")
if summary_line not in cur:
    raise RuntimeError("SECTION one-shot summary drifted unexpectedly")

# Reject every residue introduced by the active-set interpretation.
for bad in (
    "active set",
    "active instanceごとのmetadata",
    "内部extension/current_nextでmatching instanceを除外しない",
    "matching instanceを別trackerとして追跡",
    "全active instance完了後だけ停止",
    "hidden eligibility filter",
):
    if bad in cur:
        raise RuntimeError(f"active-set residue remains: {bad}")

# Positive assertions for the restored contract.
for required in (
    "first-instanceはAOSPの明文要求ではなく、有限なsnapshotを決定的に選択する製品内規則である",
    "1 filter当たり1個の`TableInstanceKey`",
    "| T-SEC-14i | 複数extension/versionが並行する`TableInfo repeat=true` |",
    "SECTIONでは公開数分の`TableInfoOneShotTracker`（target metadataと256-bit bitmap）を含む",
    "各公開filterについて1個のtarget metadataと256-bit（32 byte）の配送済みbitmap",
):
    if required not in cur:
        raise RuntimeError(f"restored contract missing: {required}")

p.write_text(cur)
print("restored TableInfo first-instance contract and all directly related resource/test changes")
