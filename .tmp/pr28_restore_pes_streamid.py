from pathlib import Path
import re
import subprocess

BASE = "630c21d9b595a35fbf2d1e2e7c8f5d9f557a3a12"
PATH = "tuner_hal/DESIGN_JA.md"

p = Path(PATH)
cur = p.read_text()
old = subprocess.check_output(["git", "show", f"{BASE}:{PATH}"], text=True)


def line_with_prefix(text: str, prefix: str) -> str:
    for line in text.splitlines():
        if line.startswith(prefix):
            return line
    raise RuntimeError(f"missing line prefix: {prefix}")


def sentence_starting(text: str, prefix: str) -> str:
    i = text.index(prefix)
    j = text.index("。", i) + 1
    return text[i:j]

# The bad TableInfo-review rollback ended immediately before this pre-existing PES
# line, so restore it explicitly from the pre-review design.
old_runtime = line_with_prefix(old, "- PES `streamId`は")
if old_runtime not in cur:
    anchor = line_with_prefix(cur, "- `TableInfo.version`は")
    cur = cur.replace(anchor + "\n", anchor + "\n" + old_runtime + "\n", 1)

# Restore the currently surviving review-4944521799 surfaces. Older capability
# paragraphs/rows that were independently deleted by later cleanup are not
# resurrected.
old_assembler = line_with_prefix(old, "次表は一般PES filterが満たす構文・再同期条件を表す。設定は")
cur, n = re.subn(
    r"^次表は一般PES filterが満たす構文・再同期条件を表す。設定は.*$",
    lambda _: old_assembler,
    cur,
    count=1,
    flags=re.M,
)
if n != 1:
    raise RuntimeError(f"assembler intro replacement count={n}")

old_tpes17 = line_with_prefix(old, "| T-PES-17 |")
cur, n = re.subn(r"^\| T-PES-17 \|.*$", lambda _: old_tpes17, cur, count=1, flags=re.M)
if n != 1:
    raise RuntimeError(f"T-PES-17 replacement count={n}")

reject_sentence = (
    "設定で受理する明示`streamId`は0..255だけとし、`0xFFFF` (`INVALID_STREAM_ID`) "
    "と256..65535は`INVALID_ARGUMENT`で拒否する。"
)
if reject_sentence in cur:
    cur = cur.replace(
        reject_sentence,
        "明示`streamId 0..255`またはwildcard `0xFFFF`の有効な設定を受理する。",
        1,
    )

old_filter_pes = line_with_prefix(old, "| FILTER_PES |")
cur, n = re.subn(r"^\| FILTER_PES \|.*$", lambda _: old_filter_pes, cur, count=1, flags=re.M)
if n != 1:
    raise RuntimeError(f"FILTER_PES row replacement count={n}")

# Restore the review-changed PES-closure sentence while preserving all later
# unrelated text in the same bullet.
cap_prefix = "PES assemblerは全ての有効なPES `streamId` 0..255"
if cap_prefix in cur:
    desired = sentence_starting(old, cap_prefix)
    i = cur.index(cap_prefix)
    j = cur.index("。", i) + 1
    cur = cur[:i] + desired + cur[j:]
else:
    raise RuntimeError("current PES capability-summary sentence missing")

# No surviving PES-specific mandatory-rejection residue may remain.
for bad in (
    "| T-PES-17 | `streamId=0xFFFF`または256..65535 |",
    "設定で受理する明示`streamId`は0..255だけとし、`0xFFFF`",
    "| FILTER_PES | サービス全体 | 4 | `CapabilitySnapshot`の値 | 0 | demux当たり1 | 有効な明示`streamId 0..255`を同じPES capabilityで扱う。`0xFFFF`",
    "次表は一般PES filterが満たす構文・再同期条件を表す。設定は有効なPES `streamId` 0..255を受理し",
):
    if bad in cur:
        raise RuntimeError(f"PES rejection residue remains: {bad}")

for required in (
    old_runtime,
    old_assembler,
    old_tpes17,
    old_filter_pes,
):
    if required not in cur:
        raise RuntimeError(f"restored PES contract missing: {required}")

if "wildcard `0xFFFF`" not in cur:
    raise RuntimeError("no PES wildcard contract remains")

p.write_text(cur)
print("restored all surviving PES streamId changes caused by review 4944521799")
