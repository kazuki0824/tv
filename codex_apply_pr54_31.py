from pathlib import Path
import re

ROOT = Path('.')

def read(path):
    return (ROOT / path).read_text(encoding='utf-8')

def write(path, text):
    (ROOT / path).write_text(text, encoding='utf-8')

def replace_once(path, old, new, label):
    text = read(path)
    count = text.count(old)
    if count != 1:
        raise SystemExit(f'{label}: expected 1 occurrence, found {count}')
    write(path, text.replace(old, new, 1))

# --- Preserve the complete ARIB component facts through Kotlin/Rust provider-data. ---
replace_once(
    'tis/src/com/maleicacid/tvinput/aribsi/SiModels.kt',
    '''data class AribComponentEntry(\n    val esPid: TsPid?,\n    val streamType: Int? = null,\n    val componentTag: Int? = null,\n    val componentType: Int? = null,\n    val codec: String? = null,\n    val language: String? = null,\n    val secondLanguage: String? = null,\n    val channelConfiguration: String? = null,\n    val samplingInfo: String? = null,\n    val sourceDescriptor: String? = null,\n''',
    '''data class AribComponentEntry(\n    val esPid: TsPid?,\n    val streamType: Int? = null,\n    val streamContent: Int? = null,\n    val componentTag: Int? = null,\n    val componentType: Int? = null,\n    val codec: String? = null,\n    val language: String? = null,\n    val secondLanguage: String? = null,\n    val channelConfiguration: String? = null,\n    val simulcastGroupTag: Int? = null,\n    val samplingRate: Int? = null,\n    val samplingInfo: String? = null,\n    val text: String? = null,\n    val sourceDescriptor: String? = null,\n''',
    'SiModels component facts',
)

replace_once(
    'tis/src/com/maleicacid/tvinput/aribsi/NativeAribSiParser.kt',
    '''            streamType = optIntOrNull(obj, "streamType"),\n            componentTag = optIntOrNull(obj, "componentTag"),\n            componentType = optIntOrNull(obj, "componentType"),\n            codec = optStringOrNull(obj, "codec"),\n            language = optStringOrNull(obj, "language"),\n            secondLanguage = optStringOrNull(obj, "secondLanguage"),\n            channelConfiguration = optStringOrNull(obj, "channelConfiguration"),\n            samplingInfo = optStringOrNull(obj, "samplingInfo"),\n            sourceDescriptor = optStringOrNull(obj, "sourceDescriptor"),\n''',
    '''            streamType = optIntOrNull(obj, "streamType"),\n            streamContent = optIntOrNull(obj, "streamContent"),\n            componentTag = optIntOrNull(obj, "componentTag"),\n            componentType = optIntOrNull(obj, "componentType"),\n            codec = optStringOrNull(obj, "codec"),\n            language = optStringOrNull(obj, "language"),\n            secondLanguage = optStringOrNull(obj, "secondLanguage"),\n            channelConfiguration = optStringOrNull(obj, "channelConfiguration"),\n            simulcastGroupTag = optIntOrNull(obj, "simulcastGroupTag"),\n            samplingRate = optIntOrNull(obj, "samplingRate"),\n            samplingInfo = optStringOrNull(obj, "samplingInfo"),\n            text = optStringOrNull(obj, "text"),\n            sourceDescriptor = optStringOrNull(obj, "sourceDescriptor"),\n''',
    'Native parser component facts',
)

