from pathlib import Path
import json

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

# -----------------------------------------------------------------------------
# ARIB short/extended event multilingual facts: preserve per-language candidates.
# -----------------------------------------------------------------------------
path = 'arib_si_engine_rs/src/descriptors.rs'
replace_once(path,
'''#[derive(Clone, Debug, Default, Eq, PartialEq)]\npub struct ExtendedEventItem {\n    pub language_code: String,\n    pub item_description: String,\n    pub item_text: String,\n}\n\n#[derive(Clone, Debug, Default, Eq, PartialEq)]\npub struct EventDescriptors {\n    pub title: String,\n    pub description: String,\n    pub extended_description: String,\n''',
'''#[derive(Clone, Debug, Default, Eq, PartialEq)]\npub struct ExtendedEventItem {\n    pub language_code: String,\n    pub item_description: String,\n    pub item_text: String,\n}\n\n#[derive(Clone, Debug, Default, Eq, PartialEq)]\npub struct ShortEventText {\n    pub language_code: String,\n    pub title: String,\n    pub text: String,\n}\n\n#[derive(Clone, Debug, Default, Eq, PartialEq)]\npub struct ExtendedEventText {\n    pub language_code: String,\n    pub text: String,\n}\n\n#[derive(Clone, Debug, Default, Eq, PartialEq)]\npub struct EventDescriptors {\n    pub title: String,\n    pub description: String,\n    pub extended_description: String,\n    pub short_events: Vec<ShortEventText>,\n    pub extended_texts: Vec<ExtendedEventText>,\n''', 'multilingual descriptor models')

old_short_tail = '''    let title = decode_descriptor_text_lossy(\n        &body[name_start..name_end],\n        out,\n        tag,\n        offset,\n        declared_length,\n        body,\n        (\n            "eventName",\n            offset.saturating_add(2).saturating_add(name_start),\n        ),\n    )\n    .trim()\n    .to_string();\n    if out.title.is_empty() {\n        out.title = title;\n    }\n    let text = decode_descriptor_text_lossy(\n        &body[text_start..text_end],\n        out,\n        tag,\n        offset,\n        declared_length,\n        body,\n        ("text", offset.saturating_add(2).saturating_add(text_start)),\n    )\n    .trim()\n    .to_string();\n    if !text.is_empty() {\n        out.description = join_description(&out.description, &text);\n    }\n}\n'''
new_short_tail = '''    let language_code = language(&body[0..3]);\n    if language_code.len() != 3 || !language_code.bytes().all(|byte| byte.is_ascii_alphabetic()) {\n        out.diagnostics.push(descriptor_diagnostic(\n            DescriptorParseStatus::UnsupportedValue,\n            tag,\n            offset,\n            declared_length,\n            body.len(),\n            body,\n            "short_event_descriptor ISO_639_language_code is invalid",\n        ));\n        return;\n    }\n    if out.short_events.iter().any(|candidate| candidate.language_code == language_code) {\n        out.diagnostics.push(descriptor_diagnostic(\n            DescriptorParseStatus::InvalidSequence,\n            tag,\n            offset,\n            declared_length,\n            body.len(),\n            body,\n            &format!("short_event_descriptor is repeated for the same language={language_code}"),\n        ));\n        return;\n    }\n    let title = decode_descriptor_text_lossy(\n        &body[name_start..name_end],\n        out,\n        tag,\n        offset,\n        declared_length,\n        body,\n        (\n            "eventName",\n            offset.saturating_add(2).saturating_add(name_start),\n        ),\n    )\n    .trim()\n    .to_string();\n    let text = decode_descriptor_text_lossy(\n        &body[text_start..text_end],\n        out,\n        tag,\n        offset,\n        declared_length,\n        body,\n        ("text", offset.saturating_add(2).saturating_add(text_start)),\n    )\n    .trim()\n    .to_string();\n    out.short_events.push(ShortEventText {\n        language_code,\n        title: title.clone(),\n        text: text.clone(),\n    });\n    if out.short_events.len() == 1 {\n        out.title = title;\n        out.description = text;\n    }\n}\n'''
replace_once(path, old_short_tail, new_short_tail, 'short event language preservation')

replace_once(path,
'''    let mut by_language = std::collections::BTreeMap::<String, Vec<ExtendedEventFragment>>::new();\n    for fragment in fragments {\n        by_language\n            .entry(fragment.language_code.clone())\n            .or_default()\n            .push(fragment);\n    }\n    for (language_code, mut language_fragments) in by_language {\n''',
'''    let mut by_language = std::collections::BTreeMap::<String, Vec<ExtendedEventFragment>>::new();\n    let mut language_order = Vec::<String>::new();\n    for fragment in fragments {\n        if !by_language.contains_key(&fragment.language_code) {\n            language_order.push(fragment.language_code.clone());\n        }\n        by_language\n            .entry(fragment.language_code.clone())\n            .or_default()\n            .push(fragment);\n    }\n    for language_code in language_order {\n        let mut language_fragments = by_language.remove(&language_code).unwrap_or_default();\n''', 'extended language descriptor order')

replace_once(path,
'''        if !text.is_empty() {\n            out.extended_description = join_description(&out.extended_description, &text);\n        }\n    }\n}\n''',
'''        if !text.is_empty() {\n            out.extended_texts.push(ExtendedEventText { language_code, text });\n        }\n    }\n    out.extended_description = if let Some(language_code) = out.short_events.first().map(|candidate| candidate.language_code.as_str()) {\n        out.extended_texts\n            .iter()\n            .find(|candidate| candidate.language_code == language_code)\n            .map(|candidate| candidate.text.clone())\n            .unwrap_or_default()\n    } else {\n        out.extended_texts\n            .iter()\n            .find(|candidate| candidate.language_code == "jpn")\n            .or_else(|| out.extended_texts.first())\n            .map(|candidate| candidate.text.clone())\n            .unwrap_or_default()\n    };\n}\n''', 'extended language selection')

