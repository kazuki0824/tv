from pathlib import Path

path = Path("tuner_hal/DESIGN_JA.md")
text = path.read_text(encoding="utf-8")


def replace_once(old: str, new: str, label: str) -> None:
    global text
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{label}: expected 1 match, got {count}")
    text = text.replace(old, new)


# PES capability and budget must cover every valid public streamId, including
# legal zero-length video PES. Remove all remaining 0xBD-only capability text.
text = text.replace("pesBoundedRuntimeBudgetBytes", "pesRuntimeBudgetBytes")

replace_once(
    "ARIB字幕用bounded PESは明示`streamId=0xBD`だけを公開し、`pesRuntimeBudgetBytes >= 65_541 * pesFilterCount`を満たす場合だけ非0にする。",
    "PES filterを非0で公開する場合は、`pesRuntimeBudgetBytes >= MAX_PES_BUFFER_BYTES * pesFilterCount`を満たし、有効な明示`streamId 0..255`とwildcard `0xFFFF`を同じ公開能力で受理する。ARIB字幕用`0xBD`はTISが選ぶ利用設定であり、HAL capabilityの部分集合ではない。",
    "runtime capability PES paragraph",
)

replace_once(
    "| `pesRuntimeBudgetBytes` | `65_541 * pesFilterCount`以上 |\n| PES製品契約 | `pesSupportedStreamIds={0xBD}`、`pesWildcardSupported=false`、`pesZeroLengthSupported=false` |",
    "| `pesRuntimeBudgetBytes` | `MAX_PES_BUFFER_BYTES * pesFilterCount`以上。実領域はPES単位の必要量だけをclaimする |\n| PES製品契約 | 明示`streamId 0..255`とwildcard `0xFFFF`を受理する。`PES_packet_length=0`は映像`0xE0..0xEF`だけをruntimeで許可する |",
    "PES product profile rows",
)

replace_once(
    "| T-PES-8 | bounded字幕PESが宣言長到達前に次PUSI | 未完PESを破棄し、次PESから再開 |\n| T-PES-9 | bounded字幕PESのflush/stop/close | 未完成を完成扱いせず、claimを返却 |\n| T-PES-10 | 同時PES filterが各65,541 byteをclaim | `pesRuntimeBudgetBytes`内で全filterを受理 |",
    "| T-PES-8 | bounded PESが宣言長到達前に次PUSI | 未完PESを破棄し、次PESから再開 |\n| T-PES-9 | bounded PESのflush/stop/close | 未完成を完成扱いせず、claimを返却 |\n| T-PES-10 | 同時PES filterが各`MAX_PES_BUFFER_BYTES`までclaim可能 | `pesRuntimeBudgetBytes`内で公開数全filterを受理 |",
    "PES tests bounded rows",
)

replace_once(
    "| T-PES-15 | 映像以外の`stream_id`で`PES_packet_length=0` | malformedとして破棄 |",
    "| T-PES-15 | 映像以外の`stream_id`で`PES_packet_length=0` | malformedとして破棄 |\n| T-PES-16 | `streamId=0xBD`以外の有効な明示stream ID | configure成功し、指定IDだけを照合・配送 |\n| T-PES-17 | wildcard `streamId=0xFFFF` | configure成功し、有効な全stream IDを配送対象にする |\n| T-PES-18 | 映像`stream_id 0xE0..0xEF`の長さ0 PES | 次PUSIまたはAU境界で完成し、`MAX_PES_BUFFER_BYTES`超過時だけoversize破棄 |",
    "PES tests generic rows",
)

replace_once(
    "現行PES filterは、外形と意味検証を分ける2段階契約に従う。完全なPES外形として明示`streamId=0xBD`かつ宣言長を持つ有効なPESを扱い、ヘッダーが複数TSパケットに分割される場合にも対応する。意味イベントの通知には、接頭辞、オプションヘッダー形式、フラグ、マーカービット、`header_data_length`、PTS/DTSの検証にも成功しなければならない。完全PES bytesを通常FMQへ書き込み、対応する`DemuxFilterPesEvent`で`dataLength`とPTS有無を通知する。長さ0 video PESは設定段階で`UNAVAILABLE`とし、raw PES payloadも通知しない。",
    "PES filterは、外形と意味検証を分ける2段階契約に従う。明示`streamId 0..255`またはwildcard `0xFFFF`の有効な設定を受理し、ヘッダーが複数TSパケットに分割される場合にも対応する。意味イベントの通知には、接頭辞、オプションヘッダー形式、フラグ、マーカービット、`header_data_length`、PTS/DTSの検証にも成功しなければならない。完全PES bytesを通常FMQへ書き込み、対応する`DemuxFilterPesEvent`で`dataLength`とPTS有無を通知する。宣言長ありPESは宣言長で完成し、映像`stream_id 0xE0..0xEF`の長さ0 PESは次PUSIまたはAU境界で完成する。その他のstream IDで長さ0を受信した場合はruntime malformedとして破棄する。",
    "PES test narrative",
)

