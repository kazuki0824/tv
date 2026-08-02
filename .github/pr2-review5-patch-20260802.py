from pathlib import Path
import re


def replace_once(text: str, old: str, new: str, label: str) -> str:
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{label}: expected 1 exact match, got {count}")
    return text.replace(old, new)


def sub_once(text: str, pattern: str, new: str, label: str, flags: int = re.S) -> str:
    updated, count = re.subn(pattern, new, text, count=1, flags=flags)
    if count != 1:
        raise SystemExit(f"{label}: expected 1 regex match, got {count}")
    return updated


# ---------------------------------------------------------------------------
# TIS: keep the public selector independent of backend details, but do not
# claim a TMCC-based conversion that is not the selected px4 legacy ABI path.
# ---------------------------------------------------------------------------
path = Path("tis/DESIGN_JA.md")
text = path.read_text(encoding="utf-8")
text = replace_once(
    text,
    "CS110 tune request 生成時、TIS は Android Tuner API builder の default `streamId` / `streamIdType` に依存しない。CS110 では frontend stream selector を明示的に none / `UNDEFINED` 相当に設定する。CS110 の ONID / TSID / service_id は channel identity / サービス識別子 として保持してよいが、HAL frontend selector へ転用してはならない。BSはIF周波数と`STREAM_ID`のTSIDを使い、driver固有の相対番号への変換はTuner HAL内部へ閉じる。",
    "CS110 tune request 生成時、TIS は Android Tuner API builder の default `streamId` / `streamIdType` に依存しない。CS110 では frontend stream selector を明示的に none / `UNDEFINED` 相当に設定する。CS110 の ONID / TSID / service_id は channel identity / サービス識別子 として保持してよいが、HAL frontend selector へ転用してはならない。BSの通常製品経路はIF周波数と`STREAM_ID`のTSIDを使う。TISはdriver固有slotへ変換せず、typed selectorの検証とbackend ABIへの写像はTuner HALへ委ねる。",
    "TIS tune selector boundary",
)
text = replace_once(
    text,
    "BSの通常実行時候補は、TISが保持するBS TSID表からIF周波数と`STREAM_ID 0..65534`として生成する。TISはHALのeffective capabilityやdriver名で候補を分岐しない。Tuner HALは同じ公開requestを各backendへ変換し、px4では受信TMCCから要求TSIDに対応する相対slotを実行時に解決する。固定のTSID→slot候補表をHALへ複製してはならない。",
    "BSの通常実行時候補は、TISが保持するBS TSID表からIF周波数と`STREAM_ID 0..65534`として生成する。TISはHALのeffective capabilityやdriver名で候補を分岐しない。Tuner HALはselector kindを保持して各backend ABIへ写像する。px4の相対slot表またはlegacy数値域をTISへ複製してはならない。",
    "TIS normal BS candidates",
)
path.write_text(text, encoding="utf-8")