# join_description is no longer allowed to merge different language descriptors.
text = read(path)
text = text.replace('''fn join_description(current: &str, next: &str) -> String {\n    if current.is_empty() {\n        next.to_string()\n    } else {\n        format!("{}\\n{}", current, next)\n    }\n}\n''', '')
write(path, text)

# Extend existing Rust tests without adding test-count-sensitive Kotlin classes.
replace_once(path,
'''        let parsed = parse_event_descriptors(&bytes);\n        assert_eq!(parsed.extended_description, "EN\\nJP");\n        assert!(!parsed\n            .diagnostics\n            .iter()\n            .any(|diagnostic| diagnostic.parse_status == DescriptorParseStatus::InvalidSequence));\n''',
'''        let parsed = parse_event_descriptors(&bytes);\n        assert_eq!(parsed.extended_description, "JP");\n        assert_eq!(parsed.extended_texts.len(), 2);\n        assert_eq!(parsed.extended_texts[0].language_code, "jpn");\n        assert_eq!(parsed.extended_texts[0].text, "JP");\n        assert_eq!(parsed.extended_texts[1].language_code, "eng");\n        assert_eq!(parsed.extended_texts[1].text, "EN");\n        assert!(!parsed\n            .diagnostics\n            .iter()\n            .any(|diagnostic| diagnostic.parse_status == DescriptorParseStatus::InvalidSequence));\n''', 'extended language test')
replace_once(path,
'''        assert_eq!(parsed.title, "AB");\n        assert_eq!(parsed.description, "CD");\n    }\n''',
'''        assert_eq!(parsed.title, "AB");\n        assert_eq!(parsed.description, "CD");\n        assert_eq!(parsed.short_events.len(), 1);\n        assert_eq!(parsed.short_events[0].language_code, "jpn");\n\n        let mut english = b"eng".to_vec();\n        let english_title = arib_alnum(b"EF");\n        let english_text = arib_alnum(b"GH");\n        english.push(english_title.len() as u8);\n        english.extend_from_slice(&english_title);\n        english.push(english_text.len() as u8);\n        english.extend_from_slice(&english_text);\n        let mut multilingual = descriptor(0x4d, &body);\n        multilingual.extend_from_slice(&descriptor(0x4d, &english));\n        let parsed = parse_event_descriptors(&multilingual);\n        assert_eq!(parsed.title, "AB");\n        assert_eq!(parsed.description, "CD");\n        assert_eq!(parsed.short_events.len(), 2);\n        assert_eq!(parsed.short_events[1].language_code, "eng");\n        assert_eq!(parsed.short_events[1].title, "EF");\n        assert_eq!(parsed.short_events[1].text, "GH");\n\n        let mut duplicate = descriptor(0x4d, &body);\n        duplicate.extend_from_slice(&descriptor(0x4d, &body));\n        let parsed = parse_event_descriptors(&duplicate);\n        assert_eq!(parsed.short_events.len(), 1);\n        assert!(parsed.diagnostics.iter().any(|diagnostic|\n            diagnostic.parse_status == DescriptorParseStatus::InvalidSequence\n                && diagnostic.message.contains("language=jpn")));\n    }\n''', 'short language test')

# -----------------------------------------------------------------------------
# Rust event DTO exposes the candidates, while selected legacy fields stay single-language.
# -----------------------------------------------------------------------------
path = 'arib_si_engine_rs/src/lib.rs'
replace_once(path,
'''fn extended_items_value(event: &EitEvent) -> serde_json::Value {\n''',
'''fn short_events_value(event: &EitEvent) -> serde_json::Value {\n    serde_json::Value::Array(\n        event.descriptors.short_events.iter().map(|candidate| {\n            serde_json::json!({\n                "languageCode": candidate.language_code,\n                "title": candidate.title,\n                "text": candidate.text,\n                "parseStatus": "OK",\n            })\n        }).collect(),\n    )\n}\n\nfn extended_texts_value(event: &EitEvent) -> serde_json::Value {\n    serde_json::Value::Array(\n        event.descriptors.extended_texts.iter().map(|candidate| {\n            serde_json::json!({\n                "languageCode": candidate.language_code,\n                "text": candidate.text,\n                "parseStatus": "OK",\n            })\n        }).collect(),\n    )\n}\n\nfn extended_items_value(event: &EitEvent) -> serde_json::Value {\n''', 'event multilingual helpers')
replace_once(path,
'''        "descriptors": {\n            "extendedItems": extended_items_value(event),\n''',
'''        "descriptors": {\n            "shortEvents": short_events_value(event),\n            "extendedTexts": extended_texts_value(event),\n            "extendedItems": extended_items_value(event),\n''', 'event DTO candidate arrays')

# -----------------------------------------------------------------------------
# Kotlin SI and Program models keep candidates; linkage names its bounded prefix honestly.
# -----------------------------------------------------------------------------
path = 'tis/src/com/maleicacid/tvinput/aribsi/SiModels.kt'
replace_once(path,
'''data class AribExtendedItem(\n    val languageCode: String,\n    val itemDescription: String,\n    val itemText: String,\n)\n''',
'''data class AribExtendedItem(\n    val languageCode: String,\n    val itemDescription: String,\n    val itemText: String,\n)\n\ndata class AribShortEventText(\n    val languageCode: String,\n    val title: String,\n    val text: String,\n    val parseStatus: String = "OK",\n)\n\ndata class AribExtendedEventText(\n    val languageCode: String,\n    val text: String,\n    val parseStatus: String = "OK",\n)\n''', 'Kotlin multilingual models')
replace_once(path,
'''data class AribLinkage(\n    val linkageType: Int,\n    val serviceKey: ServiceKey,\n    val privateDataHex: String = "",\n''',
'''data class AribLinkage(\n    val linkageType: Int,\n    val serviceKey: ServiceKey,\n    val privateDataPrefixHex: String = "",\n''', 'linkage prefix model name')
replace_once(path,
'''data class AribEventDescriptors(\n    val extendedItems: List<AribExtendedItem> = emptyList(),\n''',
'''data class AribEventDescriptors(\n    val shortEvents: List<AribShortEventText> = emptyList(),\n    val extendedTexts: List<AribExtendedEventText> = emptyList(),\n    val extendedItems: List<AribExtendedItem> = emptyList(),\n''', 'event descriptor candidates')

