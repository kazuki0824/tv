from pathlib import Path
import re


def replace_required(text: str, old: str, new: str, label: str) -> str:
    if new in text:
        return text
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{label}: expected one occurrence, got {count}")
    return text.replace(old, new)


def sub_required(text: str, pattern: str, new: str, label: str) -> str:
    if new in text:
        return text
    updated, count = re.subn(pattern, new, text, count=1, flags=re.S)
    if count != 1:
        raise SystemExit(f"{label}: expected one regex occurrence, got {count}")
    return updated


# TIS owns only public Tuner inputs and its own decoder/queue limits.
path = Path("tis/DESIGN_JA.md")
text = path.read_text(encoding="utf-8")
text = replace_required(
    text,
    "BSはIF周波数とtyped stream selectorを保持する。現行製品ではearth_pt1 / DVB backendはabsolute TSID `0..65534`だけを許容し、px4 backendは相対TS番号`0..7`だけを許容する。px4のabsolute TSID対応は、HAL側で別の将来能力として有効化された場合にだけ候補生成へ追加する。CS110は周波数帯だけでscan candidateとtune selectorを作り、stream selectorを保存しない。",
    "BSはIF周波数とAOSP Tuner公開契約のtyped stream selectorを保持する。通常のscan候補、channel保存、再選局ではbackend種別に依存せず、`STREAM_ID`のTSID `0..65534`だけを使用する。TISはpx4の相対slot、Linux DVBの`DTV_STREAM_ID`、HAL内部のbackend capabilityを取得・推測・保存しない。CS110は周波数帯だけでscan candidateとtune selectorを作り、stream selectorを保存しない。",
    "TIS BS public boundary",
)
text = replace_required(
    text,
    "CS110 tune request 生成時、TIS は Android Tuner API builder の default `streamId` / `streamIdType` に依存しない。CS110 では frontend stream selector を明示的に none / `UNDEFINED` 相当に設定する。CS110 の ONID / TSID / service_id は channel identity / サービス識別子 として保持してよいが、HAL frontend selector へ転用してはならない。BS は IF 周波数 + TSID、または px4 backend 限定の relative stream number を使う。",
    "CS110 tune request 生成時、TIS は Android Tuner API builder の default `streamId` / `streamIdType` に依存しない。CS110 では frontend stream selector を明示的に none / `UNDEFINED` 相当に設定する。CS110 の ONID / TSID / service_id は channel identity / サービス識別子 として保持してよいが、HAL frontend selector へ転用してはならない。BSはIF周波数と`STREAM_ID`のTSIDを使い、driver固有の相対番号への変換はTuner HAL内部へ閉じる。",
    "TIS tune request",
)
text = replace_required(
    text,
    "TvProvider の channel internal provider data には JSON v1 `tune.streamIdType` と `tune.streamId` を保存する。`NONE` は `streamId=null`、`TSID` は `0..65534`、`RELATIVE` は `0..7` とする。`65535`はAOSP `INVALID_STREAM_ID`であり、実TSIDとして保存または再投入しない。",
    "TvProvider の channel internal provider data には JSON v1 `tune.streamIdType` と `tune.streamId` を保存する。通常製品経路で書き込む値は、`NONE` の `streamId=null`、または `TSID` の `0..65534`だけとする。`65535`はAOSP `INVALID_STREAM_ID`であり、実TSIDとして保存または再投入しない。`RELATIVE`はdriver固有値になるため、TISの通常channelデータへ保存しない。",
    "TIS provider selector schema",
)
text = replace_required(
    text,
    "BSの通常実行時候補は、TISが保持する候補データとHALのeffective backend capabilityから生成する。現行px4にはTIS所有のrelative候補表から`RELATIVE_STREAM_NUMBER 0..7`を生成し、earth_pt1 / DVBにはTIS所有のTSID表から`STREAM_ID 0..65534`を生成する。将来px4 absolute能力がHALで有効になった場合だけTSID候補を追加する。HAL/backendにTSIDからrelative slotへの変換表を置かず、能力外selectorを通常候補へ混入させない。",
    "BSの通常実行時候補は、TISが保持するBS TSID表からIF周波数と`STREAM_ID 0..65534`として生成する。TISはHALのeffective capabilityやdriver名で候補を分岐しない。Tuner HALは同じ公開requestを各backendへ変換し、px4では受信TMCCから要求TSIDに対応する相対slotを実行時に解決する。固定のTSID→slot候補表をHALへ複製してはならない。",
    "TIS normal candidates",
)
text = replace_required(
    text,
    "MediaEvent payloadは、`offset >= 0`、`dataLength > 0`、加算overflowなし、`offset + dataLength <= mapped buffer capacity`を満たす場合だけdecoder queueへ渡す。sample byte上限は固定4 MiBにせず、同一製品profileから生成されるHAL per-event予算と、選択したMediaCodecの入力上限の小さい方をTISの受付上限とする。TISは共有領域方式とイベント固有fd方式の両方を受け付け、実際の`dataLength`をpending byte予算へ予約する。上限超過は構文不正として黙って破棄せず、対応codec/profileの上限超過として診断し、再生継続不能なら`notifyVideoUnavailable()`へ接続する。",
    "MediaEvent payloadは、`offset >= 0`、`dataLength > 0`、加算overflowなし、`offset + dataLength <= mapped buffer capacity`を満たす場合だけdecoder queueへ渡す。TISは共有領域方式とイベント固有fd方式の両方を受け付け、選択したMediaCodecの入力上限とTIS自身のpending queue byte予算だけを受付判定に使う。HALの`avPerFilterLiveBytes`、`avRuntimeBudgetBytes`その他の未解放payload集約台帳をTISへ公開・複製・1イベント上限化しない。codec入力またはTIS queue予算を超える場合は診断し、再生継続不能なら`notifyVideoUnavailable()`へ接続する。",
    "TIS media event budget",
)
path.write_text(text, encoding="utf-8")


