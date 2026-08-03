from __future__ import annotations

import re
from pathlib import Path

ROOT = Path.cwd()


def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def write(path: str, text: str) -> None:
    (ROOT / path).write_text(text, encoding="utf-8")


def replace_once(text: str, old: str, new: str, label: str) -> str:
    count = text.count(old)
    if count != 1:
        raise RuntimeError(f"{label}: expected exactly one literal match, got {count}")
    return text.replace(old, new, 1)


def regex_once(text: str, pattern: str, replacement: str, label: str) -> str:
    new_text, count = re.subn(pattern, replacement, text, count=1, flags=re.DOTALL)
    if count != 1:
        raise RuntimeError(f"{label}: expected exactly one regex match, got {count}")
    return new_text


# tuner_hal/DESIGN_JA.md
path = "tuner_hal/DESIGN_JA.md"
text = read(path)

linkcaps_row = "| `linkCaps` | main type 粒度 | 広告した main type pair は VTS が生成する subtype `UNDEFINED` 接続も成功対象に含める。成功させない pair は広告しない |"
text = replace_once(
    text,
    linkcaps_row,
    linkcaps_row
    + "\n| section filter | 現行ProductProfileでは非広告 | Android 14 AIDLのSECTION能力は`TableInfo`の`isRepeat=false`を含む通常設定全体を契約とし、`DemuxCapabilities`にはcondition／repeat別の部分対応を示す欄がない。`numSectionFilter=0`とし、VTS XMLにもSECTION filter scenarioを入れない |",
    "insert section capability profile row",
)

text = regex_once(
    text,
    r"- セクションフィルター は condition の必要 byte 幅.*?- `TableInfo\.version`は`-1`または`0\.\.31`だけを受け付ける。.*?範囲外は`INVALID_ARGUMENT`とする。\n",
    """- 現行`ProductProfile`はsection filter capabilityを公開しない。`DemuxCapabilities.numSectionFilter=0`とし、SECTION用object枠、FMQ、assembler、callback、workerを`CapabilitySnapshot`へ含めない。TS main typeとraw TS subtypeの能力は独立して維持する。\n- Android 14 AIDLの`DemuxFilterSectionSettings`は、`TableInfo`かつ`isRepeat=false`についてcallerのtable id／versionに基づくall sectionsを配送後に停止することをSECTION能力内の通常契約として定める。一方、`DemuxCapabilities`にはconditionまたは`isRepeat`別の部分対応を示す欄がない。このため、`TableInfo repeat=false`だけを`UNAVAILABLE`にする私的な部分対応は行わない。\n- `openFilter(TYPE_TS / SUBTYPE_SECTION, ...)`は、SECTION非広告と同じ確定済みsnapshotに基づき、object、ID、FMQ、callback artifact、ledger claimを生成せず`UNAVAILABLE`を返す。`configure()`まで不完全objectを公開して遅延拒否してはならない。\n- 現行TISのPSI/SI/CA取得は、PIDごとの`TYPE_TS / SUBTYPE_TS` raw TS filterを使用する。HALは指定PIDの完全な188-byte TS packetをFMQへ配送し、section境界、`pointer_field`、continuity、`section_length`、CRC、table versionの意味処理を行わない。section再組立とARIB表構文はTISから入力を受ける`arib_si_engine_rs`の責務とする。\n- 将来SECTION能力を公開する場合は、`SectionBits`と`TableInfo`、`isRepeat=false/true`、`version=-1`、複数extension/versionを含むAIDL有効設定全体を実装し、`numSectionFilter>0`、VTS profile、TIS利用経路、資源閉包、適合試験を同一変更で有効化する。\n""",
    "replace partial section support",
)

text = replace_once(
    text,
    "| T-TS-3 | TEI set packet | section/PES/AV assemblyへ入れない |",
    "| T-TS-3 | TEI set packet | raw TS出力には保持し、HAL内PES/AV意味処理へ入れない。TIS／arib_si_engine_rsもPSI/SI section組立てには使用しない |",
    "update TEI test",
)