replace_once(
    "- demux、型別filter、DVRの個数は、frontendと公開可能LNBの検出後、`ProductProfile`が列挙する完全な`RuntimeCapabilityVector`から選ぶ。各vectorは任意の非負整数を使用でき、2の冪へ丸めない。object数、worker、callback、reaper、cleanup、PES/AV/playback/FMQ byte予算をvector全体で一括予約し、候補間の列を混成しない。機能群ごとの縮退は他群の値を維持した完全vectorとして明示する。確定値は`CapabilitySnapshot`へ格納し、open/配送時の実領域はsnapshot残量から割り当てる。PES assemblerはARIB字幕用bounded PESだけを公開し、最大65,541 byte/active filterの共通実行時予算で保持する。unbounded video PESは公開しない。Tuner VTSは別途起動前環境へ結び付け、入力元、PID、経路、queue容量、memory予算が定義されるまで`DESIGN_HOLD_VTS_ENVIRONMENT_UNDECLARED`とする。",
    "- demux、型別filter、DVRの個数は、frontendと公開可能LNBの検出後、`ProductProfile`が列挙する完全な`RuntimeCapabilityVector`から選ぶ。各vectorは任意の非負整数を使用でき、2の冪へ丸めない。object数、worker、callback、reaper、cleanup、PES/AV/playback/FMQ byte予算をvector全体で一括予約し、候補間の列を混成しない。機能群ごとの縮退は他群の値を維持した完全vectorとして明示する。確定値は`CapabilitySnapshot`へ格納し、open/配送時の実領域はsnapshot残量から割り当てる。PES assemblerは全ての有効な明示stream IDとwildcardを同じ能力で扱い、宣言長ありPESと映像stream IDの長さ0 PESを`MAX_PES_BUFFER_BYTES`および`pesRuntimeBudgetBytes`内で保持する。Tuner VTSは別途起動前環境へ結び付け、入力元、PID、経路、queue容量、memory予算が定義されるまで`DESIGN_HOLD_VTS_ENVIRONMENT_UNDECLARED`とする。",
    "PES capability summary bullet",
)

replace_once(
    "公開object数と、その全数に必要なworker、callback、reaper、cleanup authority、PES、AV、playback処理中buffer、FMQ台帳使用権をvector全体で同時に仮予約し、最初に完全予約できた1個だけを確定する。別候補の列を混成しない。機能群を落とすfallbackが必要なら、他群を維持した別の完全vectorを`ProductProfile`へ明示する。確定済みsnapshotは能力広告と受付判定の唯一の入力であり、byte予算は後続割り当てが越えられない台帳上限である。AV payloadは配送時、bounded PESは開始時、FMQとplayback処理中bufferはconfigure時に実領域を確保する。PES filterを非0で公開する場合は、`pesRuntimeBudgetBytes >= MAX_PES_BUFFER_BYTES * pesFilterCount`を満たし、有効な明示`streamId 0..255`とwildcard `0xFFFF`を同じ公開能力で受理する。ARIB字幕用`0xBD`はTISが選ぶ利用設定であり、HAL capabilityの部分集合ではない。VTSは別の起動前環境bindingであり、未定義中は固定path XMLをinstallせず成功を表明しない。",
    "公開object数と、その全数に必要なworker、callback、reaper、cleanup authority、PES、AV、playback処理中buffer、FMQ台帳使用権をvector全体で同時に仮予約し、最初に完全予約できた1個だけを確定する。別候補の列を混成しない。機能群を落とすfallbackが必要なら、他群を維持した別の完全vectorを`ProductProfile`へ明示する。確定済みsnapshotは能力広告と受付判定の唯一の入力であり、byte予算は後続割り当てが越えられない台帳上限である。AV payloadは配送時、宣言長ありPESはヘッダー確定時、長さ0映像PESは受信量の増加時、FMQとplayback処理中bufferはconfigure時に実領域を確保する。PES filterを非0で公開する場合は、`pesRuntimeBudgetBytes >= MAX_PES_BUFFER_BYTES * pesFilterCount`を満たし、有効な明示`streamId 0..255`とwildcard `0xFFFF`を同じ公開能力で受理する。ARIB字幕用`0xBD`はTISが選ぶ利用設定であり、HAL capabilityの部分集合ではない。VTSは別の起動前環境bindingであり、未定義中は固定path XMLをinstallせず成功を表明しない。",
    "PES allocation timing",
)

# Resolve the remaining selector sentence against the px4 ABI collision rule.
replace_once(
    "`STREAM_ID`と`RELATIVE_STREAM_NUMBER`は別々に検証する。absolute selectorの`0..11`を数値だけを理由に特別拒否しない。",
    "`STREAM_ID`と`RELATIVE_STREAM_NUMBER`は別々に検証する。absolute `STREAM_ID 0..11`はLinux DVBでは通常のabsolute値として受理し、px4ではlegacy ABI上relative値と区別できないため`UNAVAILABLE`とする。数値だけでrelativeへ読み替えたり`INVALID_ARGUMENT`にしてはならない。",
    "selector collision summary",
)

for forbidden in (
    "pesSupportedStreamIds={0xBD}",
    "ARIB字幕用bounded PESだけを公開",
    "unbounded video PESは公開しない",
    "長さ0 video PESは設定段階で`UNAVAILABLE`",
    "absolute selectorの`0..11`を数値だけを理由に特別拒否しない",
    "pesBoundedRuntimeBudgetBytes",
):
    if forbidden in text:
        raise SystemExit(f"stale contract remains: {forbidden}")

for required in (
    "明示`streamId 0..255`またはwildcard `0xFFFF`の有効な設定を受理",
    "`pesRuntimeBudgetBytes >= MAX_PES_BUFFER_BYTES * pesFilterCount`",
    "px4ではlegacy ABI上relative値と区別できないため`UNAVAILABLE`",
):
    if required not in text:
        raise SystemExit(f"required contract missing: {required}")

path.write_text(text, encoding="utf-8")