replace_once(
    'tis/src/com/maleicacid/tvinput/aribsi/ProviderDataBridge.kt',
    '''                .put("streamType", entry.streamType ?: JSONObject.NULL)\n                .put("componentTag", entry.componentTag ?: JSONObject.NULL)\n                .put("componentType", entry.componentType ?: JSONObject.NULL)\n                .put("codec", entry.codec?.takeIf { it.isNotBlank() } ?: JSONObject.NULL)\n                .put("parseStatus", entry.parseStatus)\n''',
    '''                .put("streamType", entry.streamType ?: JSONObject.NULL)\n                .put("streamContent", entry.streamContent ?: JSONObject.NULL)\n                .put("componentTag", entry.componentTag ?: JSONObject.NULL)\n                .put("componentType", entry.componentType ?: JSONObject.NULL)\n                .put("codec", entry.codec?.takeIf { it.isNotBlank() } ?: JSONObject.NULL)\n                .put("language", entry.language?.takeIf { it.isNotBlank() } ?: JSONObject.NULL)\n                .put("text", entry.text?.takeIf { it.isNotBlank() } ?: JSONObject.NULL)\n                .put("parseStatus", entry.parseStatus)\n''',
    'video provider facts',
)
replace_once(
    'tis/src/com/maleicacid/tvinput/aribsi/ProviderDataBridge.kt',
    '''                .put("streamType", entry.streamType ?: JSONObject.NULL)\n                .put("componentTag", entry.componentTag ?: JSONObject.NULL)\n                .put("componentType", entry.componentType ?: JSONObject.NULL)\n                .put("codec", entry.codec?.takeIf { it.isNotBlank() } ?: JSONObject.NULL)\n                .put("language", entry.language?.takeIf { it.isNotBlank() } ?: JSONObject.NULL)\n                .put("secondLanguage", entry.secondLanguage?.takeIf { it.isNotBlank() } ?: JSONObject.NULL)\n                .put("parseStatus", entry.parseStatus)\n            entry.channelConfiguration?.let { obj.put("channelConfiguration", it) }\n            entry.samplingInfo?.let { obj.put("samplingInfo", it) }\n            entry.sourceDescriptor?.let { obj.put("sourceDescriptor", it) }\n''',
    '''                .put("streamType", entry.streamType ?: JSONObject.NULL)\n                .put("streamContent", entry.streamContent ?: JSONObject.NULL)\n                .put("componentTag", entry.componentTag ?: JSONObject.NULL)\n                .put("componentType", entry.componentType ?: JSONObject.NULL)\n                .put("codec", entry.codec?.takeIf { it.isNotBlank() } ?: JSONObject.NULL)\n                .put("language", entry.language?.takeIf { it.isNotBlank() } ?: JSONObject.NULL)\n                .put("secondLanguage", entry.secondLanguage?.takeIf { it.isNotBlank() } ?: JSONObject.NULL)\n                .put("simulcastGroupTag", entry.simulcastGroupTag ?: JSONObject.NULL)\n                .put("samplingRate", entry.samplingRate ?: JSONObject.NULL)\n                .put("text", entry.text?.takeIf { it.isNotBlank() } ?: JSONObject.NULL)\n                .put("main", entry.main ?: JSONObject.NULL)\n                .put("multiLingual", entry.multiLingual ?: JSONObject.NULL)\n                .put("qualityIndicator", entry.qualityIndicator ?: JSONObject.NULL)\n                .put("parseStatus", entry.parseStatus)\n            entry.channelConfiguration?.let { obj.put("channelConfiguration", it) }\n            entry.samplingInfo?.let { obj.put("samplingInfo", it) }\n            entry.sourceDescriptor?.let { obj.put("sourceDescriptor", it) }\n''',
    'audio provider facts',
)

