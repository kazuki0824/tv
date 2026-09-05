from pathlib import Path
import json


def replace_once(path: str, old: str, new: str) -> None:
    p = Path(path)
    text = p.read_text()
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"置換対象が一意ではありません: {path} count={count}")
    p.write_text(text.replace(old, new, 1))


# CA_descriptor由来factとfree_CA_modeを独立させる。
replace_once(
    "tis/src/com/maleicacid/tvinput/aribsi/SiModels.kt",
    "    val requiresCas: Boolean get() = serviceScopedCaDescriptors.isNotEmpty() || freeCaMode == true\n",
    "    val requiresCas: Boolean get() = serviceScopedCaDescriptors.isNotEmpty()\n",
)

# Data Component DescriptorのDMF/Timing/automatic presentation factをcanonical componentへ保持する。
replace_once(
    "tis/src/com/maleicacid/tvinput/aribsi/SiModels.kt",
    "    val dataComponentId: Int? = null,\n    val captionServiceKind: String? = null,\n",
    "    val dataComponentId: Int? = null,\n    val captionDmf: Int? = null,\n    val captionTiming: Int? = null,\n    val automaticPresentationOnReception: Boolean? = null,\n    val captionServiceKind: String? = null,\n",
)
replace_once(
    "tis/src/com/maleicacid/tvinput/aribsi/NativeAribSiParser.kt",
    "                        dataComponentId = stream.dataComponentId,\n                        language = null,\n",
    "                        dataComponentId = stream.dataComponentId,\n                        captionDmf = stream.captionDmf,\n                        captionTiming = stream.captionTiming,\n                        automaticPresentationOnReception = stream.automaticPresentationOnReception,\n                        language = null,\n",
)
replace_once(
    "tis/src/com/maleicacid/tvinput/aribsi/ProviderDataBridge.kt",
    "                    .put(\"dataComponentId\", entry.dataComponentId ?: JSONObject.NULL)\n                    .put(\"language\", entry.language?.takeIf { it.isNotBlank() } ?: JSONObject.NULL)\n",
    "                    .put(\"dataComponentId\", entry.dataComponentId ?: JSONObject.NULL)\n                    .put(\"captionDmf\", entry.captionDmf ?: JSONObject.NULL)\n                    .put(\"captionTiming\", entry.captionTiming ?: JSONObject.NULL)\n                    .put(\"automaticPresentationOnReception\", entry.automaticPresentationOnReception ?: JSONObject.NULL)\n                    .put(\"language\", entry.language?.takeIf { it.isNotBlank() } ?: JSONObject.NULL)\n",
)
replace_once(
    "arib_si_engine_rs/src/core/provider_data.rs",
    "    data_component_id: Option<i64>,\n    language: Option<String>,\n    caption_service_kind: String,\n",
    "    data_component_id: Option<i64>,\n    caption_dmf: Option<i64>,\n    caption_timing: Option<i64>,\n    automatic_presentation_on_reception: Option<bool>,\n    language: Option<String>,\n    caption_service_kind: String,\n",
)
replace_once(
    "arib_si_engine_rs/src/core/provider_data.rs",
    "                \"dataComponentId\",\n                \"language\",\n                \"captionServiceKind\",\n",
    "                \"dataComponentId\",\n                \"captionDmf\",\n                \"captionTiming\",\n                \"automaticPresentationOnReception\",\n                \"language\",\n                \"captionServiceKind\",\n",
)

# Program provider-data schemaのsubtitle itemへ同じ3 factを追加する。
p = Path("arib_si_engine_rs/schema/program_provider_data_v1.schema.json")
data = json.loads(p.read_text())

def visit(node):
    if isinstance(node, dict):
        props = node.get("properties")
        if isinstance(props, dict) and "captionServiceKind" in props and "dataComponentId" in props:
            props["captionDmf"] = {"type": ["integer", "null"], "minimum": 0, "maximum": 15}
            props["captionTiming"] = {"type": ["integer", "null"], "minimum": 0, "maximum": 3}
            props["automaticPresentationOnReception"] = {"type": ["boolean", "null"]}
            required = node.setdefault("required", [])
            for name in ("captionDmf", "captionTiming", "automaticPresentationOnReception"):
                if name not in required:
                    required.append(name)
        for value in node.values():
            visit(value)
    elif isinstance(node, list):
        for value in node:
            visit(value)