# ---------------------------------------------------------------------------
# Cross-module rules: TIS uses canonical public inputs; HAL accepts both AOSP
# selector kinds and maps them to the px4 legacy slot ABI without guessing.
# ---------------------------------------------------------------------------
path = Path("開発規則.md")
text = path.read_text(encoding="utf-8")
text = replace_once(
    text,
    "- px4 legacy driverはBS選択を相対slotで実行するが、Tuner公開面ではTSIDを受け付ける。HALは周波数選局後に受信TMCCから要求TSIDの相対slotを実行時に解決し、driver固有値をTISへ公開しない",
    "- px4 legacy driverの`slot` ABIは、`slot < 12`を相対番号、`slot >= 12`をabsolute TSIDとして解釈する。HALはAOSPのtyped `RELATIVE_STREAM_NUMBER`と`STREAM_ID`を区別し、表現可能な値をlegacy `slot`へ直接渡す。数値域からselector kindを推測せず、driver固有値をTISへ公開しない",
    "root px4 selector fact",
)
text = sub_once(
    text,
    r"## BS/CS選局方法\n.*?\n\n## 日本向け scan / 選局契約",
    """## BS/CS選局方法
日本のISDB-Sでは、BSは同一周波数帯で複数TSを扱い、CS110は本製品の選局契約上、周波数だけで選局する。

TISが生成するBSの通常製品requestは、backendにかかわらずIF周波数とAOSP `STREAM_ID`のTSID `0..65534`とする。earth_pt1 / DVB backendはTSIDを`DTV_STREAM_ID`へ渡す。px4 backendはtyped selectorを保持し、`RELATIVE_STREAM_NUMBER 0..7`はlegacy `slot`へそのまま渡し、`STREAM_ID 12..65534`もlegacy `slot`へそのまま渡す。absolute `STREAM_ID 0..11`はAOSP上有効だがpx4 ABIでは相対番号と区別できないため、px4では副作用なしの`UNAVAILABLE`とする。`65535`は明示TSIDとして`INVALID_ARGUMENT`とする。

HALはselector kindを数値域から推測せず、TISへdriver種別、相対slot、HAL内部capabilityを公開しない。AOSP公開`RELATIVE_STREAM_NUMBER`はHALの正式入力として保持するが、TISの通常scan・channel保存・再選局では使用しない。

## 日本向け scan / 選局契約""",
    "root BS selection section",
)
text = sub_once(
    text,
    r"px4 backendは、TISから渡されたIF周波数とabsolute TSIDをpx4 legacy APIへ変換するadapterに限定する。.*?表19のbackend失敗分類に従う。",
    """px4 backendはscan policyを持たず、AOSPのtyped selectorをpx4 legacy APIの`slot`へ写像するadapterに限定する。`RELATIVE_STREAM_NUMBER 0..7`と`STREAM_ID 12..65534`はkindを保持した検証後に値をそのまま渡す。`STREAM_ID 0..11`はlegacy ABI上の相対番号と衝突してabsolute値として表現できないため`UNAVAILABLE`とし、別TSへfallbackせずbackendを変更しない。固定TSID→slot表、TMCC由来の暗黙変換、数値域によるselector kind推測を導入しない。""",
    "root px4 adapter",
)
text = sub_once(
    text,
    r"frontend_px4はscan policyを持たず、AOSPのIF周波数\+TSID requestをpx4_drvの`freq_no / slot / addfreq`へ変換するだけにする。.*?固定表を持ってはならない。",
    """frontend_px4はscan policyを持たず、AOSPのIF周波数とtyped selectorをpx4_drvの`freq_no / slot / addfreq`へ変換するだけにする。相対番号とabsolute TSIDはselector kindで検証し、legacy ABIで表現可能な値だけを`slot`へ直接渡す。本書の選局契約とTISの製品用候補表を置き換える変換表を持ってはならない。""",
    "root frontend px4",
)
text = sub_once(
    text,
    r"px4のISDB-S frontendを製品へ公開する条件は、対象driverでTMCC取得とTSID→相対slotの動的解決が検証済みであることとする。.*?静的TSID→relative slot表を導入してはならない。",
    """px4のISDB-S frontendを製品へ公開する条件は、対象driverのlegacy `slot` ABIと、`RELATIVE_STREAM_NUMBER 0..7`および`STREAM_ID 12..65534`の直接写像が検証済みであることとする。absolute `STREAM_ID 0..11`を成功広告せず、指定時は`UNAVAILABLE`として副作用を生じさせない。TIS側を相対番号へ分岐させて補償せず、frontend_px4へTSID→relative slot変換表を導入してはならない。""",
    "root px4 export condition",
)
text = sub_once(
    text,
    r"## BS/CS110 stream selector境界\n.*?\n\n## BS/CS110 selector固定テスト",
    """## BS/CS110 stream selector境界
BSの通常製品経路では、TISはtyped `STREAM_ID`のTSID `0..65534`を生成する。HALはselector kindを正として値域から種類を推測しない。earth_pt1 / DVBでは`STREAM_ID 0..65534`をabsolute値として処理する。px4では`RELATIVE_STREAM_NUMBER 0..7`と`STREAM_ID 12..65534`をlegacy `slot`へ直接渡し、absolute `STREAM_ID 0..11`は相対値域と衝突して表現不能なため`UNAVAILABLE`とする。衝突値を`INVALID_ARGUMENT`またはrelative selectorとして解釈してはならない。

TISは`RELATIVE_STREAM_NUMBER`を通常候補またはchannelデータへ使用しない。CS110では、channel key/サービス識別子にONID/TSID/service_idを保持してよいが、HAL frontend selectorへ転用しない。stream selectorはNONEとし、HALは周波数だけで選局する。AOSP SDK defaultの`streamIdType=STREAM_ID`かつ`streamId=INVALID_STREAM_ID(0xFFFF)`はselectorなしとして吸収する。CS110に実selectorが指定された場合は`INVALID_ARGUMENT`とする。

## BS/CS110 selector固定テスト""",
    "root selector boundary",
)
text = sub_once(
    text,
    r"## BS/CS110 selector固定テスト\n.*?\n\n## px4_drv系のロック方法",
    """## BS/CS110 selector固定テスト
次の契約を単体テストで固定し、必要に応じて静的確認で補強すること。

- TISがBSの通常候補をbackendにかかわらず`STREAM_ID`のTSIDで生成し、HAL内部capabilityまたは相対slotを参照しないこと。
- earth_pt1 / DVB backendがBS `STREAM_ID 0..65534`をabsolute値として処理すること。
- px4 backendが`RELATIVE_STREAM_NUMBER 0..7`をlegacy `slot`へ直接渡すこと。
- px4 backendが`STREAM_ID 12..65534`をlegacy `slot`へ直接渡すこと。
- px4 backendがabsolute `STREAM_ID 0..11`を相対番号へ誤解せず、副作用なしの`UNAVAILABLE`とすること。
- frontend_px4が固定TSID→slot表またはTMCC由来の暗黙変換を持たず、TISがdriver名またはHAL内部capabilityで候補を分岐しないこと。
- TISがCS110 tune requestにstream selectorを付けないこと。
- HALがCS110の実selectorを`INVALID_ARGUMENT`とし、AOSP SDK defaultの`STREAM_ID / INVALID_STREAM_ID(0xFFFF)`だけをselectorなしとして吸収すること。

## px4_drv系のロック方法""",
    "root selector tests",
)
path.write_text(text, encoding="utf-8")