replace_once(
    'arib_si_engine_rs/src/lib.rs',
    '''            serde_json::json!({\n                "componentTag": component.component_tag,\n                "componentType": component.component_type,\n                "resolution": resolution,\n                "scan": scan,\n                "aspect": aspect,\n                "profileLevel": serde_json::Value::Null,\n                "sourceDescriptor": "component_descriptor",\n                "parseStatus": "OK",\n            })\n''',
    '''            serde_json::json!({\n                "streamContent": component.stream_content,\n                "componentTag": component.component_tag,\n                "componentType": component.component_type,\n                "language": component.language_code,\n                "text": component.text,\n                "resolution": resolution,\n                "scan": scan,\n                "aspect": aspect,\n                "profileLevel": serde_json::Value::Null,\n                "sourceDescriptor": "component_descriptor",\n                "parseStatus": "OK",\n            })\n''',
    'Rust video event facts',
)
replace_once(
    'arib_si_engine_rs/src/lib.rs',
    '''            serde_json::json!({\n                "streamType": component.stream_type,\n                "componentTag": component.component_tag,\n                "componentType": component.component_type,\n                "language": component.language_code,\n                "secondLanguage": component.language_code_2,\n                "channelConfiguration": audio_channel_configuration(\n                    component.stream_content,\n                    component.component_type,\n                ),\n                "samplingInfo": audio_sampling_info(component.sampling_rate),\n                "sourceDescriptor": "audio_component_descriptor",\n                "main": component.main_component_flag,\n                "multiLingual": component.es_multi_lingual_flag,\n                "qualityIndicator": component.quality_indicator,\n                "parseStatus": "OK",\n            })\n''',
    '''            serde_json::json!({\n                "streamType": component.stream_type,\n                "streamContent": component.stream_content,\n                "componentTag": component.component_tag,\n                "componentType": component.component_type,\n                "language": component.language_code,\n                "secondLanguage": component.language_code_2,\n                "channelConfiguration": audio_channel_configuration(\n                    component.stream_content,\n                    component.component_type,\n                ),\n                "simulcastGroupTag": component.simulcast_group_tag,\n                "samplingRate": component.sampling_rate,\n                "samplingInfo": audio_sampling_info(component.sampling_rate),\n                "text": component.text,\n                "sourceDescriptor": "audio_component_descriptor",\n                "main": component.main_component_flag,\n                "multiLingual": component.es_multi_lingual_flag,\n                "qualityIndicator": component.quality_indicator,\n                "parseStatus": "OK",\n            })\n''',
    'Rust audio event facts',
)

replace_once(
    'arib_si_engine_rs/src/provider_data.rs',
    '''struct VideoComponentV1 {\n    es_pid: Option<i64>,\n    stream_type: Option<i64>,\n    component_tag: Option<i64>,\n    component_type: Option<i64>,\n    codec: Option<String>,\n''',
    '''struct VideoComponentV1 {\n    es_pid: Option<i64>,\n    stream_type: Option<i64>,\n    #[serde(default)]\n    stream_content: Option<i64>,\n    component_tag: Option<i64>,\n    component_type: Option<i64>,\n    codec: Option<String>,\n    #[serde(default)]\n    language: Option<String>,\n    #[serde(default)]\n    text: Option<String>,\n''',
    'Rust VideoComponentV1',
)
replace_once(
    'arib_si_engine_rs/src/provider_data.rs',
    '''struct AudioComponentV1 {\n    es_pid: Option<i64>,\n    stream_type: Option<i64>,\n    component_tag: Option<i64>,\n    component_type: Option<i64>,\n    codec: Option<String>,\n    language: Option<String>,\n    second_language: Option<String>,\n    channel_configuration: Option<String>,\n    sampling_info: Option<String>,\n    source_descriptor: Option<String>,\n    parse_status: String,\n}\n''',
    '''struct AudioComponentV1 {\n    es_pid: Option<i64>,\n    stream_type: Option<i64>,\n    #[serde(default)]\n    stream_content: Option<i64>,\n    component_tag: Option<i64>,\n    component_type: Option<i64>,\n    codec: Option<String>,\n    language: Option<String>,\n    second_language: Option<String>,\n    channel_configuration: Option<String>,\n    #[serde(default)]\n    simulcast_group_tag: Option<i64>,\n    #[serde(default)]\n    sampling_rate: Option<i64>,\n    sampling_info: Option<String>,\n    #[serde(default)]\n    text: Option<String>,\n    source_descriptor: Option<String>,\n    #[serde(default)]\n    main: Option<bool>,\n    #[serde(default)]\n    multi_lingual: Option<bool>,\n    #[serde(default)]\n    quality_indicator: Option<i64>,\n    parse_status: String,\n}\n''',
    'Rust AudioComponentV1',
)

# Extend JSON schema without changing v1 compatibility: new properties are optional, typed facts.
schema_path = 'arib_si_engine_rs/schema/program_provider_data_v1.schema.json'
schema = read(schema_path)
schema = schema.replace(
    '''        "streamType": {\n          "anyOf": [\n            {\n              "$ref": "#/$defs/uint8"\n            },\n            {\n              "type": "null"\n            }\n          ]\n        },\n        "componentTag": {''',
    '''        "streamType": {\n          "anyOf": [\n            {\n              "$ref": "#/$defs/uint8"\n            },\n            {\n              "type": "null"\n            }\n          ]\n        },\n        "streamContent": {\n          "type": ["integer", "null"],\n          "minimum": 0,\n          "maximum": 15\n        },\n        "componentTag": {''',
    2,
)
# First occurrence in video: insert language/text before resolution.
video_anchor = '''        "resolution": {\n          "type": [\n            "string",\n            "null"\n          ]\n        },'''
if schema.count(video_anchor) != 1:
    raise SystemExit('video schema anchor mismatch')
