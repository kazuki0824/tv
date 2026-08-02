from pathlib import Path

path = Path("tuner_hal/DESIGN_JA.md")
text = path.read_text(encoding="utf-8")

replacements = [
    (
        "pesBoundedRuntimeBudgetBytes",
        "pesRuntimeBudgetBytes",
    ),
    (
        "ARIB字幕用bounded PESは明示`streamId=0xBD`だけを公開し、`pesRuntimeBudgetBytes >= 65_541 * pesFilterCount`を満たす場合だけ非0にする。",
        "PES filterを非0で公開する場合は、`pesRuntimeBudgetBytes >= MAX_PES_BUFFER_BYTES * pesFilterCount`を満たし、有効な明示`streamId 0..255`とwildcard `0xFFFF`を同じ公開能力で受理する。ARIB字幕用`0xBD`はTISが選ぶ利用設定であり、HAL capabilityの部分集合ではない。",
    ),
    (
        "| `pesRuntimeBudgetBytes` | `65_541 * pesFilterCount`以上 |\n| PES製品契約 | `pesSupportedStreamIds={0xBD}`、`pesWildcardSupported=false`、`pesZeroLengthSupported=false` |",
        "| `pesRuntimeBudgetBytes` | `MAX_PES_BUFFER_BYTES * pesFilterCount`以上。実領域はPES単位の必要量だけをclaimする |\n| PES製品契約 | 明示`streamId 0..255`とwildcard `0xFFFF`を受理する。`PES_packet_length=0`は映像`0xE0..0xEF`だけをruntimeで許可する |",
    ),
    (
        "| T-PES-8 | bounded字幕PESが宣言長到達前に次PUSI | 未完PESを破棄し、次PESから再開 |\n| T-PES-9 | bounded字幕PESのflush/stop/close | 未完成を完成扱いせず、claimを返却 |\n| T-PES-10 | 同時PES filterが各65,541 byteをclaim | `pesRuntimeBudgetBytes`内で全filterを受理 |",
        "| T-PES-8 | bounded PESが宣言長到達前に次PUSI | 未完PESを破棄し、次PESから再開 |\n| T-PES-9 | bounded PESのflush/stop/close | 未完成を完成扱いせず、claimを返却 |\n| T-PES-10 | 同時PES filterが各`MAX_PES_BUFFER_BYTES`までclaim可能 | `pesRuntimeBudgetBytes`内で公開数全filterを受理 |",
    ),
    (
        "| T-PES-15 | 映像以外の`stream_id`で`PES_packet_length=0` | malformedとして破棄 |",
        "| T-PES-15 | 映像以外の`stream_id`で`PES_packet_length=0` | malformedとして破棄 |\n| T-PES-16 | `streamId=0xBD`以外の有効な明示stream ID | configure成功し、指定IDだけを照合・配送 |\n| T-PES-17 | wildcard `streamId=0xFFFF` | configure成功し、有効な全stream IDを配送対象にする |\n| T-PES-18 | 映像`stream_id 0xE0..0xEF`の長さ0 PES | 次PUSIで完成し、`MAX_PES_BUFFER_BYTES`超過時だけoversize破棄 |",
    ),
    (
        "現行PES filterは、外形と意味検証を分ける2段階契約に従う。完全なPES外形として明示`streamId=0xBD`かつ宣言長を持つ有効なPESを扱い、ヘッダーが複数TSパケットに分割される場合にも対応する。意味イベントの通知には、接頭辞、オプションヘッダー形式、フラグ、マーカービット、`header_data_length`、PTS/DTSの検証にも成功しなければならない。完全PES bytesを通常FMQへ書き込み、対応する`DemuxFilterPesEvent`で`dataLength`とPTS有無を通知する。長さ0 video PESは設定段階で`UNAVAILABLE`とし、raw PES payloadも通知しない。",
        "PES filterは、外形と意味検証を分ける2段階契約に従う。明示`streamId 0..255`またはwildcard `0xFFFF`の有効な設定を受理し、ヘッダーが複数TSパケットに分割される場合にも対応する。意味イベントの通知には、接頭辞、オプションヘッダー形式、フラグ、マーカービット、`header_data_length`、PTS/DTSの検証にも成功しなければならない。完全PES bytesを通常FMQへ書き込み、対応する`DemuxFilterPesEvent`で`dataLength`とPTS有無を通知する。宣言長ありPESは宣言長で完成し、映像`stream_id 0xE0..0xEF`の長さ0 PESは同一PIDの次PUSIで完成する。その他のstream IDで長さ0を受信した場合はruntime malformedとして破棄する。",
    ),
    (
        "PES assemblerはARIB字幕用bounded PESだけを公開し、最大65,541 byte/active filterの共通実行時予算で保持する。unbounded video PESは公開しない。",
        "PES assemblerは全ての有効な明示stream IDとwildcardを同じ能力で扱い、宣言長ありPESと映像stream IDの長さ0 PESを`MAX_PES_BUFFER_BYTES`および`pesRuntimeBudgetBytes`内で保持する。",
    ),
    (
        "PES payloadフィルターはARIB字幕用の明示`streamId=0xBD`かつbounded PESに限定して公開し、通常FMQを使用する。長さ0のvideo PESは公開対象にしない。",
        "PES payloadフィルターは有効な明示`streamId 0..255`とwildcard `0xFFFF`を同じ能力で公開し、通常FMQを使用する。映像`0xE0..0xEF`の長さ0 PESもruntime組立て対象とする。",
    ),
    (
        "FMQの使用方法はフィルターのサブタイプごとに定める。SectionとTS生データのペイロードフィルターは通常のフィルターFMQを使用する。PESはARIB字幕用bounded PESに限定して通常FMQを使用する。PES subtypeの`openFilter()`は個数枠とFMQ容量を予約して成功させ、`configure()`は明示`streamId=0xBD`だけを成功させる。長さ0のvideo PES、wildcard、その他のstream IDは`configure()`で`UNAVAILABLE`とし、設定前状態を維持する。",
        "FMQの使用方法はフィルターのサブタイプごとに定める。SectionとTS生データのペイロードフィルターは通常のフィルターFMQを使用する。PESも通常FMQを使用し、PES subtypeの`openFilter()`は個数枠とFMQ容量を予約して成功させる。`configure()`は有効な明示`streamId 0..255`とwildcard `0xFFFF`を成功させる。映像`0xE0..0xEF`の長さ0 PESはruntime組立て対象とし、その他のstream IDで受信した長さ0 PESだけをmalformedとして破棄する。",
    ),
    (
        "| 2 | PES | ARIB字幕用bounded PESだけ受理 | `streamId=0xBD`のPES能力を宣言する | `openFilter()`は成功。`configure()`は明示`0xBD`だけ成功し、wildcard、他stream ID、長さ0 video PES用途は`UNAVAILABLE` | 表1のFMQ対象状態に従う | 通常FMQ + `DemuxFilterPesEvent` | TISのARIB字幕経路が使用する。video/audio本体はAV filter経路を使用 |",
        "| 2 | PES | 有効なPES設定を一般に受理 | 明示`streamId 0..255`とwildcard `0xFFFF`のPES能力を宣言する | `openFilter()`は成功。`configure()`は全有効stream IDとwildcardを成功させる。映像`0xE0..0xEF`の長さ0 PESもruntimeで扱う | 表1のFMQ対象状態に従う | 通常FMQ + `DemuxFilterPesEvent` | TISのARIB字幕経路は利用設定として`0xBD`を指定する。video/audio本体はAV filter経路を使用してよい |",
    ),
    (
        "### bounded字幕PESとunbounded video PESの境界",
        "### PES stream IDと宣言長の境界",
    ),
    (
        "現行製品profileのPES filterは、TISのARIB字幕経路が実際に要求する明示`streamId=0xBD`（`private_stream_1`）だけを成功対象とする。このstream IDで`PES_packet_length == 0`は伝送構文上の正常入力ではないため、対応対象は宣言長付きPESだけで閉じる。先頭6 byteを検証後、assemblerは`PES_packet_length + 6` byteだけをservice共通台帳からclaimする。16 bit宣言長から導かれる1 PESの最大保持量は65,541 byteであり、任意の8 MiB上限を設けない。起動前に`ProductProfile`を検証して`CapabilitySnapshot.pesRuntimeBudgetBytes`へ固定し、その値を`65_541 * CapabilitySnapshot.pesFilterCount`以上とする。各filterは同時に1 assemblerだけを所有するため、対応範囲内の同時入力を個数上限まで受理できる。",
        "PES filterは、有効な明示`streamId 0..255`とwildcard `0xFFFF`を同じ公開能力で受理する。先頭6 byteを検証後、宣言長ありPESではassemblerが`PES_packet_length + 6` byteだけをservice共通台帳からclaimする。映像`stream_id 0xE0..0xEF`の`PES_packet_length == 0`は、同一PIDの次PUSIまでを収集し、`MAX_PES_BUFFER_BYTES`を上限とする。起動前に`ProductProfile`を検証して`CapabilitySnapshot.pesRuntimeBudgetBytes`へ固定し、その値を`MAX_PES_BUFFER_BYTES * CapabilitySnapshot.pesFilterCount`以上とする。各filterは同時に1 assemblerだけを所有するため、公開個数上限まで最大保持量を同時に保証できる。",
    ),
    (
        "`DemuxCapabilities.numPesFilter`は個数だけを表し、対応stream ID集合または長さ制約を表現できない。このため現行製品profileは、`pesSupportedStreamIds={0xBD}`、`pesWildcardSupported=false`、`pesZeroLengthSupported=false`をTISとの統合契約として起動前に固定する。`numPesFilter > 0`を任意stream IDの一般PES対応表明として扱ってはならない。製品同梱TISはPMTで字幕ESを検出した場合に限り、字幕PIDと明示`streamId=0xBD`でPES filterを設定する。この制限を知らない一般クライアントからのopen自体は許容するが、`0xBD`以外またはwildcardのconfigureは状態を変えず`UNAVAILABLE`とし、任意PESへの対応を約束しない。一般クライアントへ追加stream IDを保証する製品profileを導入する場合は、対応集合、bounded/unbounded別assembler、全filter分の予算、VTS/product設定を先に追加し、そのprofileでだけ能力を有効にする。",
        "`DemuxCapabilities.numPesFilter`は個数だけを表し、対応stream ID集合または長さ制約を表現できない。そのため`numPesFilter > 0`を広告するdemuxは、有効な明示`streamId 0..255`とwildcard `0xFFFF`を一般PES設定として受理する。製品同梱TISはPMTで字幕ESを検出した場合に字幕PIDと明示`streamId=0xBD`を指定するが、これは利用側の選択でありHAL能力の非公開制限ではない。",
    ),
    (
        "`PES_packet_length == 0`を許す映像`stream_id 0xE0..0xEF`については、全量保持、chunk/streaming、共有予算、公平性、overflow時の再同期が未設計である。しかし本製品の映像本体はAV filterと`MediaEvent`の経路を使用し、PES filterへ映像を要求しない。したがって未設計範囲はPES能力全体を0にせず、PES `configure()`でwildcardと`0xE0..0xEF`を`UNAVAILABLE`として拒否する境界に閉じる。将来unbounded video PESを有効化する場合だけ、共有byte予算、chunk/streamingまたは有界保持、複数PID・複数demux間の公平性、overflow時の破棄・再同期・診断、close時の返却を先に固定する。",
        "`PES_packet_length == 0`を許す映像`stream_id 0xE0..0xEF`は、同一PIDの次PUSIを完成境界とし、`MAX_PES_BUFFER_BYTES`を超えた時点で当該PESをoversizeとして破棄して次PUSIから再同期する。全filterの最大保持量は`pesRuntimeBudgetBytes`で予約し、filter間は各1 assemblerの固定上限で公平性を確保する。`flush()`、`stop()`、`close()`では未完PESを完成扱いせずclaimを返す。",
    ),
    (
        "次表は現行のARIB字幕用bounded PES filterが満たす構文・再同期条件を表す。`streamId=0xBD`の宣言長付きPESだけを公開能力とし、長さ0 video PESに固有の行は将来拡張の禁止境界を表す。",
        "次表は一般PES filterが満たす構文・再同期条件を表す。設定は有効な明示stream IDまたはwildcardを受理し、受信したstream IDごとに宣言長ありPESと映像の長さ0 PESを区別する。",
    ),
    (
        "| `stream_id=0xBD`かつ`PES_packet_length > 0` | supported bounded PES | 宣言長+6 byteを共通台帳からclaimし、1 filter 1 assemblerで収集 | 完全長と意味検証成功時だけ配送 |\n| `stream_id=0xE0..0xEF`またはwildcard設定 | unsupported unbounded-video scope | `configure()`を`UNAVAILABLE`として設定前状態を維持 | 配送しない |",
        "| 任意の有効`stream_id`かつ`PES_packet_length > 0` | supported bounded PES | 宣言長+6 byteを共通台帳からclaimし、1 filter 1 assemblerで収集 | 完全長と意味検証成功時だけ配送 |\n| `stream_id=0xE0..0xEF`かつ`PES_packet_length == 0` | supported zero-length video PES | 次PUSIまで収集し、`MAX_PES_BUFFER_BYTES`超過時はoversize破棄 | 完成境界と意味検証成功時だけ配送 |",
    ),
    (
        "`STREAM_ID`と`RELATIVE_STREAM_NUMBER`は別々に検証する。absolute selectorの`0..11`を数値だけを理由に特別拒否しない。",
        "`STREAM_ID`と`RELATIVE_STREAM_NUMBER`は別々に検証する。absolute `STREAM_ID 0..11`はLinux DVBでは通常のabsolute値として受理し、px4ではlegacy ABI上relative値と区別できないため`UNAVAILABLE`とする。数値だけでrelativeへ読み替えたり`INVALID_ARGUMENT`にしてはならない。",
    ),
]

for old, new in replacements:
    if old in text:
        text = text.replace(old, new)

for forbidden in (
    "pesSupportedStreamIds={0xBD}",
    "ARIB字幕用bounded PESだけを公開",
    "unbounded video PESは公開しない",
    "長さ0 video PESは設定段階で`UNAVAILABLE`",
    "`configure()`は明示`streamId=0xBD`だけを成功",
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
