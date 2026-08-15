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


def paragraph_starting(text: str, prefix: str) -> str:
    i = text.index(prefix)
    # Require paragraph start or list-item start anchor supplied by caller.
    j = text.find("\n\n", i)
    if j < 0:
        j = len(text)
    return text[i:j]


def sentence_starting(text: str, prefix: str) -> str:
    i = text.index(prefix)
    j = text.index("。", i) + 1
    return text[i:j]


def replace_sentence(cur_text: str, old_text: str, old_prefix: str, current_prefixes: tuple[str, ...]) -> str:
    desired = sentence_starting(old_text, old_prefix)
    for cp in current_prefixes:
        if cp in cur_text:
            i = cur_text.index(cp)
            j = cur_text.index("。", i) + 1
            return cur_text[:i] + desired + cur_text[j:]
    raise RuntimeError(f"missing current sentence for {old_prefix}")

# 1. Restore the runtime PES sentinel contract. The TableInfo rollback restored the
# surrounding pre-review block but intentionally ended before this PES line, so add
# it back if absent.
old_runtime = line_with_prefix(old, "- PES `streamId`は")
if old_runtime not in cur:
    anchor = line_with_prefix(cur, "- `TableInfo.version`は")
    cur = cur.replace(anchor + "\n", anchor + "\n" + old_runtime + "\n", 1)

# 2. Restore every caller-visible/capability sentence changed from wildcard to
# mandatory rejection, without rolling back unrelated later wording in the same
# paragraphs.
cur = replace_sentence(
    cur, old,
    "PES filterを非0で公開する場合は",
    ("PES filterを非0で公開する場合は",),
)
cur = replace_sentence(
    cur, old,
    "PESは有効な明示`streamId 0..255`とwildcard `0xFFFF`を同じ能力で受理し",
    ("PESは有効な明示`streamId 0..255`", "PESは有効なPES `streamId` 0..255"),
)
cur = replace_sentence(
    cur, old,
    "`configure()`は有効な明示`streamId 0..255`とwildcard `0xFFFF`を成功させる",
    ("`configure()`は有効な明示`streamId 0..255`", "`configure()`は有効なPES `streamId` 0..255"),
)

# 3. Restore the PES capability row exactly.
old_row2 = line_with_prefix(old, "| 2 | PES |")
cur, n = re.subn(r"^\| 2 \| PES \|.*$", lambda _: old_row2, cur, count=1, flags=re.M)
if n != 1:
    raise RuntimeError(f"PES capability row replacement count={n}")

# 4. Restore only the first two paragraphs of the PES stream-ID section; retain
# later parser/generation ownership corrections made for unrelated reviews.
heading = "### PES stream IDと宣言長の境界\n"
if heading not in cur:
    raise RuntimeError("current PES stream-ID heading missing")
if heading not in old:
    raise RuntimeError("old PES stream-ID heading missing")
old_sec_start = old.index(heading) + len(heading)
old_sec_end = old.index("\n## 失敗時状態・境界処理の設計固定", old_sec_start)
old_paras = [x for x in old[old_sec_start:old_sec_end].strip().split("\n\n") if x.strip()]
if len(old_paras) < 2:
    raise RuntimeError("old PES section has fewer than two paragraphs")
cur_sec_start = cur.index(heading) + len(heading)
cur_sec_end = cur.index("\n## 失敗時状態・境界処理の設計固定", cur_sec_start)
cur_paras = [x for x in cur[cur_sec_start:cur_sec_end].strip().split("\n\n") if x.strip()]
if len(cur_paras) < 2:
    raise RuntimeError("current PES section has fewer than two paragraphs")
cur_paras[0] = old_paras[0]
cur_paras[1] = old_paras[1]
cur = cur[:cur_sec_start] + "\n" + "\n\n".join(cur_paras) + "\n" + cur[cur_sec_end:]