schema = schema.replace(video_anchor, '''        "language": {\n          "type": ["string", "null"]\n        },\n        "text": {\n          "type": ["string", "null"]\n        },\n''' + video_anchor, 1)
# Audio-specific properties before samplingInfo/sourceDescriptor.
audio_anchor = '''        "samplingInfo": {\n          "type": [\n            "string",\n            "null"\n          ]\n        },\n        "sourceDescriptor": {'''
if schema.count(audio_anchor) != 1:
    raise SystemExit('audio schema anchor mismatch')
schema = schema.replace(audio_anchor, '''        "simulcastGroupTag": {\n          "type": ["integer", "null"],\n          "minimum": 0,\n          "maximum": 255\n        },\n        "samplingRate": {\n          "type": ["integer", "null"],\n          "minimum": 0,\n          "maximum": 7\n        },\n        "samplingInfo": {\n          "type": [\n            "string",\n            "null"\n          ]\n        },\n        "text": {\n          "type": ["string", "null"]\n        },\n        "main": {\n          "type": ["boolean", "null"]\n        },\n        "multiLingual": {\n          "type": ["boolean", "null"]\n        },\n        "qualityIndicator": {\n          "type": ["integer", "null"],\n          "minimum": 0,\n          "maximum": 3\n        },\n        "sourceDescriptor": {''', 1)
write(schema_path, schema)

# --- AOSP TvTrackInfo projection policy. ---
replace_once(
    'tis/src/com/maleicacid/tvinput/tis/TunerSelectionPolicy.kt',
    'import android.media.tv.TvTrackInfo\n',
    'import android.media.MediaFormat\nimport android.media.tv.TvTrackInfo\n',
    'MediaFormat import',
)
policy_append = r'''

/**
 * ARIB audio_component_descriptor facts to Android live-track metadata.
 * PMT stream_type remains the codec authority. Descriptor facts are used only when valid and
 * directly representable by TvTrackInfo; reserved/ambiguous values are left unset.
 */
object AudioTrackMetadataPolicy {
    data class Projection(
        val language: String?,
        val encoding: String?,
        val channelCount: Int?,
        val sampleRateHz: Int?,
        val description: String?,
        val audioDescription: Boolean,
        val hardOfHearing: Boolean,
    )

    fun project(
        pmtStreamType: Int,
        fallbackLanguage: String?,
        component: AribComponentEntry?,
    ): Projection {
        val valid = component?.takeIf { it.parseStatus.equals("OK", ignoreCase = true) }
        val componentType = valid?.componentType
        return Projection(
            language = valid?.language?.takeIf { it.isNotBlank() } ?: fallbackLanguage,
            encoding = encodingForPmtStreamType(pmtStreamType),
            channelCount = componentType?.let(::channelCountForComponentType),
            sampleRateHz = valid?.samplingRate?.let(::sampleRateHz),
            description = valid?.text?.takeIf { it.isNotBlank() },
            audioDescription = componentType?.let(::isAudioDescription) == true,
            hardOfHearing = componentType?.let(::isHardOfHearing) == true,
        )
    }

    fun encodingForPmtStreamType(streamType: Int): String? = when (streamType) {
        0x03, 0x04 -> MediaFormat.MIMETYPE_AUDIO_MPEG
        0x0f -> MediaFormat.MIMETYPE_AUDIO_AAC
        else -> null
    }

    fun channelCountForComponentType(componentType: Int): Int? = when (componentType and 0x1f) {
        0x01 -> 1 // 1/0
        0x02 -> 2 // 1/0 + 1/0 (dual mono; presentation is controlled separately)
        0x03 -> 2 // 2/0
        0x04, 0x05 -> 3 // 2/1, 3/0
        0x06, 0x07 -> 4 // 2/2, 3/1
        0x08 -> 5 // 3/2
        0x09 -> 6 // 3/2 + LFE
        else -> null
    }

    fun sampleRateHz(rawSamplingRate: Int): Int? = when (rawSamplingRate) {
        0x01 -> 16_000
        0x02 -> 22_050
        0x03 -> 24_000
        0x05 -> 32_000
        0x06 -> 44_100
        0x07 -> 48_000
        else -> null
    }

    fun isAudioDescription(componentType: Int): Boolean = ((componentType ushr 5) and 0x03) == 0x01
    fun isHardOfHearing(componentType: Int): Boolean = ((componentType ushr 5) and 0x03) == 0x02
}
'''
path = 'tis/src/com/maleicacid/tvinput/tis/TunerSelectionPolicy.kt'
text = read(path)
if 'object AudioTrackMetadataPolicy' in text:
    raise SystemExit('AudioTrackMetadataPolicy already exists')