path = 'tis/src/com/maleicacid/tvinput/db/TvInputModels.kt'
replace_once(path,
'''import com.maleicacid.tvinput.aribsi.AribSeries\n''',
'''import com.maleicacid.tvinput.aribsi.AribSeries\nimport com.maleicacid.tvinput.aribsi.AribShortEventText\nimport com.maleicacid.tvinput.aribsi.AribExtendedEventText\n''', 'Program model imports')
replace_once(path,
'''data class ProgramDescriptors(\n    val extendedItems: List<com.maleicacid.tvinput.aribsi.AribExtendedItem> = emptyList(),\n''',
'''data class ProgramDescriptors(\n    val shortEvents: List<AribShortEventText> = emptyList(),\n    val extendedTexts: List<AribExtendedEventText> = emptyList(),\n    val extendedItems: List<com.maleicacid.tvinput.aribsi.AribExtendedItem> = emptyList(),\n''', 'Program descriptor candidates')

# -----------------------------------------------------------------------------
# Native DTO parsing selects one descriptor language and never mixes languages.
# -----------------------------------------------------------------------------
path = 'tis/src/com/maleicacid/tvinput/aribsi/NativeAribSiParser.kt'
replace_once(path,
'''        val series = descriptorsObj.optJSONObject("series")\n        val descriptorDiagnosticsCanonicalJson = diagnostics.optString("descriptorDiagnosticsCanonicalJson", "[]")\n        if (eventId < 0) return@mapNotNull null\n        AribEvent(\n''',
'''        val series = descriptorsObj.optJSONObject("series")\n        val shortEvents = parseShortEvents(descriptorsObj.optJSONArray("shortEvents"))\n        val extendedTexts = parseExtendedTexts(descriptorsObj.optJSONArray("extendedTexts"))\n        val extendedItems = parseExtendedItems(descriptorsObj.optJSONArray("extendedItems"))\n        val selectedLanguage = shortEvents.firstOrNull()?.languageCode\n            ?: extendedTexts.firstOrNull()?.languageCode\n            ?: extendedItems.firstOrNull()?.languageCode\n        val selectedShort = selectedLanguage?.let { language -> shortEvents.firstOrNull { it.languageCode == language } }\n        val selectedExtended = selectedLanguage?.let { language -> extendedTexts.firstOrNull { it.languageCode == language } }\n        val descriptorDiagnosticsCanonicalJson = diagnostics.optString("descriptorDiagnosticsCanonicalJson", "[]")\n        if (eventId < 0) return@mapNotNull null\n        AribEvent(\n''', 'parse candidate arrays')
replace_once(path,
'''            title = obj.optString("title"),\n            description = obj.optString("description"),\n            extendedDescription = obj.optString("extendedDescription"),\n''',
'''            title = if (shortEvents.isNotEmpty()) selectedShort?.title.orEmpty() else obj.optString("title"),\n            description = if (shortEvents.isNotEmpty()) selectedShort?.text.orEmpty() else obj.optString("description"),\n            extendedDescription = if (extendedTexts.isNotEmpty()) selectedExtended?.text.orEmpty() else obj.optString("extendedDescription"),\n''', 'selected event strings')
replace_once(path,
'''            descriptors = AribEventDescriptors(\n                extendedItems = parseExtendedItems(descriptorsObj.optJSONArray("extendedItems")),\n''',
'''            descriptors = AribEventDescriptors(\n                shortEvents = shortEvents,\n                extendedTexts = extendedTexts,\n                extendedItems = extendedItems,\n''', 'store candidate arrays')
replace_once(path,
'''            privateDataHex = obj.optString("privateDataPrefixHex", ""),\n''',
'''            privateDataPrefixHex = obj.optString("privateDataPrefixHex", ""),\n''', 'parse linkage prefix')
replace_once(path,
'''    private fun parseExtendedItems(array: JSONArray?): List<AribExtendedItem> = (0 until (array?.length() ?: 0)).mapNotNull { index ->\n''',
'''    private fun parseShortEvents(array: JSONArray?): List<AribShortEventText> = (0 until (array?.length() ?: 0)).mapNotNull { index ->\n        val obj = array!!.optJSONObject(index) ?: return@mapNotNull null\n        val languageCode = obj.optString("languageCode")\n        if (languageCode.length != 3) null else AribShortEventText(\n            languageCode = languageCode,\n            title = obj.optString("title"),\n            text = obj.optString("text"),\n            parseStatus = obj.optString("parseStatus", "OK"),\n        )\n    }.filter { it.parseStatus.equals("OK", ignoreCase = true) }.distinctBy { it.languageCode }\n\n    private fun parseExtendedTexts(array: JSONArray?): List<AribExtendedEventText> = (0 until (array?.length() ?: 0)).mapNotNull { index ->\n        val obj = array!!.optJSONObject(index) ?: return@mapNotNull null\n        val languageCode = obj.optString("languageCode")\n        if (languageCode.length != 3) null else AribExtendedEventText(\n            languageCode = languageCode,\n            text = obj.optString("text"),\n            parseStatus = obj.optString("parseStatus", "OK"),\n        )\n    }.filter { it.parseStatus.equals("OK", ignoreCase = true) }.distinctBy { it.languageCode }\n\n    private fun parseExtendedItems(array: JSONArray?): List<AribExtendedItem> = (0 until (array?.length() ?: 0)).mapNotNull { index ->\n''', 'parse candidate helpers')

