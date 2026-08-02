from __future__ import annotations

import sys
from pathlib import Path


root = Path(sys.argv[1])


def read(path: str) -> str:
    return (root / path).read_text(encoding="utf-8")


def write(path: str, text: str) -> None:
    (root / path).write_text(text, encoding="utf-8")


def replace_once(text: str, old: str, new: str, label: str) -> str:
    count = text.count(old)
    if count != 1:
        raise RuntimeError(f"{label}: expected exactly one match, got {count}")
    return text.replace(old, new, 1)


hal_path = "tuner_hal/DESIGN_JA.md"
hal = read(hal_path)

old_table = """- `TableInfo repeat=false`は、table idと確定versionに一致する全extensionを同一`start()`世代の収集対象とする。version wildcardは最初に受理した有効sectionのversionへ固定する。各extensionについて`last_section_number`と`section_number=0..last_section_number`を独立に管理し、`(table_id_extension, section_number)`ごとに最初の1件だけを配送する。最初に到着したextensionをcompletion targetとして固定せず、NIT other、BAT、SDT other、EITを含め、table種別またはextensionだけを理由に候補を除外しない。
- 停止判定はbroadcast-cycle closureで行う。観測済み全extensionの`0..last`が完成した後も受信を継続し、最後に新規`(table_id_extension, section_number)`を追加した時点より後に、観測済みの全section keyが少なくとも1回再出現し、その間に新しいextensionまたはsection keyが追加されなかった時点で停止する。closure前に新規keyを受信した場合は収集集合へ追加し、再出現確認を最初からやり直す。AOSPが公開していない総extension数を`ProductProfile`、PID、table種別その他の非公開情報から補わない。同一extension/versionで`last_section_number`が変化するsectionは当該収集集合へ混ぜず型付き診断を記録し、早期停止に使わない。`repeat=true`はtable id/versionに一致する全extensionのsectionを繰り返し配送する。この配送停止は公開`IFilter.stop()`と同じ状態遷移ではなく、filter objectの公開状態はStartedのまま維持し、利用側が明示的に`stop()` / `flush()` / `configure()` / `close()`を呼べる状態を保つ。
- `TableInfo.version` は `-1` または `0..31` だけを受け付ける。`-1` は wildcard、範囲外は `INVALID_ARGUMENT` とする。
"""
new_table = """- `TableInfo repeat=false`は、AOSP公開条件であるtable idとversionを変更せず、MPEG-TSの有限なtable instanceを1個だけsnapshotとして配送する。候補keyは`(table_id_extension, actual_version)`とし、各候補について同一の`last_section_number`に属する`section_number=0..last_section_number`を独立に収集する。明示versionではそのversionだけを候補にし、`version=-1`はwildcardのまま維持して、観測した各actual versionを別候補として扱う。最初の観測versionへ設定を書き換えたり、別versionを不一致として捨てたりしない。
- candidateは`0..last_section_number`が全て揃った時点で完成する。部分sectionは完成前に配送しない。最初に完成したcandidateをwinnerとし、同一ingress batchで複数が完成した場合は`actual_version`、`table_id_extension`の昇順で一意に選ぶ。winnerのsectionを`section_number`昇順で各1回配送して自動配送を停止し、他candidateのpartialを破棄する。AOSP公開面には総extension数・全version集合・終了時刻がなく、ARIB/MPEG-TSの`last_section_number`も1個のtable instanceだけを完結させるため、未観測instanceまで含む全体集合や再送周期を推測して停止条件にしない。この自動配送停止は公開`IFilter.stop()`ではなく、filter objectの公開状態はStartedのまま維持する。
- `TableInfo repeat=false`のcandidate bufferは`RuntimeCapabilityVector.tableInfoSnapshotBudgetBytes`から原子的にclaimする。winner完成前に予算を使い切った場合はpartialを全て破棄し、section filterのoverflow状態と型付き診断を通知して自動配送を停止する。表種別、PID、`ProductProfile`の非公開subtable一覧をcandidate選別または完了判定に使わない。`repeat=true`はtable idと、明示versionまたはwildcardに一致する全sectionを継続配送する。
- `TableInfo.version` は `-1` または `0..31` だけを受け付ける。`-1` はwildcardであり、runtimeの最初の観測値へ固定しない。範囲外は `INVALID_ARGUMENT` とする。
"""
hal = replace_once(hal, old_table, new_table, "TableInfo contract")