text = regex_once(
    text,
    r"188バイトで構造上完全なTSパケットに `TEI=1` が設定されている場合、.*?エラーパケットを除いたTS生データまたは記録データを公開する場合は、バイト番号の契約を含む明示的な `ProductProfile` を別に定義する。",
    """188バイトで構造上完全なTSパケットに`TEI=1`が設定されている場合、TS生データ出力とTS記録出力には入力順のまま保持する。HALはTEIカウンターを飽和加算し、記録の`byteNumber`は実際に書き込んだバイト数を基準に進める。HAL内のPES／AV意味処理では当該packetを破棄または再同期し、解析済みeventを通知しない。現行SECTION能力は非広告であり、PSI/SI用raw TSを受けるTIS／`arib_si_engine_rs`は同じTEI packetをsection組立てへ使用しない。同期バイトまたは長さの不正、TEI、continuity不連続をそれぞれ型付き診断で分離し、放送packet上の異常だけを理由にFMQまたは経路を隔離してはならない。隔離は基盤破損の場合に限る。""",
    "update TEI ownership paragraph",
)

text = regex_once(
    text,
    r"### ARIB section 系\n.*?\n### PES / record index 系",
    """### Section filter capability 非広告\n\n| 番号 | 確認観点 | 目的 |\n|---:|---|---|\n| T-SEC-CAP-1 | `getDemuxCaps()` | `numSectionFilter=0` |\n| T-SEC-CAP-2 | `openFilter(TYPE_TS / SUBTYPE_SECTION)` | `UNAVAILABLE`、object／ID／FMQ／callback／ledger副作用なし |\n| T-SEC-CAP-3 | VTS profile | SECTION filter設定を含めず、raw TS filterだけを宣言 |\n| T-SEC-CAP-4 | TIS PSI/SI/CA取得 | PID別`TYPE_TS / SUBTYPE_TS`から完全188-byte packetを受信 |\n| T-SEC-CAP-5 | `arib_si_engine_rs` | continuity、PUSI、pointer、section再組立、長さ、CRC、version更新を試験 |\n| T-SEC-CAP-6 | 将来のSECTION有効化 | AIDL有効設定全体とVTSを同一変更で有効化し、部分対応を広告しない |\n\nARIB STD-B10に基づくtable ID、section長、CRC、表固有構文の検証は`arib_si_engine_rs/DESIGN_JA.md`を正本とする。HALはraw TS filterでPID照合と188-byte packet配送だけを行い、section payloadまたは`DemuxFilterSectionEvent`を生成しない。したがってSECTION能力を非広告にした状態で、HALの私的なsection設定部分集合をTISから利用してはならない。\n\n### PES / record index 系""",
    "replace ARIB section tests",
)

text = replace_once(
    text,
    "snapshot確定時に、ID重複、未定義bit、公開filter数と矛盾するmain type、`numDemux != size(publicDemuxes)`、または`filterCaps != OR(filterTypes)`を検出した候補はcommitせず、候補vector全体を戻す。確定後に`DemuxInfo`と`DemuxCapabilities`を別々に補正してはならない。これによりAndroid 14 VTSの全demux横断一致を構造的に保証する。",
    "snapshot確定時に、ID重複、未定義bit、公開filter数と矛盾するmain type、`numDemux != size(publicDemuxes)`、または`filterCaps != OR(filterTypes)`を検出した場合はsnapshotをcommitせず、選択済み閉包の仮予約を逆順に戻す。確定後に`DemuxInfo`と`DemuxCapabilities`を別々に補正してはならない。これによりAndroid 14 VTSの全demux横断一致を構造的に保証する。",
    "remove candidate vector wording",
)

text = replace_once(
    text,
    "- `numDemux`、main type別`filterCaps`、PES/AV/DVR個数が、依存先demuxと共有runtime claimを越えない。",
    "- `numDemux`、main type別`filterCaps`、PES/AV/DVR個数が、依存先demuxと共有runtime claimを越えない。\n- 現行ProductProfileでは`numSectionFilter=0`であり、SECTION subtype用object数、FMQ、assembler、worker、callback claimが0である。raw TS用`numTsFilter`は独立したTS filter閉包から導出する。",
    "add section closure invariant",
)