# -----------------------------------------------------------------------------
# TvProvider projection: no fake title, and long text uses the selected language only.
# -----------------------------------------------------------------------------
path = 'tis/src/com/maleicacid/tvinput/aribsi/EventModelMapper.kt'
replace_once(path,
'''                title = event.title.ifBlank { "event-${event.eventId}" },\n''',
'''                title = event.title,\n''', 'remove synthetic title')
replace_once(path,
'''                descriptors = ProgramDescriptors(\n                    extendedItems = event.descriptors.extendedItems,\n''',
'''                descriptors = ProgramDescriptors(\n                    shortEvents = event.descriptors.shortEvents,\n                    extendedTexts = event.descriptors.extendedTexts,\n                    extendedItems = event.descriptors.extendedItems,\n''', 'map candidate arrays')
replace_once(path,
'''        val d = event.descriptors\n        val extended = d.extendedItems.joinToString("\\n") { item ->\n            if (item.itemDescription.isBlank()) item.itemText else "【${item.itemDescription}】${item.itemText}"\n        }\n''',
'''        val d = event.descriptors\n        val selectedLanguage = d.shortEvents.firstOrNull()?.languageCode\n            ?: d.extendedTexts.firstOrNull()?.languageCode\n            ?: d.extendedItems.firstOrNull()?.languageCode\n        val extended = d.extendedItems\n            .asSequence()\n            .filter { item -> selectedLanguage == null || item.languageCode == selectedLanguage }\n            .joinToString("\\n") { item ->\n                if (item.itemDescription.isBlank()) item.itemText else "【${item.itemDescription}】${item.itemText}"\n            }\n''', 'filter extended items by selected language')

path = 'tis/src/com/maleicacid/tvinput/tis/TvProviderWriter.kt'
replace_once(path,
'''        program.title.isBlank() -> Diagnostic(program.serviceKey, "program-validate", "title が空です")\n''', '', 'allow blank title')
replace_once(path,
'''        put(TvContract.Programs.COLUMN_TITLE, program.title)\n''',
'''        if (program.title.isBlank()) putNull(TvContract.Programs.COLUMN_TITLE) else put(TvContract.Programs.COLUMN_TITLE, program.title)\n''', 'nullable title projection')

# -----------------------------------------------------------------------------
# Provider-data: preserve language-tagged candidates; do not duplicate selected strings.
# -----------------------------------------------------------------------------
path = 'tis/src/com/maleicacid/tvinput/aribsi/ProviderDataBridge.kt'
replace_once(path,
'''            .put("freeCaMode", toFreeCaModeObject(descriptors))\n            .put("series", toSeriesObject(descriptors))\n            .put("diagnostics", JSONObject()\n''',
'''            .put("freeCaMode", toFreeCaModeObject(descriptors))\n            .put("series", toSeriesObject(descriptors))\n            .put("shortEvents", toShortEventsArray(descriptors.shortEvents))\n            .put("extendedTexts", toExtendedTextsArray(descriptors.extendedTexts))\n            .put("diagnostics", JSONObject()\n''', 'provider request candidates')
replace_once(path,
'''                .put("privateDataPrefixHex", item.privateDataHex)\n''',
'''                .put("privateDataPrefixHex", item.privateDataPrefixHex)\n''', 'provider linkage prefix')
replace_once(path,
'''    private fun toExtendedItemsArray(items: List<AribExtendedItem>): JSONArray = JSONArray().apply {\n''',
'''    private fun toShortEventsArray(items: List<AribShortEventText>): JSONArray = JSONArray().apply {\n        items.forEach { item ->\n            put(JSONObject()\n                .put("languageCode", item.languageCode)\n                .put("title", item.title)\n                .put("text", item.text)\n                .put("parseStatus", item.parseStatus))\n        }\n    }\n\n    private fun toExtendedTextsArray(items: List<AribExtendedEventText>): JSONArray = JSONArray().apply {\n        items.forEach { item ->\n            put(JSONObject()\n                .put("languageCode", item.languageCode)\n                .put("text", item.text)\n                .put("parseStatus", item.parseStatus))\n        }\n    }\n\n    private fun toExtendedItemsArray(items: List<AribExtendedItem>): JSONArray = JSONArray().apply {\n''', 'provider candidate helpers')