# ---------------------------------------------------------------------------
# Tuner HAL: remove hidden table/PES subsets, restore both typed selectors for
# px4, and separate public ILnb from verified satellite power topology.
# ---------------------------------------------------------------------------
path = Path("tuner_hal/DESIGN_JA.md")
text = path.read_text(encoding="utf-8")

text = sub_once(
    text,
    r"- セクションフィルターの`repeat=false`は重複抑止ではなく、同一`start\(\)`世代内の配送停止条件である。.*?(?=\n- `TableInfo.version`)",
    """- セクションフィルターの`repeat=false`は重複抑止ではなく、同一`start()`世代内の配送停止条件である。`SectionBits`は最初に一致したsectionを1件配送した後に停止する。`TableInfo`のconfigure可否と候補照合条件はAOSP公開settingsのtable idとversionだけから決め、PID、table種別、runtime `ProductProfile`のsubtable一覧、事前`table_id_extension`、事前`last_section_number`によって有効な設定を`UNAVAILABLE`にしない。
- `TableInfo repeat=false`は、最初に受理した有効sectionの`table_id_extension`を当該start世代のcompletion targetとして固定する。version wildcardは同sectionのversionへ固定し、同じextension/versionの`last_section_number`と`section_number=0..last_section_number`をsubtable単位で管理する。対象sectionを各1回配送し、targetの`0..last`が完成した時点で停止する。NIT other、BAT、SDT other、EITを含め、table種別だけを理由にconfigure拒否しない。
- completion target以外のextension、またはtargetと異なるversion/`last_section_number`を持つsectionは、targetの完了集合へ混ぜず配送しない。型付き診断を記録し、target完了前の早期停止に使わない。`repeat=true`はtable id/versionに一致する全extensionのsectionを繰り返し配送する。この配送停止は公開`IFilter.stop()`と同じ状態遷移ではなく、filter objectの公開状態はStartedのまま維持し、利用側が明示的に`stop()` / `flush()` / `configure()` / `close()`を呼べる状態を保つ。""",
    "HAL TableInfo generic repeat contract",
)
text = sub_once(
    text,
    r"^- PES `streamId` は .*?$",
    "- PES `streamId`は`0..=255`を明示`stream_id`として照合し、AOSP `Constant.INVALID_STREAM_ID`の`0xFFFF`をwildcardとして扱う。負値、`256..=65534`、`65536`以上は`INVALID_ARGUMENT`とする。PES能力を広告するdemuxは、全ての有効な明示stream IDとwildcardを通常のPES filter設定として受理し、`0xBD`その他の私的部分集合へ制限しない。ARIB字幕を利用するTIS profileは`0xBD`を指定してよいが、それは利用側の選択でありHAL capabilityの制限ではない。`PES_packet_length=0`はH.222.0で許可される映像stream ID `0xE0..0xEF`のruntime組立てとして扱い、その他のstream IDで受信した長さ0 PESはmalformedとして当該意味単位を破棄する。",
    "HAL generic PES streamId",
    flags=re.M,
)
text = sub_once(
    text,
    r"^\| FILTER_PES \|.*$",
    "| FILTER_PES | サービス全体 | 4 | `CapabilitySnapshot`の値 | 0 | demux当たり1 | 有効な明示`streamId 0..255`とwildcard `0xFFFF`を同じPES capabilityで扱う。宣言長ありPESは宣言長+6 byteをPES実行時台帳からclaimし、映像`0xE0..0xEF`の長さ0 PESは`MAX_PES_BUFFER_BYTES`と同台帳の上限内で組み立てる。stream ID別の非公開capabilityを設けない。 |",
    "HAL FILTER_PES resource row",
    flags=re.M,
)
text = sub_once(
    text,
    r"\| T-SEC-14 \|.*?\n\| T-SEC-14a \|.*?\n\| T-SEC-14b \|.*?(?=\n)",
    "| T-SEC-14 | NIT other / BAT / SDT other / EITを含む有効な`TableInfo repeat=false` | table種別に依存せずconfigure成功。最初の有効sectionのextension/version/lastをtargetに固定し、同じsubtableの`0..last`を各1回配送して停止 |\n| T-SEC-14a | target完了前に同じtable id/versionの別extensionを受信 | target集合へ混ぜず配送せず、診断を記録し、targetの`0..last`完了前に停止しない |\n| T-SEC-14b | multi-subtable tableで`repeat=true` | table id/versionに一致する全extensionを配送し、繰り返しを継続する |",
    "HAL TableInfo tests",
)