text = regex_once(
    text,
    r"(### サービスオブジェクトの上限\n\n) `ProductProfile`に宣言した完全vector.*?root queryと非対応APIの明示拒否に必要な最小状態も確保できない場合だけBinder serviceを登録しない。変更不能なsnapshotを個数、依存枠、byte予算、受付可否の正本とし、`CleanupPending`または`Quarantined`は解放完了まで使用中と数える。\n",
    r"\1選択済み`CapabilityClosure`のclaimから、demux、filter subtype、DVR用途別の公開個数と、FMQ／PES／AV／worker／callback／reaper／cleanup台帳上限を導出する。ある閉包の候補を確保できない場合は、その閉包と依存能力だけを非公開にし、依存しない閉包を0へ落とさない。全体を一個の完全vectorとして採否せず、AV不足を理由にraw TS、PES、record DVR等を一括0にするquery-only縮退を設けない。合成後の横断不変条件を満たすsnapshotを作れない場合は公開前に全仮予約を戻し、必須root queryと明示拒否に必要な最小状態も確保できない場合だけBinder serviceを登録しない。変更不能なsnapshotを個数、依存枠、byte予算、受付可否の正本とし、`CleanupPending`または`Quarantined`は解放完了まで使用中と数える。\n",
    "replace complete vector service limits",
)

text = regex_once(
    text,
    r"\| FILTER_SECTION \| サービス全体 \| 8 \| `CapabilitySnapshot`の値 \| 0 \| なし \| 呼び出し側指定のFMQ容量はsnapshotの`fmqRuntimeBudgetBytes`から別transactionで予約する。 \|",
    "| FILTER_SECTION | サービス全体 | 0 | 0 | 0 | なし | 現行ProductProfileはSECTION能力を非広告とし、`openFilter(TYPE_TS / SUBTYPE_SECTION)`を副作用なし`UNAVAILABLE`とする。 |",
    "set section filter resource to zero",
)

text = regex_once(
    text,
    r"- ARIB STD-B10 5\.13-E1 Part 1 5\.2\.4〜5\.2\.17・Part 3 5\.1\.1〜5\.1\.3を表ごとのsection上限1021/4093の根拠とし、STD-B32 3\.11-E1 Fascicle 3 Chapter 3 3\.1をPES構文の根拠とする。.*?AOSPに公開欄は追加せず、session間で共有する同じ内部台帳で受付と解放を強制する。",
    "- ARIB STD-B10 5.13-E1 Part 1 5.2.4〜5.2.17・Part 3 5.1.1〜5.1.3に基づくtable ID、section長1021/4093、CRC、表構文は`arib_si_engine_rs`のraw TS section組立・意味解析契約で強制する。HALはSECTION能力を非広告とし、これらをHALの公開section filter対応根拠として使用しない。STD-B32 3.11-E1 Fascicle 3 Chapter 3 3.1はHALのPES構文根拠として維持する。B25は公式英訳6.7-E1全文を精読基準とするが、Part 1 §4.9の受信機システム最小鍵組容量は本設計の適合対象外とする。STD-B25デコード能力は、対応するPart・方式・payload処理と、物理tuner/backend復号経路ごとの実鍵組数、実PID数、pool共有単位、枯渇時の`UNAVAILABLE`を製品profileの事実として定義する。AOSPに公開欄は追加せず、session間で共有する同じ内部台帳で受付と解放を強制する。",
    "move ARIB section ownership",
)

write(path, text)

# tis/DESIGN_JA.md
path = "tis/DESIGN_JA.md"
text = read(path)

marker = "## CAS / descrambler の現行境界\n"
raw_ts_section = """## PSI/SI raw TS取得境界\n\n現行Tuner HALの`CapabilitySnapshot`は`numSectionFilter=0`であり、TISはSECTION filterを開かない。PAT、PMT、CAT、SDT、NIT、BAT、EIT、ECM、EMM等を取得する場合は、対象PIDごとにTuner SDKの`TYPE_TS / SUBTYPE_TS` filterを開き、FMQから完全な188-byte TS packet列を受ける。filter instanceとPIDの対応はTISが保持し、driver名、HAL内部slot、section設定の私的部分集合へ依存しない。\n\nTISは受信したraw TS packetとPID、filter generation、入力generationをRust JNI境界へ渡す。TIS自身は`pointer_field`、section境界、continuity、CRC、table versionを重複実装せず、`arib_si_engine_rs`のraw TS section assemblerを唯一の意味解析入口とする。flush、stop、retune、入力元変更、filter closeでは旧generationの未完成sectionを破棄し、新generationのpacketと連結しない。TEI、continuity不連続、malformed packetは型付き診断へ残す。\n\n現行TISは`SectionBits`／`TableInfo` settings、`setCrcEnabled()`、SECTION eventを使用しない。VTS/product profileにもSECTION filter scenarioを含めず、PSI/SI/CA経路の試験はraw TS filter出力と`arib_si_engine_rs`のsection再組立・CRC・表構文試験へ分離する。\n\n"""
text = replace_once(text, marker, raw_ts_section + marker, "insert TIS raw TS section")