# -----------------------------------------------------------------------------
# Rust provider-data v1: optional new arrays preserve old v1 fixtures/canonical bytes.
# -----------------------------------------------------------------------------
path = 'arib_si_engine_rs/src/provider_data.rs'
replace_once(path,
'''struct ExtendedItemV1 {\n    language_code: String,\n    description: String,\n    text: String,\n    parse_status: String,\n}\n''',
'''struct ShortEventV1 {\n    language_code: String,\n    title: String,\n    text: String,\n    parse_status: String,\n}\n\n#[derive(Clone, Debug, Serialize, Deserialize)]\n#[serde(rename_all = "camelCase", deny_unknown_fields)]\nstruct ExtendedTextV1 {\n    language_code: String,\n    text: String,\n    parse_status: String,\n}\n\n#[derive(Clone, Debug, Serialize, Deserialize)]\n#[serde(rename_all = "camelCase", deny_unknown_fields)]\nstruct ExtendedItemV1 {\n    language_code: String,\n    description: String,\n    text: String,\n    parse_status: String,\n}\n''', 'provider candidate structs')
replace_once(path,
'''    free_ca_mode: Option<FreeCaModeV1>,\n    extended_items: Vec<ExtendedItemV1>,\n    components: ComponentsV1,\n''',
'''    free_ca_mode: Option<FreeCaModeV1>,\n    #[serde(default, skip_serializing_if = "Vec::is_empty")]\n    short_events: Vec<ShortEventV1>,\n    #[serde(default, skip_serializing_if = "Vec::is_empty")]\n    extended_texts: Vec<ExtendedTextV1>,\n    extended_items: Vec<ExtendedItemV1>,\n    components: ComponentsV1,\n''', 'provider stored candidates')
replace_once(path,
'''    free_ca_mode: Option<FreeCaModeV1>,\n    extended_items: Vec<ExtendedItemV1>,\n    components: ComponentsV1,\n    diagnostics: ProgramRequestDiagnosticsV1,\n''',
'''    free_ca_mode: Option<FreeCaModeV1>,\n    #[serde(default)]\n    short_events: Vec<ShortEventV1>,\n    #[serde(default)]\n    extended_texts: Vec<ExtendedTextV1>,\n    extended_items: Vec<ExtendedItemV1>,\n    components: ComponentsV1,\n    diagnostics: ProgramRequestDiagnosticsV1,\n''', 'provider request candidates')
replace_once(path,
'''        free_ca_mode: request.free_ca_mode,\n        extended_items: request.extended_items,\n        components: request.components,\n''',
'''        free_ca_mode: request.free_ca_mode,\n        short_events: request.short_events,\n        extended_texts: request.extended_texts,\n        extended_items: request.extended_items,\n        components: request.components,\n''', 'provider builder candidates')
replace_once(path,
'''            "freeCaMode",\n            "audioLanguages",\n            "extendedItems",\n''',
'''            "freeCaMode",\n            "audioLanguages",\n            "shortEvents",\n            "extendedTexts",\n            "extendedItems",\n''', 'known top-level candidates')
replace_once(path,
'''    collect_array_unknown(\n        raw.get("extendedItems").unwrap_or(&serde_json::Value::Null),\n''',
'''    collect_array_unknown(\n        raw.get("shortEvents").unwrap_or(&serde_json::Value::Null),\n        "shortEvents",\n        &["languageCode", "title", "text", "parseStatus"],\n        out,\n    );\n    collect_array_unknown(\n        raw.get("extendedTexts").unwrap_or(&serde_json::Value::Null),\n        "extendedTexts",\n        &["languageCode", "text", "parseStatus"],\n        out,\n    );\n    collect_array_unknown(\n        raw.get("extendedItems").unwrap_or(&serde_json::Value::Null),\n''', 'unknown extension candidate arrays')
# Align #31 known-field lists so canonical normalization does not misclassify typed fields as extensions.
replace_once(path,
'''                "streamType",\n                "componentTag",\n                "componentType",\n                "codec",\n                "resolution",\n''',
'''                "streamType",\n                "streamContent",\n                "componentTag",\n                "componentType",\n                "codec",\n                "language",\n                "text",\n                "resolution",\n''', 'video known component fields')
replace_once(path,
'''                "streamType",\n                "componentTag",\n                "componentType",\n                "codec",\n                "language",\n                "secondLanguage",\n                "channelConfiguration",\n                "samplingInfo",\n                "sourceDescriptor",\n                "parseStatus",\n''',
'''                "streamType",\n                "streamContent",\n                "componentTag",\n                "componentType",\n                "codec",\n                "language",\n                "secondLanguage",\n                "channelConfiguration",\n                "simulcastGroupTag",\n                "samplingRate",\n                "samplingInfo",\n                "text",\n                "sourceDescriptor",\n                "main",\n                "multiLingual",\n                "qualityIndicator",\n                "parseStatus",\n''', 'audio known component fields')
replace_once(path,
'''        && data.extended_items.iter().all(valid_extended_item)\n        && valid_components(&data.components)\n''',
'''        && data.short_events.iter().all(valid_short_event)\n        && data.extended_texts.iter().all(valid_extended_text)\n        && data.extended_items.iter().all(valid_extended_item)\n        && valid_components(&data.components)\n''', 'provider candidate validation')
replace_once(path,
'''fn valid_extended_item(v: &ExtendedItemV1) -> bool {\n''',
'''fn valid_short_event(v: &ShortEventV1) -> bool {\n    valid_iso639(&v.language_code) && nonempty(&v.parse_status)\n}\nfn valid_extended_text(v: &ExtendedTextV1) -> bool {\n    valid_iso639(&v.language_code) && nonempty(&v.parse_status)\n}\nfn valid_extended_item(v: &ExtendedItemV1) -> bool {\n''', 'candidate validation helpers')
# Validate the newly typed #31 fields instead of accepting arbitrary integer values.
replace_once(path,
'''    valid_optional_es_pid(v.es_pid)\n        && valid_optional_u8(v.stream_type)\n        && valid_optional_u8(v.component_tag)\n        && valid_optional_u8(v.component_type)\n        && v.codec.as_deref().map(nonempty).unwrap_or(true)\n        && (v.es_pid.is_some() || v.component_tag.is_some())\n        && nonempty(&v.parse_status)\n}\nfn valid_audio_component(v: &AudioComponentV1) -> bool {\n    valid_optional_es_pid(v.es_pid)\n        && valid_optional_u8(v.stream_type)\n        && valid_optional_u8(v.component_tag)\n        && valid_optional_u8(v.component_type)\n        && v.codec.as_deref().map(nonempty).unwrap_or(true)\n        && (v.es_pid.is_some() || v.component_tag.is_some())\n        && valid_optional_iso639(&v.language)\n        && valid_optional_iso639(&v.second_language)\n        && nonempty(&v.parse_status)\n}\n''',
'''    valid_optional_es_pid(v.es_pid)\n        && valid_optional_u8(v.stream_type)\n        && v.stream_content.map(|value| (0..=15).contains(&value)).unwrap_or(true)\n        && valid_optional_u8(v.component_tag)\n        && valid_optional_u8(v.component_type)\n        && v.codec.as_deref().map(nonempty).unwrap_or(true)\n        && valid_optional_iso639(&v.language)\n        && v.text.as_deref().map(nonempty).unwrap_or(true)\n        && (v.es_pid.is_some() || v.component_tag.is_some())\n        && nonempty(&v.parse_status)\n}\nfn valid_audio_component(v: &AudioComponentV1) -> bool {\n    valid_optional_es_pid(v.es_pid)\n        && valid_optional_u8(v.stream_type)\n        && v.stream_content.map(|value| (0..=15).contains(&value)).unwrap_or(true)\n        && valid_optional_u8(v.component_tag)\n        && valid_optional_u8(v.component_type)\n        && v.codec.as_deref().map(nonempty).unwrap_or(true)\n        && (v.es_pid.is_some() || v.component_tag.is_some())\n        && valid_optional_iso639(&v.language)\n        && valid_optional_iso639(&v.second_language)\n        && valid_optional_u8(v.simulcast_group_tag)\n        && v.sampling_rate.map(|value| (0..=7).contains(&value)).unwrap_or(true)\n        && v.text.as_deref().map(nonempty).unwrap_or(true)\n        && v.quality_indicator.map(|value| (0..=3).contains(&value)).unwrap_or(true)\n        && nonempty(&v.parse_status)\n}\n''', 'typed component validation')
# Hard-limit handling: keep the selected/first short event, shorten text before dropping it.
replace_once(path,
'''fn shorten_program_long_text(data: &mut ProgramProviderDataV1, requested_bytes: usize) -> usize {\n    if let Some(item) = data\n''',
'''fn shorten_program_long_text(data: &mut ProgramProviderDataV1, requested_bytes: usize) -> usize {\n    for item in data.extended_texts.iter_mut().rev() {\n        if !item.text.is_empty() {\n            return shorten_utf8_tail(&mut item.text, requested_bytes, false);\n        }\n    }\n    for item in data.short_events.iter_mut().rev() {\n        if !item.text.is_empty() {\n            return shorten_utf8_tail(&mut item.text, requested_bytes, false);\n        }\n        if !item.title.is_empty() {\n            return shorten_utf8_tail(&mut item.title, requested_bytes, false);\n        }\n    }\n    if let Some(item) = data\n''', 'candidate truncation')
replace_once(path,
'''        if data.extended_items.pop().is_some() {\n            note_drop(&mut counts, "extendedItems", 1);\n            continue;\n        }\n        let removed =\n''',
'''        if data.extended_items.pop().is_some() {\n            note_drop(&mut counts, "extendedItems", 1);\n            continue;\n        }\n        if data.extended_texts.len() > 1 {\n            data.extended_texts.pop();\n            note_drop(&mut counts, "extendedTexts", 1);\n            continue;\n        }\n        if data.short_events.len() > 1 {\n            data.short_events.pop();\n            note_drop(&mut counts, "shortEvents", 1);\n            continue;\n        }\n        let removed =\n''', 'candidate hard-limit drop')