# Cross-module rule: TIS emits canonical TSID; HAL owns backend conversion.
path = Path("開発規則.md")
text = path.read_text(encoding="utf-8")
text = replace_required(
    text,
    "- 現行製品のpx4能力は、BSの相対TS番号 `0..7` に限定する。absolute TSID能力は、機器・driver契約版・functional probeを持つ別の将来能力としてのみ有効化する",
    "- px4 legacy driverはBS選択を相対slotで実行するが、Tuner公開面ではTSIDを受け付ける。HALは周波数選局後に受信TMCCから要求TSIDの相対slotを実行時に解決し、driver固有値をTISへ公開しない",
    "root px4 fact",
)
text = sub_required(
    text,
    r"## BS/CS選局方法\n.*?\n\n## 日本向け scan / 選局契約",
    """## BS/CS選局方法
日本のISDB-Sでは、BSは同一周波数帯で複数TSを扱い、CS110は本製品の選局契約上、周波数だけで選局する。

TISが生成するBSの公開requestは、backendにかかわらずIF周波数とAOSP `STREAM_ID`のTSID `0..65534`に統一する。earth_pt1 / DVB backendはTSIDを`DTV_STREAM_ID`へ変換する。px4 backendは周波数選局後に受信TMCCを読み、要求TSIDと一致する相対slotを実行時に解決してlegacy APIへ渡す。TISへdriver種別、相対slot、HAL内部capabilityを公開せず、HALに固定のBS TSID候補表または静的TSID→slot表を置かない。

AOSP公開`RELATIVE_STREAM_NUMBER`は通常製品scan・channel保存・再選局では使用しない。直接指定を対応させる場合もHAL内部の追加入力契約であり、TIS候補生成の分岐条件にしてはならない。

## 日本向け scan / 選局契約""",
    "root BS selection section",
)
text = sub_required(
    text,
    r"px4 backend は、TIS から渡された explicit tune request を px4 legacy API の `freq_no / slot / addfreq` へ落とす adapter に限定する。.*?px4 backend に BS TSID から相対TS番号へ変換する表を置いてはならない。",
    """px4 backendは、TISから渡されたIF周波数とabsolute TSIDをpx4 legacy APIへ変換するadapterに限定する。まず周波数を設定し、同一tune generationで取得したTMCCからTSIDと相対slotの対応を構築し、要求TSIDに一致するslotだけを`slot`へ渡す。この対応は当該受信generationの動的観測であり、製品候補表や永続的なTSID→slot表として保持しない。要求TSIDが有効値だが受信TMCCに存在しない場合は、公開`tune()`受付成功後の非同期終端を`NO_SIGNAL`とする。TMCC取得またはbackend状態を確定できない失敗は表19のbackend失敗分類に従う。""",
    "root px4 adapter",
)
text = sub_required(
    text,
    r"frontend_px4 は scan policy を持たず、px4_drv legacy ioctl adapter として、TIS から渡された explicit tune request を px4_drv の `freq_no / slot / addfreq` へ変換するだけにする。.*?本書の選局契約と TIS の製品用候補表を置き換える表を持ってはならない。",
    """frontend_px4はscan policyを持たず、AOSPのIF周波数+TSID requestをpx4_drvの`freq_no / slot / addfreq`へ変換するだけにする。相対slotは同一generationのTMCC観測から導出し、本書の選局契約とTISの製品用候補表を置き換える固定表を持ってはならない。""",
    "root frontend px4",
)
text = sub_required(
    text,
    r"px4の将来absolute `STREAM_ID`能力は、.*?互換性目的で frontend_px4 に TSID→relative slot 変換表を再導入してはならない。",
    """px4のISDB-S frontendを製品へ公開する条件は、対象driverでTMCC取得とTSID→相対slotの動的解決が検証済みであることとする。検証不能なpx4 ISDB-S frontendは公開せず、TIS側を相対番号へ分岐させて補償しない。互換性目的でfrontend_px4へ静的TSID→relative slot表を導入してはならない。""",
    "root px4 export condition",
)
text = sub_required(
    text,
    r"## BS/CS110 stream selector境界\n.*?\n\n## BS/CS110 selector固定テスト",
    """## BS/CS110 stream selector境界
BSの通常製品経路では、TISはtyped `STREAM_ID`のTSID `0..65534`だけを生成する。HALはselectorのkindを正として値域から種類を推測せず、earth_pt1 / DVBではTSIDをそのままbackendへ渡し、px4では同一generationのTMCCから相対slotへ変換する。`0..11`もabsolute TSIDとして扱い、数値重複だけを理由に拒否しない。TISは`RELATIVE_STREAM_NUMBER`を通常候補またはchannelデータへ使用しない。

CS110では、channel key/サービス識別子にONID/TSID/service_idを保持してよいが、HAL frontend selectorへ転用しない。stream selectorはNONEとし、HALは周波数だけで選局する。AOSP SDK defaultの`streamIdType=STREAM_ID`かつ`streamId=INVALID_STREAM_ID(0xFFFF)`はselectorなしとして吸収する。CS110にTSID、relative selector、負値selectorが指定された場合は`INVALID_ARGUMENT`とする。

## BS/CS110 selector固定テスト""",
    "root selector boundary",
)
text = sub_required(
    text,
    r"## BS/CS110 selector固定テスト\n.*?\n\n## px4_drv系のロック方法",
    """## BS/CS110 selector固定テスト
次の契約を単体テストで固定し、必要に応じて静的確認で補強すること。

- TISがBSの通常候補をbackendにかかわらず`STREAM_ID`のTSIDで生成し、HAL内部capabilityまたは相対slotを参照しないこと。
- earth_pt1 / DVB backendがBS TSID `0..65534`を受け付け、`0..11`もabsolute値として処理すること。
- px4 backendが同じ公開TSID requestを受け、同一generationのTMCCから対応slotを解決してdriverへ渡すこと。
- px4のTMCCに要求TSIDが存在しない場合に、別TSへ誤選局せず`NO_SIGNAL`へ終端すること。
- frontend_px4が固定TSID→slot表を持たず、TISがdriver名またはHAL内部capabilityで候補を分岐しないこと。
- TISがCS110 tune requestにstream selectorを付けないこと。
- HALがCS110のTSID、相対TS番号、負値selectorを`INVALID_ARGUMENT`とし、AOSP SDK defaultの`STREAM_ID / INVALID_STREAM_ID(0xFFFF)`だけをselectorなしとして吸収すること。

## px4_drv系のロック方法""",
    "root selector tests",
)
path.write_text(text, encoding="utf-8")