write(path, text.rstrip() + policy_append + '\n')

# Correlate current EIT descriptor by component_tag and project into TvTrackInfo.
path = 'tis/src/com/maleicacid/tvinput/tis/MaleicacidLiveSession.kt'
replace_once(path,
    '''        val selection = initialSelection.copy(\n            audioComponentType = currentAudioComponentType(service.serviceKey, initialSelection.audio),\n        )\n''',
    '''        val selection = initialSelection.copy(\n            audioComponentType = currentAudioComponent(service.serviceKey, initialSelection.audio?.componentTag)?.componentType\n                ?: initialSelection.audio?.componentType,\n        )\n''',
    'playback current audio component')
replace_once(path,
    '''                val selection = initialSelection.copy(\n                    audioComponentType = currentAudioComponentType(service.serviceKey, initialSelection.audio),\n                )\n''',
    '''                val selection = initialSelection.copy(\n                    audioComponentType = currentAudioComponent(service.serviceKey, initialSelection.audio?.componentTag)?.componentType\n                        ?: initialSelection.audio?.componentType,\n                )\n''',
    'audio switch current component')
old_helper = '''    private fun currentAudioComponentType(\n        serviceKey: ServiceKey,\n        audio: com.maleicacid.tvinput.aribsi.AribElementaryStream?,\n        nowMillis: Long = System.currentTimeMillis(),\n    ): Int? {\n        audio ?: return null\n        val componentTag = audio.componentTag ?: return audio.componentType\n        val currentEvent = aribSiEngine.programStateSnapshot().events\n            .asSequence()\n            .filter { event -> event.serviceKey == serviceKey && event.durationMillis > 0L }\n            .filter { event -> nowMillis >= event.startTimeMillis && nowMillis < event.startTimeMillis + event.durationMillis }\n            .minByOrNull { it.startTimeMillis }\n            ?: return audio.componentType\n        return currentEvent.descriptors.components.audio\n            .firstOrNull { component -> component.parseStatus == "OK" && component.componentTag == componentTag }\n            ?.componentType\n            ?: audio.componentType\n    }\n'''
new_helper = '''    private fun currentAudioComponent(\n        serviceKey: ServiceKey,\n        componentTag: Int?,\n        nowMillis: Long = System.currentTimeMillis(),\n    ): com.maleicacid.tvinput.aribsi.AribComponentEntry? {\n        componentTag ?: return null\n        val currentEvent = aribSiEngine.programStateSnapshot().events\n            .asSequence()\n            .filter { event -> event.serviceKey == serviceKey && event.durationMillis > 0L }\n            .filter { event -> nowMillis >= event.startTimeMillis && nowMillis < event.startTimeMillis + event.durationMillis }\n            .minByOrNull { it.startTimeMillis }\n            ?: return null\n        return currentEvent.descriptors.components.audio\n            .firstOrNull { component -> component.parseStatus.equals("OK", ignoreCase = true) && component.componentTag == componentTag }\n    }\n'''
replace_once(path, old_helper, new_helper, 'current audio component helper')
old_update = '''        val signature = tracks.map { track ->\n            val audioComponentType = if (track.type == TvTrackInfo.TYPE_AUDIO) track.componentType ?: -1 else -1\n            val videoComponentType = if (track.type == TvTrackInfo.TYPE_VIDEO) track.componentType ?: -1 else -1\n            val subtitleDataComponentId = if (track.type == TvTrackInfo.TYPE_SUBTITLE) track.dataComponentId ?: -1 else -1\n            listOf(\n                track.id,\n                track.type.toString(),\n                track.pid.toString(),\n                track.streamType.toString(),\n                track.componentTag?.toString() ?: "-1",\n                track.language.orEmpty(),\n                audioComponentType.toString(),\n                videoComponentType.toString(),\n                subtitleDataComponentId.toString(),\n            ).joinToString("|")\n        }.toSet()\n        if (signature != currentTrackSignature) {\n            currentTrackSignature = signature\n            notifyTracksChanged(tracks.map { track ->\n                val builder = TvTrackInfo.Builder(track.type, track.id)\n                LanguageCodeNormalizer.normalizeForTvTrackLanguage(track.language)?.let { language ->\n                    builder.setLanguage(language)\n                }\n                builder.build()\n            })\n        }\n'''
new_update = '''        val audioMetadataByTrackId = tracks\n            .filter { it.type == TvTrackInfo.TYPE_AUDIO }\n            .associate { track ->\n                val component = currentAudioComponent(service.serviceKey, track.componentTag)\n                track.id to AudioTrackMetadataPolicy.project(track.streamType, track.language, component)\n            }\n        val signature = tracks.map { track ->\n            val audioMetadata = audioMetadataByTrackId[track.id]\n            val videoComponentType = if (track.type == TvTrackInfo.TYPE_VIDEO) track.componentType ?: -1 else -1\n            val subtitleDataComponentId = if (track.type == TvTrackInfo.TYPE_SUBTITLE) track.dataComponentId ?: -1 else -1\n            listOf(\n                track.id,\n                track.type.toString(),\n                track.pid.toString(),\n                track.streamType.toString(),\n                track.componentTag?.toString() ?: "-1",\n                audioMetadata?.language.orEmpty(),\n                audioMetadata?.encoding.orEmpty(),\n                audioMetadata?.channelCount?.toString() ?: "-1",\n                audioMetadata?.sampleRateHz?.toString() ?: "-1",\n                audioMetadata?.description.orEmpty(),\n                (audioMetadata?.audioDescription == true).toString(),\n                (audioMetadata?.hardOfHearing == true).toString(),\n                videoComponentType.toString(),\n                subtitleDataComponentId.toString(),\n            ).joinToString("|")\n        }.toSet()\n        if (signature != currentTrackSignature) {\n            currentTrackSignature = signature\n            notifyTracksChanged(tracks.map { track ->\n                val builder = TvTrackInfo.Builder(track.type, track.id)\n                val audioMetadata = audioMetadataByTrackId[track.id]\n                val language = audioMetadata?.language ?: track.language\n                LanguageCodeNormalizer.normalizeForTvTrackLanguage(language)?.let(builder::setLanguage)\n                if (track.type == TvTrackInfo.TYPE_AUDIO && audioMetadata != null) {\n                    audioMetadata.encoding?.let(builder::setEncoding)\n                    audioMetadata.channelCount?.let(builder::setAudioChannelCount)\n                    audioMetadata.sampleRateHz?.let(builder::setAudioSampleRate)\n                    audioMetadata.description?.let(builder::setDescription)\n                    if (audioMetadata.audioDescription) builder.setAudioDescription(true)\n                    if (audioMetadata.hardOfHearing) builder.setHardOfHearing(true)\n                }\n                builder.build()\n            })\n        }\n'''
replace_once(path, old_update, new_update, 'TvTrackInfo detailed projection')