text = sub_once(
    text,
    r"ISDB-S frontendを公開する場合、通常製品経路のAOSP公開selector契約はbackendにかかわらず`STREAM_ID`のTSID `0\.\.65534`へ統一する。.*?`ProductProfile`は検証済み能力を抑止できるが、新設または拡張してはならない。",
    """ISDB-S selectorはAOSPの`FrontendIsdbsStreamIdType`を正とし、`STREAM_ID`と`RELATIVE_STREAM_NUMBER`を別domainとして受理・検証する。Linux DVB / earth_pt1は`STREAM_ID 0..65534`を`DTV_STREAM_ID`へ渡す。px4 legacy ABIは`slot < 12`を相対番号、`slot >= 12`をabsolute TSIDとして解釈するため、px4では`RELATIVE_STREAM_NUMBER 0..7`と`STREAM_ID 12..65534`をlegacy `slot`へ直接渡す。absolute `STREAM_ID 0..11`はAOSP上有効だが同ABIで相対値と区別できないため、副作用なしの`UNAVAILABLE`とする。`65535`は明示TSIDとして`INVALID_ARGUMENT`とする。selector kindを数値域から推測せず、TISへ`EffectiveCapabilities`、driver名、relative slotを公開しない。`ProductProfile`は検証済み能力を抑止できるが、新設または拡張してはならない。""",
    "HAL selector capability paragraph",
)
text = sub_once(
    text,
    r"セレクターの基本対応は次のとおりとする。BSの通常公開入力は`STREAM_ID 0\.\.65534`に統一する。.*?ISDB-T、CATV、CS110ではISDB-S用selectorを使用しない。\n\n\nselectorの種類を正として判定し、数値域から種類を推測しない。\n\n\npx4 BSで絶対値の`STREAM_ID`をslotへ直接渡す経路は、.*?HALはTSIDから相対slotへの変換表を互換処理として復活させない。",
    """セレクターの基本対応は次のとおりとする。Linux DVB / earth_pt1は`STREAM_ID 0..65534`を値を変更せず`DTV_STREAM_ID`へ渡す。px4は`RELATIVE_STREAM_NUMBER 0..7`と`STREAM_ID 12..65534`をkind別に検証してlegacy `slot`へ直接渡す。px4のabsolute `STREAM_ID 0..11`は相対番号とのABI衝突により表現不能なので`UNAVAILABLE`とし、relativeとして解釈せずbackendを変更しない。固定TSID→slot表、TMCC由来の暗黙変換、TISからのbackend hintを使わない。ISDB-T、CATV、CS110ではISDB-S用selectorを使用しない。

selectorの種類を正として判定し、数値域から種類を推測しない。`RELATIVE_STREAM_NUMBER`はHALの正式入力だが、TISの通常product/VTS候補が使用する必要はない。""",
    "HAL basic selector section",
)
text = replace_once(
    text,
    "| 相対selectorに対応するpx4の完全一致項目 | ISDB-S | `RELATIVE_STREAM_NUMBER` | `0..7` | 検証済みの相対枠設定経路へ反映 | 独立したabsolute selectorの完全一致項目がない場合、`STREAM_ID 0..65534`は`UNAVAILABLE` | `0..7`以外の相対値または`STREAM_ID=65535`：`INVALID_ARGUMENT` |\n| absolute selectorに対応するLinux DVBの完全一致項目 | ISDB-S | `STREAM_ID` | `0..65534` | 値を変更せず`DTV_STREAM_ID`へ渡す | 独立したrelative selectorの完全一致項目がない場合、`RELATIVE_STREAM_NUMBER 0..7`は`UNAVAILABLE` | `STREAM_ID=65535`または`0..7`以外の相対値：`INVALID_ARGUMENT` |",
    "| px4 legacy selector ABIの完全一致項目 | ISDB-S | `RELATIVE_STREAM_NUMBER` | `0..7` | 値を変更せずlegacy `slot`へ渡す | なし | `0..7`以外：`INVALID_ARGUMENT` |\n| px4 legacy selector ABIの完全一致項目 | ISDB-S | `STREAM_ID` | `12..65534` | 値を変更せずlegacy `slot`へ渡す | `0..11`はAOSP上有効だがABI衝突で表現不能：`UNAVAILABLE` | `65535`または値域外：`INVALID_ARGUMENT` |\n| absolute selectorに対応するLinux DVBの完全一致項目 | ISDB-S | `STREAM_ID` | `0..65534` | 値を変更せず`DTV_STREAM_ID`へ渡す | relative selectorに対応しない場合、`RELATIVE_STREAM_NUMBER 0..7`は`UNAVAILABLE` | `STREAM_ID=65535`または`0..7`以外の相対値：`INVALID_ARGUMENT` |",
    "HAL selector capability rows",
)
text = sub_once(
    text,
    r"選択子の対応能力は、機器識別情報と改訂適用範囲、versioned backend manifestのABI/API契約版、要求を実際に設定して結果を読み戻すfunctional probeが一致し、かつ`selector_capability_release_eligible=true`である台帳項目だけから作る。.*?本表で選択子値`65535`を拒否する規則と混同しない。",
    """選択子の対応能力は、機器識別情報と改訂適用範囲、versioned backend manifestのABI/API契約版、要求を実際に設定して結果を読み戻すfunctional probeが一致し、かつ`selector_capability_release_eligible=true`である台帳項目だけから作る。repository、commit SHA、build IDは台帳項目の作成証跡として保存してよいが、実行時の一致条件にしない。現在のpx4台帳はlegacy ABIに従い、相対`0..7`とabsolute `12..65534`を別typed selectorとして有効にする。absolute `0..11`は有効なAOSP値だがABIで表現不能なので`UNAVAILABLE`とし、相対値へ読み替えない。項目が空、不一致、または使用不可の場合は該当frontendを公開しない。`ProductProfile`は使用可能な部分集合を抑止できるだけで、対応能力を新設または拡張できない。CS110の`STREAM_ID=INVALID_STREAM_ID(65535)`はselectorなしを表すAOSPの既定値として別に扱い、本表で明示selector値`65535`を拒否する規則と混同しない。""",
    "HAL selector proof paragraph",
)