# -----------------------------------------------------------------------------
# JSON schema: optional arrays, so existing v1 minimal fixture remains byte-shape compatible.
# -----------------------------------------------------------------------------
schema_path = 'arib_si_engine_rs/schema/program_provider_data_v1.schema.json'
schema = json.loads(read(schema_path))
props = schema['properties']
# Insert order near extendedItems by rebuilding the ordered dict.
new_props = {}
for key, value in props.items():
    if key == 'extendedItems':
        new_props['shortEvents'] = {'type': 'array', 'items': {'$ref': '#/$defs/shortEvent'}}
        new_props['extendedTexts'] = {'type': 'array', 'items': {'$ref': '#/$defs/extendedText'}}
    new_props[key] = value
schema['properties'] = new_props
schema['$defs']['shortEvent'] = {
    'type': 'object', 'additionalProperties': False,
    'required': ['languageCode', 'title', 'text', 'parseStatus'],
    'properties': {
        'languageCode': {'$ref': '#/$defs/iso639'},
        'title': {'type': 'string'},
        'text': {'type': 'string'},
        'parseStatus': {'$ref': '#/$defs/parseStatus'},
    },
}
schema['$defs']['extendedText'] = {
    'type': 'object', 'additionalProperties': False,
    'required': ['languageCode', 'text', 'parseStatus'],
    'properties': {
        'languageCode': {'$ref': '#/$defs/iso639'},
        'text': {'type': 'string'},
        'parseStatus': {'$ref': '#/$defs/parseStatus'},
    },
}
write(schema_path, json.dumps(schema, ensure_ascii=False, indent=2) + '\n')