# Tuner HAL owns backend conversion and a fixed TableInfo repeat=false support matrix.
path = Path("tuner_hal/DESIGN_JA.md")
text = path.read_text(encoding="utf-8")
text = sub_required(
    text,
    r"ISDB-Sのセレクター対応能力は、機器識別子と対象リビジョン、versioned backend manifestのABI/API契約版、および起動時functional probeの結果が一致する`SupportedDeviceCapabilityCatalog`の検証済み項目からだけ生成する。.*?実行時は不変の`EffectiveCapabilities`だけを参照する。",
    """ISDB-S frontendを公開する場合、通常製品経路のAOSP公開selector契約はbackendにかかわらず`STREAM_ID`のTSID `0..65534`へ統一する。`65535`は`Constant.INVALID_STREAM_ID`であり、明示selector値としては使用しない。Linux DVB / earth_pt1はTSIDを`DTV_STREAM_ID`へ渡す。px4は、機器識別子、driver契約版、TMCC取得、同一generationでのTSID→相対slot解決を`SupportedDeviceCapabilityCatalog`とfunctional probeで検証できる場合だけISDB-S frontendを公開し、公開TSID requestを内部slotへ変換する。TISへ`EffectiveCapabilities`、driver名、relative slotを公開しない。`RELATIVE_STREAM_NUMBER`は現行の通常product/VTS profileでは使用せず、有効値を直接指定された場合はbackendを変更せず`UNAVAILABLE`とする。`ProductProfile`は検証済み能力を抑止できるが、新設または拡張してはならない。""",
    "HAL selector capability paragraph",
)
text = sub_required(
    text,
    r"セレクターの基本対応は次のとおりとする。Linux DVBはISDB-Sの `STREAM_ID` として `0..65534` を受け付け、値を変更せず `DTV_STREAM_ID` へ渡す。.*?ISDB-T、CATV、CS110ではISDB-S用セレクターを使用しない。",
    """セレクターの基本対応は次のとおりとする。BSの通常公開入力は`STREAM_ID 0..65534`に統一する。Linux DVB / earth_pt1は値を変更せず`DTV_STREAM_ID`へ渡す。px4は周波数設定後、同一tune generationで読み取ったTMCCから要求TSIDに一致する相対slotを解決し、legacy `slot`へ渡す。固定のTSID→slot表、TISからのbackend hint、数値域によるselector種別推測を使わない。要求TSIDがTMCCに存在しない場合は別TSへfallbackせず`NO_SIGNAL`へ終端する。TMCC取得またはbackend副作用を確定できない場合は表19の失敗分類に従う。`RELATIVE_STREAM_NUMBER`は現行product/VTS profileでは`UNAVAILABLE`とする。ISDB-T、CATV、CS110ではISDB-S用selectorを使用しない。""",
    "HAL basic selector paragraph",
)
text = sub_required(
    text,
    r"- セクションフィルターの`repeat=false`は重複抑止ではなく、同一`start\(\)`世代内の配送停止条件である。.*?filter objectの公開状態はStartedのまま維持し、利用側が明示的に`stop\(\)` / `flush\(\)` / `configure\(\)` / `close\(\)`を呼べる状態を保つ。",
    """- セクションフィルターの`repeat=false`は重複抑止ではなく、同一`start()`世代内の配送停止条件である。`SectionBits`は最初に一致したsectionを1件配送した後に停止する。`TableInfo`の候補照合条件はAOSP公開settingsのtable idとversionだけとし、configure可否をruntime `ProductProfile`のsubtable一覧や事前`last_section_number`へ依存させない。
- `TableInfo repeat=false`を成功させる固定対応範囲は、1つのfilter PIDとtable idの組が規格・製品配線上1つのsubtableだけを運ぶ組に限定する。現行対応は、PAT (`PID 0x0000 / table_id 0x00`)、CAT (`PID 0x0001 / 0x01`)、PATで選択した個別PMT PID (`0x02`)、TSDT (`PID 0x0002 / 0x03`)、NIT actual (`PID 0x0010 / 0x40`)、SDT actual (`PID 0x0011 / 0x42`) とする。NIT other、BAT、SDT other、EIT p/f・scheduleその他の同じPID/table idに複数extensionが並ぶ組は、`repeat=false`では`configure()`を`UNAVAILABLE`とし、`repeat=true`だけを許す。この固定表は公開settingsとfilter PIDから一意に判定し、起動時profile内容で成否を変えない。
- 成功した`TableInfo repeat=false`は、最初に受理した有効sectionから`table_id_extension`、version、`last_section_number`をruntime完了状態として記録し、同じsubtableの`section_number=0..last_section_number`を各1回配送して停止する。version wildcardは最初に受理したversionへ固定する。完了前に別extensionまたは異なる`last_section_number`が来た場合は固定対応範囲違反として配送集合へ混ぜず、型付き診断を記録し、元のsubtableが完了するまで停止しない。`repeat=true`はtable id/versionに一致する全sectionを繰り返し配送する。この配送停止は公開`IFilter.stop()`と同じ状態遷移ではなく、filter objectの公開状態はStartedのまま維持し、利用側が明示的に`stop()` / `flush()` / `configure()` / `close()`を呼べる状態を保つ。""",
    "HAL TableInfo repeat contract",
)
text = replace_required(
    text,
    "| T-SEC-14 | `TableInfo repeat=false` | 最初に受理したtable id / extension / version / last sectionを固定し、同じsubtableの`0..last_section_number`だけで完了 |\n| T-SEC-14a | 完了前に同じtable id/versionの別extensionを受信 | 固定済み集合へ混ぜず配送しない |",
    "| T-SEC-14 | 固定対応表のsingle-subtable bindingで`TableInfo repeat=false` | configure成功後、受信sectionからextension/version/lastを構築し、`0..last_section_number`を各1回配送して停止 |\n| T-SEC-14a | NIT other / BAT / SDT other / EIT等のmulti-subtable bindingで`repeat=false` | 固定対応表によりconfigure時`UNAVAILABLE`。起動時profile内容で結果を変えない |\n| T-SEC-14b | single-subtable binding完了前に別extensionまたは異なるlastを受信 | 元の完了集合へ混ぜず配送せず、診断を記録し、元の`0..last`完了前に停止しない |",
    "HAL TableInfo tests",
)
path.write_text(text, encoding="utf-8")