# 5. Restore the assembler-input statement that wildcard is a valid configuration.
old_assembler_sentence = sentence_starting(
    old,
    "次表は一般PES filterが満たす構文・再同期条件を表す。設定は",
)
# The first sentence contains both clauses and ends at the second Japanese period;
# recover the exact old full line instead because it is one Markdown line.
old_assembler_line = line_with_prefix(old, "次表は一般PES filterが満たす構文・再同期条件を表す。設定は")
cur, n = re.subn(
    r"^次表は一般PES filterが満たす構文・再同期条件を表す。設定は.*$",
    lambda _: old_assembler_line,
    cur,
    count=1,
    flags=re.M,
)
if n != 1:
    raise RuntimeError(f"PES assembler intro replacement count={n}")

# 6. Restore T-PES-17 exactly.
old_tpes17 = line_with_prefix(old, "| T-PES-17 |")
cur, n = re.subn(r"^\| T-PES-17 \|.*$", lambda _: old_tpes17, cur, count=1, flags=re.M)
if n != 1:
    raise RuntimeError(f"T-PES-17 replacement count={n}")

# 7. Restore only the streamId-validation sentence in the later PES parser summary;
# preserve later syntax/detail improvements in the rest of that paragraph.
reject_re = re.compile(
    r"設定で受理する明示`streamId`は0\.\.255だけとし、`0xFFFF` \(`INVALID_STREAM_ID`\) と256\.\.65535は`INVALID_ARGUMENT`で拒否する。"
)
replacement = "明示`streamId 0..255`またはwildcard `0xFFFF`の有効な設定を受理する。"
cur, n = reject_re.subn(replacement, cur, count=1)
if n != 1:
    raise RuntimeError(f"later PES parser streamId sentence replacement count={n}")

# 8. Restore FILTER_PES resource contract exactly.
old_filter_pes = line_with_prefix(old, "| FILTER_PES |")
cur, n = re.subn(r"^\| FILTER_PES \|.*$", lambda _: old_filter_pes, cur, count=1, flags=re.M)
if n != 1:
    raise RuntimeError(f"FILTER_PES row replacement count={n}")

# 9. Restore the PES-closure sentence inside the later capability-summary bullet,
# while retaining unrelated VTS/resource wording added later.
old_cap_sentence = sentence_starting(old, "PES assemblerは全ての有効なPES `streamId` 0..255")
# Current text can have the same prefix but reject 0xFFFF later or explicit-only.
if "PES assemblerは全ての有効なPES `streamId` 0..255" not in cur:
    raise RuntimeError("current PES capability-summary sentence missing")
i = cur.index("PES assemblerは全ての有効なPES `streamId` 0..255")
j = cur.index("。", i) + 1
cur = cur[:i] + old_cap_sentence + cur[j:]

# PES-specific rejection residues from review 4944521799 must be gone. Do not
# globally reject INVALID_STREAM_ID because frontend ISDB-S legitimately uses it.
for bad in (
    "| T-PES-17 | `streamId=0xFFFF`または256..65535 |",
    "| FILTER_PES | サービス全体 | 4 | `CapabilitySnapshot`の値 | 0 | demux当たり1 | 有効な明示`streamId 0..255`を同じPES capabilityで扱う。`0xFFFF`",
    "設定で受理する明示`streamId`は0..255だけとし、`0xFFFF`",
    "PES filterの`streamId`として受理する値は0..255だけである。",
):
    if bad in cur:
        raise RuntimeError(f"PES rejection residue remains: {bad}")

# Positive assertions covering all directly related design surfaces.
for required in (
    old_runtime,
    old_tpes17,
    old_filter_pes,
    old_row2,
    "PES filterは、有効な明示`streamId 0..255`とwildcard `0xFFFF`を同じ公開能力で受理する。",
    "`DemuxCapabilities.numPesFilter`は個数だけを表し、対応stream ID集合または長さ制約を表現できない。",
    "`configure()`は有効な明示`streamId 0..255`とwildcard `0xFFFF`を成功させる。",
):
    if required not in cur:
        raise RuntimeError(f"restored PES contract missing: {required}")

p.write_text(cur)
print("restored PES 0xFFFF wildcard/sentinel contract on all review-affected design surfaces")