text = sub_once(
    text,
    r"ただし、公開`ILnb`対応能力と、固定ディッシュ向けsatellite frontendの内部給電は別能力として扱う。.*?本書の「LNB機器の資源規則」「表7」「表8」「ワーカー終了契約」を適用する。",
    """ただし、公開`ILnb`対応能力とsatellite frontendの電源トポロジは別能力として扱う。`SupportedDeviceCapabilityCatalog`の機器項目は、`InternalFixed15V`、`ExternalOrShared`、`UnknownOrDisabled`のいずれかを保持する。`InternalFixed15V`は、物理rail owner、15 Vの適用確認方法、停止時の安全状態、共有互換条件を同じ項目に持ち、frontend generation開始前に既存の機器単位rail leaseを取得して15 Vを実適用できる場合だけ成立する。`ExternalOrShared`は、給電主体、HALが電圧を変更しないこと、共有互換条件、選局中の給電継続を製品配線として確認できる場合だけ成立する。

`InternalFixed15V`または`ExternalOrShared`が検証済みでruntime LNB切替を必要としない場合、そのISDB-S frontendは`aidl_baseline_eligible_lnb_count=0`のまま公開してよい。前者ではHAL内部で選局前に固定15 Vを適用し、後者ではHALは電圧操作を行わない。いずれもframeworkから選択・変更できるLNB IDとして列挙せず、`IFrontend.setLnb()`成功を要求しない。`UnknownOrDisabled`、トポロジ証跡不一致、給電継続または共有互換性を確認できない場合はsatellite frontendを公開しない。給電、lease、tune準備失敗時の巻き戻し、安全状態復帰、共有rail参照管理、実状態不明時の隔離は、本書の「LNB機器の資源規則」「表7」「表8」「ワーカー終了契約」を適用する。`FixedDishPowerProfile`その他の専用profileや別状態機械を設けない。""",
    "HAL satellite power topology",
)
text = replace_once(
    text,
    "| px4_drv feat/android-ddk | c2a031db8771ddd6e3e0b3b4a712b64ec384139b | 現行profileでは非公開 | 0 Vまたは15 Vのみ。tone、position、DiSEqCの実処理証跡なし | `aidl_baseline_eligible=false`。`getLnbIds()`へ出さず、LNB給電を要するsatellite frontendも公開しない | 内部電圧backendをAOSP LNB leaseとして生成しない | `driver/px4_device.c`のblob cfed72f...、`driver/ptx_chrdev.c`のblob 18f074... |\n| earth_pt1 Linux v6.6 | ffc253263a1375a65fa6c9f62a893e9767fbebfa | 現行profileでは非公開 | `pt1.c`では`SEC_VOLTAGE_13`を11 V、`SEC_VOLTAGE_18`を15 Vに対応付ける。tone、position、DiSEqCの実処理証跡なし | `aidl_baseline_eligible=false`。`getLnbIds()`へ出さず、LNB給電を要するsatellite frontendも公開しない | 内部電圧backendをAOSP LNB endpointとして生成しない | Linux v6.6 commitの`drivers/media/pci/pt1/pt1.c` |",
    "| px4_drv feat/android-ddk | c2a031db8771ddd6e3e0b3b4a712b64ec384139b | 公開`ILnb`は非公開 | 0 Vまたは15 Vのみ。tone、position、DiSEqCの実処理証跡なし | `aidl_baseline_eligible=false`。`getLnbIds()`へ出さない。機器項目が`InternalFixed15V`ならHAL内固定15 V、`ExternalOrShared`なら電圧非操作でISDB-S frontendを公開可能。`UnknownOrDisabled`なら非公開 | 公開LNB leaseは生成せず、固定15 V時だけ既存の機器rail lease・rollback・safe-state規則を使う | `driver/px4_device.c`のblob cfed72f...、`driver/ptx_chrdev.c`のblob 18f074... |\n| earth_pt1 Linux v6.6 | ffc253263a1375a65fa6c9f62a893e9767fbebfa | 公開`ILnb`は非公開 | `pt1.c`では`SEC_VOLTAGE_13`を11 V、`SEC_VOLTAGE_18`を15 Vに対応付ける。tone、position、DiSEqCの実処理証跡なし | `aidl_baseline_eligible=false`。`getLnbIds()`へ出さない。機器項目が`InternalFixed15V`ならHAL内固定15 V、`ExternalOrShared`なら電圧非操作でISDB-S frontendを公開可能。`UnknownOrDisabled`なら非公開 | 公開LNB endpointは生成せず、固定15 V時だけ既存の機器rail lease・rollback・safe-state規則を使う | Linux v6.6 commitの`drivers/media/pci/pt1/pt1.c` |",
    "HAL LNB resource rows",
)

# Static assertions for the four review points.
for forbidden in (
    "`TableInfo repeat=false`を成功させる固定対応範囲",
    "現行製品profileで成功させるPES設定はARIB字幕用の明示`0xBD`だけ",
    "px4では受信TMCCから要求TSIDに対応する相対slotを実行時に解決",
    "LNB給電を要するsatellite frontendも公開しない",
):
    if forbidden in text:
        raise SystemExit(f"forbidden stale contract remains: {forbidden}")

for required in (
    "NIT other、BAT、SDT other、EITを含め、table種別だけを理由にconfigure拒否しない",
    "全ての有効な明示stream IDとwildcardを通常のPES filter設定として受理",
    "px4では`RELATIVE_STREAM_NUMBER 0..7`と`STREAM_ID 12..65534`をlegacy `slot`へ直接渡す",
    "`InternalFixed15V`、`ExternalOrShared`、`UnknownOrDisabled`",
):
    if required not in text:
        raise SystemExit(f"required contract missing: {required}")

path.write_text(text, encoding="utf-8")