# TIS design: keep live-track policy here, not in the TvProvider projection document.
path = 'tis/DESIGN_JA.md'
text = read(path)
anchor = '''`TvTrackInfo` の `trackId` はAndroid/TIS runtimeの識別子であり、TISがcurrent serviceのcomponent identityからcurrent session内で一意になるよう決定する。ARIB意味objectや永続`internal_provider_data`に`trackId`を保存せず、Rust SI parserへ返さない。\n'''
addition = anchor + '''\nAudio track metadata はPMTとEITの責務を混同しない。実際にfilter/decoderへ渡すPIDと`stream_type`はcurrent PMT ESを正とし、current EIT `audio_component_descriptor`は同一`component_tag`のESに対する放送意味metadataとして相関する。ARIB STD-B10の`component_tag`はPMTのstream identifier descriptorの同fieldと同値であるため、この相関以外のPID順序やtrack順序による推測を行わない。descriptorが欠落・不正・`component_tag`不一致の場合はPMTで確定できるtrack identity/codec情報だけを通知し、EIT metadataを捏造しない。\n\n有効な`audio_component_descriptor`からAndroid `TvTrackInfo`へ自然対応する値だけを投影する。ISO 639 languageは`setLanguage()`、PMT `stream_type`から一意に決まるMIMEは`setEncoding()`、ARIB `sampling_rate`の明示値は`setAudioSampleRate()`、`component_type`下位5bitのaudio modeから一意に決まるchannel数は`setAudioChannelCount()`、`text_char`は`setDescription()`へ投影する。`component_type` b6-b5=`01`の視覚障害者向け音声解説は`setAudioDescription(true)`、`10`の聴覚障害者向け音声は`setHardOfHearing(true)`へ投影する。reserved/未定義audio mode、reserved sampling rate、PMTとdescriptorで意味が一致しない値を推測で標準fieldへ埋めない。`main_component_flag`、`ES_multi_lingual_flag`、`quality_indicator`、`simulcast_group_tag`等は放送由来factとして保持するが、意味の異なるAndroid標準fieldへ転用しない。dual monoの主/副/主副presentationは1 ESのpresentation stateとして`AudioTrack.setDualMonoMode()`へ接続し、別audio trackを捏造しない。\n'''
if text.count(anchor) != 1:
    raise SystemExit('TIS design track anchor mismatch')
