from pathlib import Path
import json


def replace_one(path: str, old: str, new: str) -> None:
    p = Path(path)
    text = p.read_text(encoding="utf-8")
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{path}: expected exactly one match, got {count}: {old[:80]!r}")
    p.write_text(text.replace(old, new), encoding="utf-8")


# TIS owns Android/product policy; SI engine supplies semantic facts only.
replace_one(
    "tis/DESIGN_JA.md",
    """partial snapshot は サービス単位の登録可能判定に使ってよい。ただし partial snapshot を無条件に channel 登録へ出してはならない。global complete 判定だけで publish 可否を決めず、サービス / transport 単位の `publishability_by_service` と 登録可能判定で、service_id、TSID、ONID、PMT、PCR、必要 table、対応するaudioまたはvideo ESの欠落理由を分離する。登録可能サービスは、ONID / TSID / SID、PMT PID と PMT、有効 PCR、後続更新可能な internal key、および現行ライブ視聴で対応するaudioまたはvideo ESを持つサービスとする。video-only / audio-onlyというtrack構成は`TvContract.Channels.COLUMN_SERVICE_TYPE`の再分類根拠にせず、同列は`../ARIB_SI_EPG_TvProvider投影方針.md`に従ってARIB `service_type`のcodingを保持する。audio-onlyの視聴セッションでは`VIDEO_UNAVAILABLE_REASON_AUDIO_ONLY`を通知できるが、この値をchannel登録の禁止理由に使わない。音声・映像の欠落または未対応はtrack別診断に残す。scrambled サービスは 登録可能 として channel 登録してよいが、現行の平文ライブ視聴成功対応宣言対象にはしない。登録可能未満の partial snapshot は 診断情報 / ライブ更新 / debugに限定し、channel insert に使わない。""",
    """partial snapshot は、`arib_si_engine_rs` が返すサービス / transport単位の `ServiceSemanticFacts` を材料としてTISが登録可否を判定するために使ってよい。ただし partial snapshot を無条件に channel 登録へ出してはならない。`ServiceSemanticFacts` はONID / TSID / SID、ARIB `service_type`、PMT/PCRの存在・構文状態、ES一覧とcodec signaling、CA descriptor / free_CA_mode、SMD意味状態、欠落・不正理由までに限定し、`channelRegistrationReady`、`epgPublishable`、`clearLivePlaybackSupported`、`requiresCas`、`unsupportedCas`のような製品/TIF policy結果を含めない。TISはこれらのsemantic factsと現行product capability、decoder/CAS availability、TvProvider状態から登録可否・EPG公開可否・ライブ再生可否を算出する。登録可能サービスは、ONID / TSID / SID、PMT PID と PMT、有効 PCR、後続更新可能な internal key、および現行ライブ視聴で対応するaudioまたはvideo ESを持つサービスとする。video-only / audio-onlyというtrack構成は`TvContract.Channels.COLUMN_SERVICE_TYPE`の再分類根拠にせず、同列は`../ARIB_SI_EPG_TvProvider投影方針.md`に従ってARIB `service_type`のcodingを保持する。audio-onlyの視聴セッションでは`VIDEO_UNAVAILABLE_REASON_AUDIO_ONLY`を通知できるが、この値をchannel登録の禁止理由に使わない。音声・映像の欠落または未対応はTIS側のtrack別診断に残す。scrambled サービスはTISの登録policyでchannel登録してよいが、現行の平文ライブ視聴成功対応宣言対象にはしない。登録可能未満の partial snapshot は 診断情報 / ライブ更新 / debugに限定し、channel insert に使わない。

`TvTrackInfo` の `trackId` はAndroid/TIS runtimeの識別子であり、TISがcurrent serviceのcomponent identityからcurrent program内で一意になるよう決定する。`trackId`をARIB意味objectまたは永続`internal_provider_data`の正本にせず、Rust SI parserへ逆流させない。""",
)

replace_one(
    "arib_si_engine_rs/DESIGN_JA.md",
    """`arib_si_engine_rs` は、service / transport単位の意味解析結果として、ONID / TSID / SID、ARIB `service_type`のraw 8-bit値、PMT、PCR、audio/video ESの存在・欠落理由、scrambling情報、および`publishability_by_service`を構造化してTISへ渡す。Android channelを登録するか、partial snapshotをchannel insertへ使用するかはTISの責務であり、`../tis/DESIGN_JA.md`を正とする。`Channels.COLUMN_SERVICE_TYPE`への最終投影は`../ARIB_SI_EPG_TvProvider投影方針.md`を正とし、本crateはAndroid generic `TvContract.Channels.SERVICE_TYPE_*`への意味変換を行わない。`publishability_by_service`はservice / transport単位の登録判断材料を構造化してTISへ渡す意味解析結果であり、channel登録とchannel insertの最終判断はTISが行う。""",
    """`arib_si_engine_rs` は、service / transport単位の `ServiceSemanticFacts` として、ONID / TSID / SID、ARIB `service_type`のraw 8-bit値、PMT/PCRの存在・構文状態、audio/video/subtitle/data ES一覧とcodec signaling、CA descriptor / free_CA_mode、SMD意味状態、欠落・不正理由を構造化してTISへ渡す。`ServiceSemanticFacts` は放送信号から導ける事実と構文・意味解析結果だけを持ち、Android channel登録可否、EPG公開可否、現行productのdecoder/CAS対応可否、ライブ再生可否を算出しない。Android channelを登録するか、partial snapshotをchannel insertへ使用するかはTISの責務であり、`../tis/DESIGN_JA.md`を正とする。`Channels.COLUMN_SERVICE_TYPE`への最終投影は`../ARIB_SI_EPG_TvProvider投影方針.md`を正とし、本crateはAndroid generic `TvContract.Channels.SERVICE_TYPE_*`への意味変換を行わない。""",
)