# -----------------------------------------------------------------------------
# Design docs: keep TvProvider projection document's existing scope exactly unchanged.
# Do not touch the multiple-series-ID row per user instruction.
# -----------------------------------------------------------------------------
path = 'ARIB_SI_EPG_TvProvider投影方針.md'
replace_once(path,
'| 番組名 | `Programs.COLUMN_TITLE` | イベントキーと合わせて保持する | EDCB/EPGStationとも番組名として扱う |',
'| 番組名 | 選択した1言語の `event_name_char` を `Programs.COLUMN_TITLE` へ投影する。受信番組名が空なら架空の番組名を生成せず `NULL` とする。 | 言語タグ付き `shortEvents[]` 候補を保持し、標準列へ選択した同一文字列だけを重複保存しない | Android標準列は1値であり、ARIBは異なる言語の `short_event_descriptor` を複数許可するため |', 'doc program title')
replace_once(path,
'| 短形式イベント本文 | `Programs.COLUMN_SHORT_DESCRIPTION` | 元文字列を保持する | 概要は専用の標準列へ投影し、追加情報がない場合に `COLUMN_LONG_DESCRIPTION` へ同文を重複保存しないため |',
'| 短形式イベント本文 | 番組名と同じ選択言語の `short_event_descriptor.text_char` を `Programs.COLUMN_SHORT_DESCRIPTION` へ投影する | 言語タグ付き `shortEvents[]` 候補を保持し、選択済み文字列だけを別fieldへ重複保存しない | 概要は専用標準列へ出し、異なる言語の本文を連結しないため |', 'doc short text')
replace_once(path,
'| 長形式イベント本文 | `Programs.COLUMN_LONG_DESCRIPTION` | 元文字列を保持する | 詳細説明としてUI表示する |',
'| 長形式イベント本文 | 選択言語と同じ `extended_event_descriptor` の連続descriptorだけを再構成し `Programs.COLUMN_LONG_DESCRIPTION` へ投影する | 言語タグ付き `extendedTexts[]` と `extendedItems[]` を保持し、選択済み平坦化文字列だけを重複保存しない | ARIBは言語ごとに独立した `extended_event_descriptor` 集合を許可するため |', 'doc extended text')
replace_once(path,
'| サービス名 | `Channels.COLUMN_DISPLAY_NAME` | サービス構造を保持する | チャンネル名としてUI表示する |',
'| サービス名 | `Channels.COLUMN_DISPLAY_NAME` | service identity / tune情報とは別に同一表示名を重複保存しない | Android標準列がチャンネル表示名の正本であり、private dataへの同値複製を避けるため |', 'doc service name duplication')
replace_once(path,
'| service_type | `Channels.COLUMN_SERVICE_TYPE` にARIB raw 8-bit codingの符号なし10進文字列を格納する | ARIB raw `service_type` を保持する | AOSPがARIB STD-B10のcoding維持を要求するため |',
'| service_type | `Channels.COLUMN_SERVICE_TYPE` にARIB raw 8-bit codingの符号なし10進文字列を格納する | 同一raw値をprivate dataへ重複保存しない | AOSPがunderlying broadcast standardとしてARIB STD-B10のcoding維持を要求し、標準列自体がその値の正本になるため |', 'doc service type duplication')
replace_once(path,
'| linkage_descriptor | JSON v1 `internal_provider_data` の linkage 構造に保存し、現行仕様では標準列・一般 UI・予約追従へ接続しない。予約追従へ接続する場合は、event identity と authoritative 条件を設計正本へ固定してから扱う。 | Android標準列に自然対応しないため |',
'| linkage_descriptor | JSON v1 `internal_provider_data.linkage[]` に `transportStreamId / originalNetworkId / serviceId / linkageType / parseStatus` と、保存上限を守る診断用 `privateDataPrefixHex` を保持する。private data全量を保存したとは表現しない。現行仕様では標準列・一般 UI・予約追従へ接続しない。予約追従へ接続する場合は、event identity と authoritative 条件を設計正本へ固定してから扱う。 | Android標準列に自然対応せず、ARIB-native identityとbounded diagnostic prefixを私的データとして明示的に分離するため |', 'doc linkage prefix')
replace_once(path,
'| multi-lingual name の候補列 | 現行仕様で選んだ1文字列だけ標準 title/name へ出し、候補列は JSON v1 `internal_provider_data` に保存する。 | 標準 title/name は1値であり、候補列の全量投影先がないため |',
'| multi-lingual event text の候補列 | `short_event_descriptor` はdescriptor順で最初に受理した言語を標準 `TITLE` / `SHORT_DESCRIPTION` の選択言語とし、同じ言語の `extended_event_descriptor` / extended itemだけを `LONG_DESCRIPTION` へ使う。short候補がない場合はextended候補、さらにない場合はextended itemの先頭言語を選択する。異なる言語を1文字列へ連結しない。候補列は `shortEvents[] / extendedTexts[] / extendedItems[]` として JSON v1 `internal_provider_data` に保存する。 | Android標準title/descriptionは単一表示値である一方、ARIBは異なる言語のshort/extended descriptorを複数許可するため |', 'doc multilingual candidates')
replace_once(path,
'''Programs.COLUMN_AUDIO_LANGUAGE:\n  音声コンポーネントから得られる言語情報。\n''',
'''Programs.COLUMN_AUDIO_LANGUAGE:\n  有効な audio_component_descriptor の primary / second ISO 639 language を Android が受理する ISO 639-1 または ISO 639-2/T 表現へ正規化し、重複を除いて comma-separated で格納する。候補がなければ列を設定しない。\n''', 'doc audio language implementation')

# Rust design: lossless language candidates, bounded linkage prefix, no selected-string duplication.
path = 'arib_si_engine_rs/DESIGN_JA.md'
replace_once(path,
'''表示・保存対象として扱う EIT descriptor は現行仕様で構造化変換する。TvProvider 標準列への投影は tv 直下の `ARIB_SI_EPG_TvProvider投影方針.md` を正とし、`internal_provider_data` の具体 schema / canonical encode は本 crate の Rust provider-data serde構造体を SSOT とする。同文書で標準列投影が固定されている component、音声コンポーネント、コンテンツジャンル、free_CA_mode、視聴年齢制限、series id、episode number、音声言語は provider 用フィールドとして出せる。last episode number は通常の `TvContract.Programs` 標準列へ投影する候補ではなく、series の完全構造、イベントグループ、linkage、unknown、診断JSON などと同様に JSON v1 `internal_provider_data` に構造化保存する。Android canonical genre の写像結果、Android rating文字列、runtime選択track、decoder/CAS capability結果はprovider-dataへ保存しない。\n''',
'''表示・保存対象として扱う EIT descriptor は現行仕様で構造化変換する。TvProvider 標準列への投影は tv 直下の `ARIB_SI_EPG_TvProvider投影方針.md` を正とし、`internal_provider_data` の具体 schema / canonical encode は本 crate の Rust provider-data serde構造体を SSOT とする。異なる言語の `short_event_descriptor` は `shortEvents[]`、言語ごとに再構成した `extended_event_descriptor` 本文は `extendedTexts[]`、長形式itemは `extendedItems[]` としてlanguage codeを失わず保持する。標準列へ選択済みのtitle/description文字列だけを別fieldへ重複保存しない。同文書で標準列投影が固定されている component、音声コンポーネント、コンテンツジャンル、free_CA_mode、視聴年齢制限、series id、episode number、音声言語は provider 用フィールドとして出せる。last episode number は通常の `TvContract.Programs` 標準列へ投影する候補ではなく、series の完全構造、イベントグループ、linkageの型付きidentityとbounded private-data prefix、unknown、診断JSON などと同様に JSON v1 `internal_provider_data` に構造化保存する。Android canonical genre の写像結果、Android rating文字列、runtime選択track、decoder/CAS capability結果はprovider-dataへ保存しない。\n''', 'Rust design multilingual/provider data')
# Clarify linkage storage if a general complete-structure phrase appears elsewhere.
text = read(path)
text = text.replace('linkage の完全構造', 'linkage の型付きidentityとbounded private-data prefix')
write(path, text)