write(path, text.replace(anchor, addition, 1))

# Projection document scope remains unchanged. Only clarify its existing TvProvider/provider-data contract.
path = 'ARIB_SI_EPG_TvProvider投影方針.md'
text = read(path)
old = '| audio language | `Programs.COLUMN_AUDIO_LANGUAGE` | 音声コンポーネント構造を保持 | Android標準列がある |'
new = '| audio language | 有効な `audio_component_descriptor` の primary / second ISO 639 language を Android が受理する ISO 639-1 または ISO 639-2/T 表現へ正規化し、重複を除いた comma-separated 値として `Programs.COLUMN_AUDIO_LANGUAGE` へ格納する。候補がなければ `NULL` とする。 | primary / second languageを含む音声コンポーネント構造を保持 | Android標準列が複数言語をcomma-separated形式で保持するため |'
if old not in text:
    raise SystemExit('projection audio language row mismatch')
text = text.replace(old, new, 1)
old = '`internal_provider_data` には長形式イベント項目リスト、component/audio/series/linkage/event_group/free_CA_mode等の完全構造と、unknownを含むdescriptor診断、元のARIBレーティング値、CAS意味事実を保存する。'
if old in text:
    new = '`internal_provider_data` には長形式イベント項目リスト、component/audio/series/event_group/free_CA_mode等の放送意味構造、linkageの型付き識別fieldと保存上限内のprivate-data診断、unknownを含むdescriptor診断、元のARIBレーティング値、CAS意味事実を保存する。特にcomponent/audioは、ARIB descriptorが意味を個別に定義する`stream_content`、`component_type`、`component_tag`、language/text、audioの`stream_type`、`simulcast_group_tag`、`ES_multi_lingual_flag`、`main_component_flag`、`quality_indicator`、`sampling_rate`を、取得できた範囲で別fieldとして保持し、Android標準列への投影結果へ潰さない。'
    text = text.replace(old, new, 1)
# Current document sometimes describes service name/type as duplicated internally; align with standard-column SSOT without changing scope.
text = text.replace(
    '| サービス名 | `Channels.COLUMN_DISPLAY_NAME` | サービス構造を保持 | Android標準列がある |',
    '| サービス名 | `Channels.COLUMN_DISPLAY_NAME` | channel identity/tune情報とは別に同一表示名を重複保存しない | Android標準列が表示名の正本であり、private dataへ同一値を二重化しないため |',
)
text = text.replace(
    '| service_type | `Channels.COLUMN_SERVICE_TYPE` | raw service_typeを保持 | Android標準列がある |',
    '| service_type | ARIB raw 8-bit codingを10進文字列として `Channels.COLUMN_SERVICE_TYPE` へ格納する | 同一raw値をprivate dataへ重複保存しない | underlying broadcast standardのcodingを保持するAndroid標準列自体を正本とするため |',
)
write(path, text)