old_sec_tests = """| T-SEC-14 | NIT other / BAT / SDT other / EITを含むmulti-extensionの`TableInfo repeat=false` | table id/versionに一致して観測した全extensionを収集し、各subtableの`0..last`をkeyごとに1回配送する。最初のextension完成では停止しない |
| T-SEC-14a | 全観測subtable完成後、cycle closure前に新しいextensionまたはsection keyを受信 | 収集集合へ追加し、全keyの再出現確認をやり直して早期停止しない |
| T-SEC-14b | 全観測subtable完成後、新規keyなしで観測済み全section keyが再出現 | broadcast-cycle closureとして自動配送を停止する |
| T-SEC-14c | multi-subtable tableで`repeat=true` | table id/versionに一致する全extensionを配送し、繰り返しを継続する |
"""
new_sec_tests = """| T-SEC-14 | 複数extensionが並行する`TableInfo repeat=false` | candidateごとにpartialを隔離し、最初に完成した1 table instanceだけをsection番号順に1回配送する |
| T-SEC-14a | `version=-1`で複数actual versionが並行 | wildcardを最初のversionへ固定せず、versionごとに独立candidateを持つ |
| T-SEC-14b | 同一ingress batchで複数candidateが完成 | actual version、extensionの固定tie-breakでwinnerを一意にする |
| T-SEC-14c | winner完成前にsnapshot予算枯渇 | partialを配送せずoverflow診断後に自動配送を停止する |
| T-SEC-14d | multi-subtable tableで`repeat=true` | table idとversion条件に一致する全sectionを継続配送する |
"""
hal = replace_once(hal, old_sec_tests, new_sec_tests, "TableInfo tests")

old_pes_rows = """| PES start code 不正 | malformed | state 破棄 | 配送しない |
| optional header marker 不正 | malformed | state 破棄 | 配送しない |
| `PTS_DTS_flags == 0b01` | malformed | state 破棄 | 配送しない |
| PTS / DTS marker bit 不正 | malformed | state 破棄 | 配送しない |
| `PES_packet_length` と header 長が矛盾 | malformed | state 破棄 | 配送しない |
| 映像以外の`stream_id`で`PES_packet_length == 0` | malformed | state 破棄 | 配送しない |
| 任意の有効`stream_id`かつ`PES_packet_length > 0` | supported bounded PES | 宣言長+6 byteを共通台帳からclaimし、1 filter 1 assemblerで収集 | 完全長と意味検証成功時だけ配送 |
| `stream_id=0xE0..0xEF`かつ`PES_packet_length == 0` | supported zero-length video PES | 次PUSIまで収集し、`MAX_PES_BUFFER_BYTES`超過時はoversize破棄 | 完成境界と意味検証成功時だけ配送 |
"""
new_pes_rows = """| PES start code 不正 | malformed | state 破棄 | 配送しない |
| `stream_id`が`0xBC,0xBE,0xBF,0xF0,0xF1,0xF2,0xF8,0xFF` | ordinary optional headerを持たないspecial syntax | start code、stream id、宣言長と当該special payload境界だけを検証し、optional-header marker、`PTS_DTS_flags`、`header_data_length`、PTS/DTSを要求しない | 完全長とspecial syntax検証成功時だけ配送 |
| 上記以外の`stream_id`でoptional header marker不正 | malformed ordinary PES | state 破棄 | 配送しない |
| ordinary PESで`PTS_DTS_flags == 0b00` | timestampなしの有効PES | timestamp fieldを要求せず収集を継続 | 完全長で配送 |
| ordinary PESで`PTS_DTS_flags == 0b01` | malformed | state 破棄 | 配送しない |
| ordinary PESで`PTS_DTS_flags == 0b10`かつPTS marker正常 | PTSあり | PTSを内部検証して収集を継続 | 完全長で配送 |
| ordinary PESで`PTS_DTS_flags == 0b11`かつPTS/DTS marker正常 | PTS/DTSあり | PTS/DTSを内部検証して収集を継続 | 完全長で配送 |
| ordinary PESのPTS / DTS marker bit不正 | malformed | state 破棄 | 配送しない |
| ordinary PESで`PES_packet_length`とheader長が矛盾 | malformed | state 破棄 | 配送しない |
| 映像以外の`stream_id`で`PES_packet_length == 0` | malformed | state 破棄 | 配送しない |
| 有効`stream_id`かつ`PES_packet_length > 0` | supported bounded PES | stream id別の構文分岐後、宣言長+6 byteを共通台帳からclaimし、1 filter 1 assemblerで収集 | 対応する完全長・構文検証成功時だけ配送 |
| `stream_id=0xE0..0xEF`かつ`PES_packet_length == 0` | supported zero-length video PES | 次PUSIまで収集し、`MAX_PES_BUFFER_BYTES`超過時はoversize破棄 | 完成境界とordinary PES検証成功時だけ配送 |
"""
hal = replace_once(hal, old_pes_rows, new_pes_rows, "PES assembler rows")