visit(data)
p.write_text(json.dumps(data, ensure_ascii=False, indent=2) + "\n")

# SI段階ではdecoder availabilityを知らないため、final playback supportを名乗らずstatic eligibilityとする。
for path in (
    "tis/src/com/maleicacid/tvinput/aribsi/SiModels.kt",
    "tis/src/com/maleicacid/tvinput/aribsi/ServiceListBuilder.kt",
    "tis/src/com/maleicacid/tvinput/tis/ChannelScanController.kt",
):
    p = Path(path)
    text = p.read_text()
    text = text.replace("clearLivePlaybackSupported", "clearLivePlaybackStaticallyEligible")
    p.write_text(text)

p = Path("tis/DESIGN_JA.md")
text = p.read_text()
old = "TISはcurrent `ServiceSemanticFacts`から`requiresCas`を意味事実として受け取り、現在releaseの対応service type/codec、実decoder availability、CAS実装状態、TvProvider transaction条件と組み合わせて、serviceごとに `channelRegistrationReady`、`epgPublishable`、`clearLivePlaybackSupported`、`unsupportedCas` を算出する。このpolicy結果はTIS runtimeの一貫した判断材料であり、SI parserへ逆流させず、保存済みprovider-dataをcurrent policyのfallback sourceにしない。"
new = "TISはcurrent `ServiceSemanticFacts`から`requiresCas`を意味事実として受け取り、SI段階では現在releaseの対応service type/codecとCAS状態から `channelRegistrationReady`、`epgPublishable`、`clearLivePlaybackStaticallyEligible`、`unsupportedCas` を算出する。`clearLivePlaybackStaticallyEligible` はdecoderを開く前の静的候補factであり、`clearLivePlaybackSupported`を表明しない。実decoder availabilityはlive playback開始時のMediaCodec選択・configure成功を正本とし、static eligibilityを満たしてもdecoderを利用できないserviceは再生成功扱いにしない。このpolicy結果はSI parserへ逆流させず、保存済みprovider-dataをcurrent policyのfallback sourceにしない。"
if text.count(old) != 1:
    raise SystemExit("DESIGNのplayback policy段落が一意ではありません")
text = text.replace(old, new, 1)
text = text.replace("`channelRegistrationReady` / `epgPublishable` / `clearLivePlaybackSupported`にしない", "`channelRegistrationReady` / `epgPublishable` / `clearLivePlaybackStaticallyEligible`にしない", 1)
p.write_text(text)

# PRで新規追加した英語コメントを日本語化する（大規模/中規模fileのみ）。
comment_replacements = {
    "tis/src/com/maleicacid/tvinput/tis/TunerSelectionPolicy.kt": {
        "/** Pure stream and track selection policy. Android session lifecycle stays in MaleicacidLiveSession. */": "/** stream/track選択の純粋policy。Android session lifecycleはMaleicacidLiveSessionが所有する。 */",
    },
    "tis/platform_patches/lineage-22.1/frameworks_base_mediasync_first_output.patch": {
        "+     * Listener for the first video frame that MediaSync successfully queues to the output Surface.": "+     * MediaSyncがoutput Surfaceへ正常queueした最初のvideo frameを通知するlistener。",
        "+     * Arms a one-shot callback for the current MediaSync instance.": "+     * current MediaSync instanceに対するone-shot callbackをarmする。",
        "+    // Disarm is part of release cleanup and must not publish a stale first-output event.": "+    // release cleanupではdisarmし、古いfirst-output eventを公開しない。",
    },
}
for path, replacements in comment_replacements.items():
    p = Path(path)
    text = p.read_text()
    for old, new in replacements.items():
        if old in text:
            text = text.replace(old, new)
    p.write_text(text)