# Rust design notes: schema is the canonical preservation boundary; no TvTrackInfo scope here.
path = 'arib_si_engine_rs/DESIGN_JA.md'
text = read(path)
needle = 'Android canonical genre の写像結果、Android rating文字列、runtime選択track、decoder/CAS capability結果はprovider-dataへ保存しない。'
if needle in text and 'simulcast_group_tag' not in text[text.find(needle)-1200:text.find(needle)+1200]:
    text = text.replace(needle, needle + '\n\n`components.audio[]` は `audio_component_descriptor` の独立意味fieldをAndroid runtime metadataへ潰さず保持する。取得できた `stream_content / component_type / component_tag / stream_type / simulcast_group_tag / ES_multi_lingual_flag / main_component_flag / quality_indicator / sampling_rate / ISO_639_language_code(_2) / text_char` と、明示的に導出できる channel configuration / sampling表示をprovider-dataの型付きfieldへ保存する。`components.video[]` も `component_descriptor` の `stream_content / component_type / component_tag / ISO_639_language_code / text_char` を保持する。runtime `TvTrackInfo`投影結果は保存しない。', 1)
write(path, text)

# Extend existing Kotlin tests rather than changing suite topology.
path = 'tis/tests/src/com/maleicacid/tvinput/tis/TisR51FixedPlanAcceptanceTest.kt'
text = read(path)
anchor = '''        check(!providerAudio.has("liveViewableClaim"))\n        check(TunerSelectionPolicy.selectVideo(service.streams) == null)\n'''
if anchor not in text:
    raise SystemExit('metadata test anchor mismatch')
replacement = '''        check(!providerAudio.has("liveViewableClaim"))\n        check(AudioTrackMetadataPolicy.encodingForPmtStreamType(0x0f) == android.media.MediaFormat.MIMETYPE_AUDIO_AAC)\n        check(AudioTrackMetadataPolicy.channelCountForComponentType(0x02) == 2)\n        check(AudioTrackMetadataPolicy.channelCountForComponentType(0x29) == 6)\n        check(AudioTrackMetadataPolicy.sampleRateHz(0x07) == 48_000)\n        check(AudioTrackMetadataPolicy.sampleRateHz(0x04) == null)\n        check(AudioTrackMetadataPolicy.isAudioDescription(0x20))\n        check(AudioTrackMetadataPolicy.isHardOfHearing(0x40))\n        check(TunerSelectionPolicy.selectVideo(service.streams) == null)\n'''
text = text.replace(anchor, replacement, 1)
write(path, text)

path = 'tis/tests/src/com/maleicacid/tvinput/tis/TvProviderWriterProgramsTest.kt'
text = read(path)
anchor = '''        check(providerData.utf8Contains("secondLanguage"))\n        check(providerData.utf8Contains("genres"))\n'''
if anchor not in text:
    raise SystemExit('provider-data audio test anchor mismatch')
replacement = '''        check(providerData.utf8Contains("secondLanguage"))\n        check(providerData.utf8Contains("streamContent"))\n        check(providerData.utf8Contains("simulcastGroupTag"))\n        check(providerData.utf8Contains("samplingRate"))\n        check(providerData.utf8Contains("qualityIndicator"))\n        check(providerData.utf8Contains("multiLingual"))\n        check(providerData.utf8Contains("genres"))\n'''
text = text.replace(anchor, replacement, 1)
write(path, text)

# Avoid accidental live-runtime scope expansion in projection MD.
projection = read('ARIB_SI_EPG_TvProvider投影方針.md')
for forbidden in ['TvTrackInfo.Builder', 'setAudioSampleRate(', 'setHardOfHearing(', 'Tuner.scan(']:
    if forbidden in projection:
        raise SystemExit(f'projection MD scope expansion detected: {forbidden}')

print('applied PR54 #31 and provider-data projection consistency changes')