replace_one(
    "arib_si_engine_rs/DESIGN_JA.md",
    """`publishability_by_service`では`NON_BROADCAST`、`UNDEFINED_BROADCAST_CLASS`、`UNSUPPORTED_BROADCAST_SYSTEM`、`UNDETERMINED_SMD`のいずれでも`channel_registration_ready=false`、`epg_publishable=false`、`clear_live_playback_supported=false`とする。意味解析・診断用の`publishable`自体はSMDだけでfalseにしない。`SUPPORTED_BROADCAST`の場合だけSMD gateを通過したものとして、PMT、PCR、service type、codec、CAS等の既存条件で最終判定する。`UNDETERMINED_SMD`は再取得によって正常なSMDを得た時点で再評価し、SMD適合を肯定する根拠には使わない。Android channel登録と視聴セッションの最終制御は引き続きTISが所有する。""",
    """本crateはSMDについて上記の意味状態と、その根拠となるraw値・構文診断だけを`ServiceSemanticFacts`へ出力する。`NON_BROADCAST`、`UNDEFINED_BROADCAST_CLASS`、`UNSUPPORTED_BROADCAST_SYSTEM`、`UNDETERMINED_SMD`をAndroid channel登録、EPG公開、ライブ再生可否のbooleanへ変換しない。`UNDETERMINED_SMD`は再取得によって正常なSMDを得た時点で意味状態を再評価し、SMD適合を肯定する根拠には使わない。SMD意味状態を他のPMT/PCR/service type/codec/CAS事実と組み合わせて製品policyを決める責務はTISが所有する。""",
)

replace_one(
    "arib_si_engine_rs/DESIGN_JA.md",
    """Kotlin/JNI の通常 サービススナップショット は channel registration 用の `registration_ready_snapshot()` 相当を使う。これは現行の平文ライブ視聴対応宣言対象だけでなく、サービス単位の登録可能条件を満たす scrambled unsupported サービス も含み得る。平文ライブ視聴対応宣言対象は別途 `clear_live_playback_supported_snapshot()` / `clear_live_playback_supported` で判定する。`publishable_snapshot()` は診断・test 用であり、登録可能未満の サービスを通常 channel 登録経路に出さない。publishable だが現行ライブ視聴対象外の サービスについては `publishability_by_service` を JNI 診断として公開し、ONID、TSID、service_id、publishable / channel_registration_ready / epg_publishable / clear_live_playback_supported / requires_cas / unsupported_cas 可否、欠落 component、除外理由を分けて観測する。""",
    """Kotlin/JNI の通常サービス境界は、channel registrationやplayback policyを確定済みのsnapshotではなく、service / transport単位の `ServiceSemanticFacts` bulk snapshotとする。snapshotはONID / TSID / SID、ARIB `service_type`、PMT/PCRの存在・構文状態、ES/component一覧とcodec signaling、CA descriptor / free_CA_mode、SMD意味状態、欠落・不正理由を返す。`registration_ready_snapshot()`、`clear_live_playback_supported_snapshot()`、`publishability_by_service`のようにAndroid/TIS/product policyをRust側で確定する公開境界は設計しない。TISが`ServiceSemanticFacts`とcurrent product capabilityから`channelRegistrationReady`、`epgPublishable`、`clearLivePlaybackSupported`、`requiresCas`、`unsupportedCas`を算出し、その判断をchannel登録、Programs公開、視聴セッションへ一貫して使用する。""",
)