text = replace_once(
    text,
    "現行 product では CAS HAL 本体はプレースホルダーのままにする。TIS は Tuner SDK API の filter 経由で PMT/CAT/SDT/ECM/EMM section payload を取得し、PMT/CAT から得た CA_descriptor と SDT 等から得た free_CA_mode / サービス識別子 補助情報を arib_si_engine_rs / TIS 側で CA情報 / サービスメタデータ意味モデル に変換する。TIS はその CA情報 に基づいて ECM/EMM セクションフィルター と MediaCas/CAS bridge を型付き API で制御し、実 key トークン が得られた場合だけ Tuner descrambler へ不透明な参照値を渡す。仮実装 や診断専用結果は復号成功を意味しないため、`setKeyToken()` へ渡さない。Tuner HAL が未接続診断を返した場合も成功扱いにしない。",
    "現行 product では CAS HAL 本体はプレースホルダーのままにする。TISはPID別raw TS filterからPMT/CAT/SDT/ECM/EMM packetを取得し、`arib_si_engine_rs`が再組立したsectionから、PMT/CATのCA_descriptor、SDT等のfree_CA_mode、サービス識別子補助情報をCA情報／サービスメタデータ意味モデルへ変換する。TISはそのCA情報に基づいてECM/EMM用raw TS PID filterとMediaCas/CAS bridgeを型付きAPIで制御し、実key tokenが得られた場合だけTuner descramblerへ不透明な参照値を渡す。仮実装または診断専用結果は復号成功を意味しないため`setKeyToken()`へ渡さず、Tuner HALの未接続診断も成功扱いにしない。",
    "replace CAS section filter dependency",
)

text = replace_once(
    text,
    "- セクションフィルター は CRC protected section で `setCrcEnabled(true)` を使用し、Rust 側 CRC 検査を defense-in-depth として維持する。TIS 側には PID / table / 状態 別 counter を持つ。",
    "- PSI/SI/CA取得はPID別`TYPE_TS / SUBTYPE_TS` raw TS filterを使用する。CRC protected sectionのCRC検査は`arib_si_engine_rs`を正本とし、TISはPID／table／状態別counterとfilter generationだけを保持する。SECTION filterまたは`setCrcEnabled()`へ依存しない。",
    "replace TIS section filter bullet",
)

write(path, text)

# arib_si_engine_rs/DESIGN_JA.md
path = "arib_si_engine_rs/DESIGN_JA.md"
text = read(path)
text = regex_once(
    text,
    r"## 責務\n\n.*?\n\n## ARIB 文字列 decoder の適用範囲",
    """## 責務\n\n`arib_si_engine_rs`は、Tuner HALのPID別raw TS filter → framework/JNI/Tuner SDK API → TISという経路で渡された、PID、filter generation、入力generation付きの完全な188-byte TS packet列を入力とする。TISはfilter instanceとPIDの対応を所有し、本crateはTS header検証、continuity、PUSI／`pointer_field`、section再組立、section長、CRC、table version、PSI/SI/EIT descriptorの意味解析をRustで実装する。PMT/CATのCA_descriptorから得るCA_system_id、ECM PID、EMM PIDと、SDT等から得るfree_CA_mode／scrambling flag、サービス識別子補助情報を含むCA情報／サービスメタデータ意味モデルも本crate／TIS側の責務とする。Tuner HALはraw TS packetのPID照合とFMQ配送までを担当し、section payload、section event、CA意味モデルの生成者またはSSOTにならない。\n\n## raw TSからのsection組立契約\n\n- 入力は188 byteかつsync byte `0x47`を持つpacketだけを受理する。不完全packet、sync不正、予約済みadaptation制御、範囲外adaptation lengthはpacket単位で破棄し、PID、generation、理由を型付き診断へ記録する。\n- `TEI=1`のpacketはsection組立へ使用しない。continuity不連続、同一CCで異なるpayload、`discontinuity_indicator`では当該PID／generationの未完成sectionだけを破棄し、次の有効境界から再開する。raw TS保存・記録の方針を本crateで変更しない。\n- PUSIと`pointer_field`を解釈し、pointer以前のbytesで前sectionを完結できる場合だけ完結させ、残りから0個以上のsectionを順に組み立てる。1 packet内の複数section、headerまたはCRCのpacket境界分割、stuffing `0xFF`を扱う。\n- `section_length`を取得した時点で、ARIB STD-B10の表種別上限1021／4093と実行時総量台帳を検証する。上限超過、宣言長矛盾、reserved bit不正、CRC不一致は対象sectionだけを破棄し、次の正しい境界から再開する。\n- 組立状態は`(input_generation, filter_generation, PID)`ごとに分離し、packet保持byte、未完成section数、診断数に正の有限上限を持つ。flush、stop、retune、入力元変更、filter close、generation変更では旧状態を破棄し、新generationへ連結しない。\n- CRCを持つPSI/SI sectionは意味解析前にCRCを検証する。TISまたはHALのCRC結果を信頼して検査を省略しない。構文上有効なsectionだけをtable parserへ渡し、未対応table／descriptorはpanicせずraw bytesと診断を保持する。\n- `TableInfo`の有限停止を本crateで推測しない。本crateは受信した各sectionをtable instance／versionごとに差分更新し、TISが明示的にraw TS filterを停止するまで処理する。\n\n最低試験は、pointerで前section完結、1 packet複数section、header／CRC分割、TEI、continuity gap、duplicate、不一致duplicate、discontinuity、1021／4093境界、CRC不一致、複数PID／generation分離、flush／retune境界、version更新を含む。\n\n## ARIB 文字列 decoder の適用範囲""",
    "replace arib engine responsibility",
)
write(path, text)