old_pes_para = """PES filterは、外形と意味検証を分ける2段階契約に従う。明示`streamId 0..255`またはwildcard `0xFFFF`の有効な設定を受理し、ヘッダーが複数TSパケットに分割される場合にも対応する。意味イベントの通知には、接頭辞、オプションヘッダー形式、フラグ、マーカービット、`header_data_length`、PTS/DTSの検証にも成功しなければならない。完全PES bytesを通常FMQへ書き込み、対応する`DemuxFilterPesEvent`で`dataLength`とPTS有無を通知する。宣言長ありPESは宣言長で完成し、映像`stream_id 0xE0..0xEF`の長さ0 PESは同一PIDの次PUSIで完成する。その他のstream IDで長さ0を受信した場合はruntime malformedとして破棄する。
"""
new_pes_para = """PES filterは、外形検証の後に`stream_id`で通常optional-header構文とspecial syntaxを分岐する。明示`streamId 0..255`またはwildcard `0xFFFF`の有効な設定を受理し、ヘッダーが複数TSパケットに分割される場合にも対応する。通常構文では`PTS_DTS_flags=00`をtimestampなしの有効PESとして受理し、PTSまたはPTS/DTSが存在する場合だけflag、marker、`header_data_length`とtimestamp fieldを内部検証する。special syntaxへ通常optional-header検証を適用しない。完全PES bytesを通常FMQへ書き込み、`DemuxFilterPesEvent`ではAIDL公開フィールドの`streamId`、`dataLength`、`mpuSequenceNumber`だけを通知する。PES eventへPTS有無またはPTS値を追加しない。Media eventのPTS公開契約とは分離する。宣言長ありPESは宣言長で完成し、映像`stream_id 0xE0..0xEF`の長さ0 PESは同一PIDの次PUSIで完成する。その他のstream IDで長さ0を受信した場合はruntime malformedとして破棄する。
"""
hal = replace_once(hal, old_pes_para, new_pes_para, "PES event paragraph")

pes_test_anchor = "| T-PES-18 | 映像`stream_id 0xE0..0xEF`の長さ0 PES | 次PUSIで完成し、`MAX_PES_BUFFER_BYTES`超過時だけoversize破棄 |\n"
pes_test_new = pes_test_anchor + "| T-PES-19 | ordinary PESの`PTS_DTS_flags=00` | timestampなしの有効PESとして配送 |\n| T-PES-20 | ordinary optional headerを持たないspecial stream id | 通常header検証を適用せず、special syntaxの完全長を配送 |\n| T-PES-21 | PES event生成 | `streamId`、`dataLength`、`mpuSequenceNumber`だけを設定し、PTS有無を捏造しない |\n"
hal = replace_once(hal, pes_test_anchor, pes_test_new, "PES tests extension")