replace_one(
    "arib_si_engine_rs/DESIGN_JA.md",
    """Programs の `internal_provider_data` には、`requiresCas`, `unsupportedCas`, `clearLivePlaybackSupported`, `channelRegistrationReady`, `epgPublishable`, `publishStateSource` 相当の CAS / 準備状態を `cas` または診断情報に保存する。視聴年齢制限については `countryCode`, `ratingValue`, `rawRatingByte`, `supported`, `parseStatus`, `mappedTvContentRating` 相当の情報を `ratings` または診断情報に保存する。現在の診断情報が完全であれば、その値を Programs CAS 状態の正とする。診断情報が欠落または不完全な場合、既存 channel の `internal_provider_data` から CAS / 準備状態を代替参照して Programs 側に保存する。channel 側だけに保存して Programs 側を false に落としてはならない。""",
    """Programs の `internal_provider_data` にTISが算出した`requiresCas`、`unsupportedCas`、`clearLivePlaybackSupported`、`channelRegistrationReady`、`epgPublishable`等を保存する場合、それらはTISからprovider-data builderへ明示的に渡された投影・診断スナップショットとしてだけ保持する。`arib_si_engine_rs` はSI意味解析からこれらを算出・補完・再構築せず、保存済みchannel/Program provider-dataをcurrent policyのfallback sourceとして使用しない。currentのchannel登録、EPG公開、CAS対応、ライブ再生可否は、TISがcurrent `ServiceSemanticFacts`とcurrent product capabilityから毎回決定する。視聴年齢制限のAndroid `TvContentRating` 投影結果を保存する場合もTIS入力値の保存に限り、Rust parserの意味解析結果またはpolicy sourceへ逆流させない。""",
)

replace_one(
    "arib_si_engine_rs/DESIGN_JA.md",
    """`components.video[]` は ES PID、stream_type、component_tag、component_type、codec、解像度、走査方式、aspect、profile / level、根拠 descriptor を ES/component 単位で保持する。`components.audio[]` は ES PID、stream_type、component_tag、component_type、codec、ISO639 language、channel configuration、sampling info、根拠 descriptor を ES/component 単位で保持する。`components.subtitle[]` は ES PID、component_tag、data_component_id、ISO639 language、TIS trackId、caption サービス kind、parse_status を保持する。`components.data[]` はデータ component の メタデータを保持するが、BML / data broadcast 実行状態や UI 状態は保持しない。

`video` と `audio` は実際に主track 候補として選択された component の要約であり、未選択の場合は `null` とする。codecメタデータの認識は ライブ viewable / playable 対応宣言を意味しない。unsupported codec、decoder unavailable、transport profile out of scope は 診断情報に保存する。`ProgramProviderDataV1.components.video[]` / `components.audio[]` にrelease固有またはruntime capability判定の `r51PlaybackSupported` / `liveViewableClaim` を保存せず、再生可否はTIS runtimeの製品policyとdecoder capability判定に閉じる。""",
    """`components.video[]` は ES PID、stream_type、component_tag、component_type、codec signaling、解像度、走査方式、aspect、profile / level、根拠 descriptor を ES/component 単位で保持する。`components.audio[]` は ES PID、stream_type、component_tag、component_type、codec signaling、ISO639 language、channel configuration、sampling info、根拠 descriptor を ES/component 単位で保持する。`components.subtitle[]` は ES PID、component_tag、data_component_id、ISO639 language、caption サービス kind、parse_status を保持し、Android/TIS runtimeの`trackId`を保持しない。`components.data[]` はデータ component のメタデータを保持するが、BML / data broadcast 実行状態やUI状態は保持しない。

`video` と `audio` はSI/descriptorから得られたcomponent情報の保存用要約であり、TIS runtimeが実際に選択したtrack、decoder availability、playback capabilityを表さない。codec metadataの認識はライブviewable / playable対応宣言を意味しない。`ProgramProviderDataV1.components.video[]` / `components.audio[]` にrelease固有またはruntime capability判定の `r51PlaybackSupported` / `liveViewableClaim` を保存せず、再生可否とtrack選択はTIS runtimeの製品policyとdecoder capability判定に閉じる。""",
)

# Persisted subtitle components must not contain TIS runtime track identifiers.
schema_path = Path("arib_si_engine_rs/schema/program_provider_data_v1.schema.json")
data = json.loads(schema_path.read_text(encoding="utf-8"))
subtitle = data["$defs"]["subtitleComponent"]
subtitle["required"] = [x for x in subtitle["required"] if x != "trackId"]
subtitle["properties"].pop("trackId", None)
subtitle["not"] = {"required": ["trackId"]}
schema_path.write_text(json.dumps(data, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")

# Static completion checks: exhaustive for this design correction, not fail-fast.
errors = []
engine = Path("arib_si_engine_rs/DESIGN_JA.md").read_text(encoding="utf-8")
tis = Path("tis/DESIGN_JA.md").read_text(encoding="utf-8")
schema_text = schema_path.read_text(encoding="utf-8")
for forbidden in (
    "TIS trackId",
    "registration_ready_snapshot()",
    "clear_live_playback_supported_snapshot()",
):
    if forbidden in engine:
        errors.append(f"engine design still contains forbidden policy boundary: {forbidden}")
if '"trackId"' in schema_text:
    errors.append("program provider-data schema still accepts trackId")
if "ServiceSemanticFacts" not in engine or "ServiceSemanticFacts" not in tis:
    errors.append("semantic-facts boundary not established in both design owners")
if errors:
    raise SystemExit("\n".join(errors))