# tuner_hal2/DESIGN_JA.md
path = "tuner_hal2/DESIGN_JA.md"
text = read(path)
text = replace_once(
    text,
    "公開AIDLの状態、戻り値、能力値、資源寿命、確定点、巻き戻し、後片付け、ワーカー、キュー、section/PES/TS処理は`../tuner_hal/DESIGN_JA.md`を正とする。PSI/SI表固有の意味解釈は`../arib_si_engine_rs/DESIGN_JA.md`を正とする。本書はこれらの契約を再定義せず、`tuner_hal2`の論理責務へ対応付ける。",
    "公開AIDLの状態、戻り値、能力値、資源寿命、確定点、巻き戻し、後片付け、ワーカー、キュー、raw TS／PES処理は`../tuner_hal/DESIGN_JA.md`を正とする。現行ProductProfileはSECTION filterを非広告とし、PSI/SI section再組立と表固有の意味解釈は`../arib_si_engine_rs/DESIGN_JA.md`を正とする。本書はこれらの契約を再定義せず、`tuner_hal2`の論理責務へ対応付ける。",
    "update tuner_hal2 responsibility",
)
text = replace_once(
    text,
    "| demux処理 | 入力元とTS packet | 入力元世代、continuity、section/PES assembler、配送候補 | PSI/SI意味解析、公開object寿命 |",
    "| demux処理 | 入力元とTS packet | 入力元世代、raw TS PID照合・FMQ配送、PES assembler、配送候補 | PSI/SI section再組立・意味解析、公開object寿命 |",
    "update demux component row",
)
text = replace_once(
    text,
    "TS入力は、frontend、playback DVR、許可されたsource filterの入力元を別の世代空間で保持する。packet validation、continuity、section/PES組み立て、filter照合までをdemux責務とし、PSI/SI意味解析を呼ばない。",
    "TS入力は、frontend、playback DVR、許可されたsource filterの入力元を別の世代空間で保持する。demuxは188-byte packetの外形検証、入力世代、raw TS PID照合・FMQ配送、HAL内PES／AV処理を所有する。現行ProductProfileではSECTION filter object／assemblerを生成せず、PSI/SI用raw TSのcontinuity、PUSI／pointer、section再組立、CRC、表構文はTIS経由の`arib_si_engine_rs`へ委ねる。",
    "update tuner_hal2 input processing",
)
write(path, text)

# tuner_hal/CODE_CONVENTION.md
path = "tuner_hal/CODE_CONVENTION.md"
text = read(path)
needle = "- capabilityは実際に同時予約が必要な依存閉包ごとに原子的に確定し、無関係な閉包の予約失敗を波及させない。最終snapshotの横断不変条件は合成後に一括検証する"
text = replace_once(
    text,
    needle,
    needle
    + "\n- SECTION filterを広告する場合は、`SectionBits`／`TableInfo`、`isRepeat=false/true`を含むAIDL有効設定全体を実処理できなければならない。conditionまたはrepeat別の私的部分対応を設けず、現行ProductProfileでは`numSectionFilter=0`とする",
    "add section advertise convention",
)
write(path, text)

print("patched review9 design documents")