old_fr = "| FR-002 | Tuning / Locked | `tune(settings)` | 成功 | Tuning(generation+1) | 設定の同異にかかわらず旧tuneを停止・遮断して新tuneを開始する。backend固有の同一設定書込み省略は、公開transaction、generation fencing、demux stream boundary、callback契約を維持する内部最適化に限る |\n"
new_fr = "| FR-002a | Tuning | `tune(settings)` | 成功 | Tuning(stream_generation+1) | 未完了の旧tuneを停止・遮断して新tuneを開始する |\n| FR-002b | Locked、正規化settings・selector・LNB/power条件が同一、backend/stream boundaryがhealthy | `tune(settings)` | 成功 | Locked(stream_generation維持、tune_request_sequence+1) | backend再要求、worker交換、demux境界終端、AV中断を行わない。現lock snapshotに基づく`LOCKED`を新request sequenceへ1回配送する |\n| FR-002c | Lockedで条件が異なる、または同値性・健全性を証明できない | `tune(settings)` | 成功 | Tuning(stream_generation+1) | 旧tuneを停止・遮断して新tuneを開始する |\n"
hal = replace_once(hal, old_fr, new_fr, "frontend same-setting rows")

old_tx_intro = """`IFrontend.tune()` は、validate / prepare が完了するまで旧tune状態を破壊しない。受理した公開 `tune()` は、正規化設定が同一で前回tuneが完了済みかつ安定中であっても無処理成功にせず、旧要求を停止・遮断して新しいtransaction / generationへ進む。backend固有の同一設定書込み省略は、公開transaction、generation fencing、demux stream boundary、callback契約を全て維持できる場合の内部最適化に限る。
"""
new_tx_intro = """`IFrontend.tune()` は、validateとtransaction-lock下の同値性判定が完了するまで旧tune状態を破壊しない。前回tuneが未完了、設定が異なる、またはbackend・stream boundaryの健全性を証明できない場合だけ旧要求を停止・遮断して新しいstream generationへ進む。前回tuneが`Locked`で、正規化settings、typed selector、LNB/power条件が同一かつbackendと接続demux境界がhealthyである場合は、`stream_generation`を維持する非破壊re-entryとする。公開呼出しごとの`request_sequence`は更新し、現lock snapshotから`LOCKED`を当該sequenceへ1回配送するが、backend再要求、worker交換、境界reset、AV中断を行わない。
"""
hal = replace_once(hal, old_tx_intro, new_tx_intro, "tune transaction intro")

old_tn3 = "| TN-003 | same-setting re-entry | 正規化設定の同異にかかわらず、受理した公開呼出しを新transaction / generationへ進める。backend固有の同一設定書込み省略は公開transactionを短絡しない | 同一設定を理由に成功確定せず、TN-004へ進める | TN-004まで維持 |\n"
new_tn3 = "| TN-003a | stable same-setting re-entry | transaction lock下で`Locked`、正規化settings・typed selector・LNB/power条件の一致、backend/stream boundary healthyを同一snapshotから確認する。`request_sequence`だけを更新し、lock外で`LOCKED`を1回配送する | snapshotまたはcallback準備を確定できなければTN-003bへ進む | stream generation、worker、backend、demux境界、AVを維持 |\n| TN-003b | full retune selection | 旧tune未完了、条件不一致、または同値性・健全性を証明できない | TN-004へ進める | TN-004まで旧状態を維持 |\n"
hal = replace_once(hal, old_tn3, new_tn3, "TN-003")

old_deadline = """フロントエンドの存在と対応能力は、機器、versioned backend manifest、functional probe、有限の選局終端を実装できることから導出する。選局は非同期操作とし、バックエンドが選局要求を受理した後は、`LOCKED`、backendの明示失敗、明示的停止、再選局、閉鎖、または`ProductProfile.tuneTerminalDeadlineMs`到達時の`NO_SIGNAL`のいずれかで現generationを必ず終端する。期限到達はbinder呼び出しの成功を後から失敗へ反転させるものではなく、AIDLが要求する非同期終端eventである。正の有限期限と取消可能なbackend I/Oを実装できないfrontendは公開しない。停止した`ioctl`または読み取りから復帰する内部I/O期限は、選局終端期限とは別に`workerIoDeadlineMs`で管理する。
"""
new_deadline = """フロントエンドの存在と対応能力は、機器、versioned backend manifest、functional probe、有限の選局終端を実装できることから導出する。選局は非同期操作とし、バックエンドが選局要求を受理した後は、`LOCKED`、backendの明示失敗、明示的停止、再選局、閉鎖、またはbackend別`ProductProfile.tuneTerminalDeadlineMs`到達時の`NO_SIGNAL`のいずれかで現generationを必ず終端する。現行profileはearth_pt1を`4000 ms`、px4を`7000 ms`とする。px4値はRT710設定、PLL確認、demod lock、absolute TSID一致、およびrelative selectorのTMCC解決からなる正常な有限経路を期限前に打ち切らないための上限である。期限到達はbinder呼出しの成功を後から失敗へ反転させず、非同期終端eventとして扱う。VTS既知信号経路はVTS自身の待機内でLOCKEDへ到達できる入力を別途要求し、製品deadlineをVTS待機値へ短縮しない。正の有限期限と取消可能なbackend I/Oを実装できないfrontendは公開しない。停止した`ioctl`、read、USB control transferから復帰する内部期限は別の`workerIoDeadlineMs`で管理し、px4の`ctrl_timeout=0`を禁止する。個別I/O期限は検証済みcontrol transfer上限より短くせず、正常処理列の合計がbackendのterminal deadline内に収まるよう固定する。
"""
hal = replace_once(hal, old_deadline, new_deadline, "backend deadlines")