# TIS design only references the Rust schema; it does not redefine TvProvider projection scope.
path = 'tis/DESIGN_JA.md'
text = read(path)
anchor = '''Program provider-data の top-level envelope、必須フィールド、検証規則、正規化、安定キー抽出は TIS では再定義しない。正本は `arib_si_engine_rs/DESIGN_JA.md`、`arib_si_engine_rs/schema/program_provider_data_v1.schema.json`、`arib_si_engine_rs/schema/descriptor_diagnostic_v1.schema.json`、`arib_si_engine_rs/testdata/program_provider_data_v1/minimal_clear_program.json` とする。TIS instrumentation テスト用の期待値 JSON を置く場合は Rust 側テストデータとバイト単位で同一に保つ。\n'''
addition = anchor + '''\nEIT文字列をTvProviderへ投影する際、TISはARIBの異なるlanguage codeを1つのtitle/descriptionへ連結しない。Rust snapshotが返す`shortEvents[] / extendedTexts[] / extendedItems[]`から`ARIB_SI_EPG_TvProvider投影方針.md`の単一言語選択規則に従って標準列用文字列を選び、候補列はprovider-data builderへ渡す。受信番組名が空の場合も`event-<eventId>`等の架空titleを生成しない。\n'''
if text.count(anchor) != 1:
    raise SystemExit('TIS provider-data design anchor mismatch')
write(path, text.replace(anchor, addition, 1))

# -----------------------------------------------------------------------------
# Existing Kotlin tests: keep total JUnit count unchanged.
# -----------------------------------------------------------------------------
path = 'tis/tests/src/com/maleicacid/tvinput/tis/EventModelMapperDescriptorTest.kt'
replace_once(path,
'''import com.maleicacid.tvinput.aribsi.AribExtendedItem\n''',
'''import com.maleicacid.tvinput.aribsi.AribExtendedItem\nimport com.maleicacid.tvinput.aribsi.AribShortEventText\nimport com.maleicacid.tvinput.aribsi.AribExtendedEventText\n''', 'mapper test candidate imports')
replace_once(path,
'''            descriptors = AribEventDescriptors(\n                extendedItems = listOf(AribExtendedItem("jpn", "出演", "A")),\n''',
'''            descriptors = AribEventDescriptors(\n                shortEvents = listOf(\n                    AribShortEventText("jpn", "番組", "短い説明"),\n                    AribShortEventText("eng", "Program", "English short"),\n                ),\n                extendedTexts = listOf(\n                    AribExtendedEventText("jpn", "詳細説明"),\n                    AribExtendedEventText("eng", "English details"),\n                ),\n                extendedItems = listOf(\n                    AribExtendedItem("jpn", "出演", "A"),\n                    AribExtendedItem("eng", "Cast", "B"),\n                ),\n''', 'mapper test candidates')
replace_once(path,
'''        check(record.description.contains("【出演】A"))\n''',
'''        check(record.description.contains("【出演】A"))\n        check(!record.description.contains("English details"))\n        check(!record.description.contains("【Cast】B"))\n        check(record.descriptors.shortEvents.size == 2)\n        check(record.descriptors.extendedTexts.size == 2)\n''', 'mapper language assertions')

path = 'tis/tests/src/com/maleicacid/tvinput/tis/TvProviderWriterProgramsTest.kt'
replace_once(path,
'''import com.maleicacid.tvinput.aribsi.AribExtendedItem\n''',
'''import com.maleicacid.tvinput.aribsi.AribExtendedItem\nimport com.maleicacid.tvinput.aribsi.AribShortEventText\nimport com.maleicacid.tvinput.aribsi.AribExtendedEventText\n''', 'writer test candidate imports')
replace_once(path,
'''        check(!providerData.utf8Contains("programKeyB64"))\n    }\n''',
'''        check(!providerData.utf8Contains("programKeyB64"))\n\n        val blankTitle = p.copy(eventId = 11, stableIdentity = "{\\"kind\\":\\"arib-event-v1\\",\\"originalNetworkId\\":4,\\"transportStreamId\\":16625,\\"serviceId\\":101,\\"eventId\\":11}", title = "")\n        val blankResult = writer.upsertPrograms(listOf(blankTitle))\n        check(blankResult.inserted == 1) { blankResult.toString() }\n        val blankRow = store.programs.values.first { it.getAsInteger(TvContract.Programs.COLUMN_EVENT_ID) == 11 }\n        check(blankRow.get(TvContract.Programs.COLUMN_TITLE) == null)\n    }\n''', 'blank title test')
replace_once(path,
'''            descriptors = ProgramDescriptors(\n                extendedItems = listOf(AribExtendedItem("jpn", "出演", "A")),\n''',
'''            descriptors = ProgramDescriptors(\n                shortEvents = listOf(\n                    AribShortEventText("jpn", "News", "desc"),\n                    AribShortEventText("eng", "News EN", "description EN"),\n                ),\n                extendedTexts = listOf(\n                    AribExtendedEventText("jpn", "詳細"),\n                    AribExtendedEventText("eng", "details"),\n                ),\n                extendedItems = listOf(AribExtendedItem("jpn", "出演", "A")),\n''', 'writer provider candidates')
replace_once(path,
'''        check(providerData.utf8Contains("extendedItems"))\n''',
'''        check(providerData.utf8Contains("shortEvents"))\n        check(providerData.utf8Contains("extendedTexts"))\n        check(providerData.utf8Contains("extendedItems"))\n        check(providerData.utf8Contains("News EN"))\n        check(providerData.utf8Contains("description EN"))\n''', 'writer provider candidate assertions')

# Ensure the projection document still does not absorb live-runtime/Tuner scope.
projection = read('ARIB_SI_EPG_TvProvider投影方針.md')
for forbidden in ['TvTrackInfo.Builder', 'setAudioSampleRate(', 'setHardOfHearing(', 'Tuner.scan(', 'onInputStreamIdsReported']:
    if forbidden in projection:
        raise SystemExit(f'projection MD scope expansion detected: {forbidden}')
# User explicitly excluded the multiple-series-ID issue: ensure this commit did not rewrite that row.
if '複数 series_id がある場合は `COLUMN_MULTI_SERIES_ID`' not in projection:
    raise SystemExit('multiple-series-ID row changed unexpectedly')

print('applied projection consistency fixes excluding multiple-series-ID')