cap_anchor = "公開能力は、機器検出後に確定して以後変更しない1個の`CapabilitySnapshot`から導出する。"
if hal.count(cap_anchor) != 1:
    raise RuntimeError("capability anchor mismatch")
av_budget = """現行ProductProfileのAV未解放payload予算は、広告する各AV filterについて`avPerFilterLiveBytes=8 MiB`とし、`avRuntimeBudgetBytes=checked_mul(avPerFilterLiveBytes, advertisedAvFilterCount)`で導出する。この8 MiBはcodec規格上限ではなくHAL所有payloadの製品資源上限である。`RuntimeCapabilityVector`の仮予約時に全額を原子的に確保できないvectorは公開せず、filter開始後は各allocationをfilter別上限と全体上限の両方へclaimし、`releaseAvHandle()`または後片付け完了時に同じ台帳へ返却する。\n\n"""
hal = hal.replace(cap_anchor, av_budget + cap_anchor, 1)

# Remove stale summary wording if present.
hal = hal.replace("tune終端期限を4秒", "backend別tune終端期限")
hal = hal.replace("`tuneTerminalDeadlineMs=4000`", "backend別`ProductProfile.tuneTerminalDeadlineMs`")

write(hal_path, hal)

conv_path = "tuner_hal/CODE_CONVENTION.md"
conv = read(conv_path)
old_conv = """- 公開`IFrontend.tune()`は、同一normalized tune settingsであってもno-op guardの対象にしない。受理に成功した呼出しは既存のfrontend統合状態表とtune transactionへ入り、新しいtune generationを発行する。前回tuneが未完了なら旧tuneを停止・遮断してから新要求を開始し、scan中、Failed/cleanup中、callback終端未確定の状態もsettings一致だけで旧generationを継続しない。
- backend固有の同一設定書込みだけを省略できるのは、新generationの受付、旧generationのfencing、必要なstream boundary処理、callback契約をすべて維持し、backend状態の同一性を検証できる場合に限る。この最適化は公開呼出しのno-op化、旧workerの継続、または旧generationの再利用を意味しない。
"""
new_conv = """- 公開`IFrontend.tune()`は、前回tuneが未完了ならAOSP契約どおり旧tuneを停止・遮断して新要求を開始する。完了済み`Locked`で、normalized settings、typed selector、LNB/power条件、backend状態、stream boundaryの同値性と健全性をtransaction lock下の単一snapshotで証明できる場合は、request sequenceだけを更新し、stream generation、worker、backend要求、demux境界、AVを維持する非破壊re-entryを許可する。
- 非破壊re-entryでは現lock snapshotに対応する`LOCKED`を新request sequenceへlock外で1回配送する。条件不一致、旧tune未完了、scan中、Failed/cleanup中、callback終端未確定、同値性または健全性を証明できない場合はno-op guardへ入れず、`DESIGN_JA.md`のfull retune transactionへ進める。
"""
conv = replace_once(conv, old_conv, new_conv, "CODE_CONVENTION same tune")
write(conv_path, conv)

tis_path = "tis/DESIGN_JA.md"
tis = read(tis_path)
old_tis = """MediaEvent payloadは、`offset >= 0`、`dataLength > 0`、加算overflowなし、`offset + dataLength <= mapped buffer capacity`を満たす場合だけdecoder queueへ渡す。TISは共有領域方式とイベント固有fd方式の両方を受け付け、選択したMediaCodecの入力上限とTIS自身のpending queue byte予算だけを受付判定に使う。HALの`avPerFilterLiveBytes`、`avRuntimeBudgetBytes`その他の未解放payload集約台帳をTISへ公開・複製・1イベント上限化しない。codec入力またはTIS queue予算を超える場合は診断し、再生継続不能なら`notifyVideoUnavailable()`へ接続する。
"""
new_tis = """MediaEvent payloadは、`offset >= 0`、`dataLength > 0`、加算overflowなし、`offset + dataLength <= mapped buffer capacity`を満たす場合だけdecoder queueへ渡す。TISは共有領域方式とイベント固有fd方式の両方を受け付け、HALの`avPerFilterLiveBytes`、`avRuntimeBudgetBytes`その他の未解放payload集約台帳を公開・複製・1イベント上限化しない。

TISはdecoder構成完了後かつAV filter開始前に変更不能な`TisPlaybackBudgetSnapshot`を作る。現行productのrequested input上限は8 MiBとし、MediaFormatで指定可能なcodecでは`KEY_MAX_INPUT_SIZE`へ設定した上で、実際に取得したdecoder input bufferまたはblock capacityと照合し、`singleEventLimitBytes=min(8 MiB, verifiedDecoderInputCapacityBytes)`とする。capacityを正に確定できないdecoderは開始しない。pending queueは`pendingQueueMaxSamples=4`、`pendingQueueBudgetBytes=checked_mul(singleEventLimitBytes, 4)`とし、必要なqueue領域とclaim台帳をplayback generation開始時に原子的に予約する。予約不能ならfilterを開始せず、資源不足の診断を残して`notifyVideoUnavailable()`へ進む。

各eventはrange検証後、copy、map保持またはdecoder投入前に`dataLength`をsnapshot台帳へ原子的にclaimする。1event上限超過は`SAMPLE_TOO_LARGE`、queue sample数またはbyte予算超過は`PENDING_QUEUE_FULL`としてHAL handleを解放し、claim済みbyteはdequeue、generation変更、stop、releaseで正確に返す。first frame前の超過、またはqueueが満杯で`playbackBackpressureDeadlineMs=1000`の間dequeue進行がない場合はplaybackを停止して`notifyVideoUnavailable()`へ進む。first frame後の単発超過は当該sampleだけを破棄して再生を継続し、連続超過または進行不能時だけunavailableへ遷移する。audioだけのqueue超過はvideo-only継続可否を既存規則で判定し、無条件にvideo unavailableへ写像しない。
"""
tis = replace_once(tis, old_tis, new_tis, "TIS playback budget")
write(tis_path, tis)

# Static checks.
for path in (hal_path, conv_path, tis_path):
    text = read(path)
    if text.count("```") % 2:
        raise RuntimeError(f"unbalanced code fences: {path}")

for stale in (
    "broadcast-cycle closure",
    "version wildcardは最初に受理した",
    "`dataLength`とPTS有無を通知",
    "同一で前回tuneが完了済みかつ安定中であっても無処理成功にせず",
    "同一normalized tune settingsであってもno-op guardの対象にしない",
):
    for path in (hal_path, conv_path, tis_path):
        if stale in read(path):
            raise RuntimeError(f"stale text {stale!r} in {path}")

required = {
    hal_path: [
        "first" if False else "最初に完成したcandidate",
        "PTS_DTS_flags == 0b00",
        "DemuxFilterPesEvent`ではAIDL公開フィールド",
        "FR-002b",
        "px4を`7000 ms`",
        "tableInfoSnapshotBudgetBytes",
        "avPerFilterLiveBytes=8 MiB",
    ],
    conv_path: ["非破壊re-entry", "stream generation"],
    tis_path: ["TisPlaybackBudgetSnapshot", "pendingQueueMaxSamples=4", "playbackBackpressureDeadlineMs=1000"],
}
for path, terms in required.items():
    text = read(path)
    for term in terms:
        if term not in text:
            raise RuntimeError(f"missing {term!r} in {path}")

print("patched:")
print(hal_path)
print(conv_path)
print(tis_path)
