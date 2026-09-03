from pathlib import Path

ROOT=Path('.')

def read(p): return (ROOT/p).read_text(encoding='utf-8')
def write(p,s): (ROOT/p).write_text(s,encoding='utf-8')
def rep(p,old,new,label,count=1):
    s=read(p); n=s.count(old)
    if n!=count: raise SystemExit(f'{label}: expected {count}, found {n}')
    write(p,s.replace(old,new,count))

# 1) PMT data_component_descriptor: parse DMF/Timing structurally.
p='arib_si_engine_rs/src/service_discovery.rs'
rep(p,
'''    pub data_component_id: Option<u16>,\n    pub language_codes: Vec<String>,\n    pub is_caption: bool,\n    pub is_superimpose: bool,\n''',
'''    pub data_component_id: Option<u16>,\n    pub language_codes: Vec<String>,\n    pub caption_dmf: Option<u8>,\n    pub caption_timing: Option<u8>,\n    pub caption_automatic_presentation: Option<bool>,\n    pub is_caption: bool,\n    pub is_superimpose: bool,\n''','stream caption facts')
rep(p,
'''                data_component_id: None,\n                language_codes: Vec::new(),\n                is_caption: false,\n                is_superimpose: false,\n''',
'''                data_component_id: None,\n                language_codes: Vec::new(),\n                caption_dmf: None,\n                caption_timing: None,\n                caption_automatic_presentation: None,\n                is_caption: false,\n                is_superimpose: false,\n''','stream init')
rep(p,
'''            0xfd if body.len() >= 2 => {\n                let data_component_id = u16::from_be_bytes([body[0], body[1]]);\n                stream.data_component_id = Some(data_component_id);\n                if matches!(data_component_id, 0x0008 | 0x0012) {\n                    stream.is_caption = true;\n                }\n                if data_component_id == 0x0008 && body.get(2).copied() == Some(0x31) {\n                    stream.is_superimpose = true;\n                }\n            }\n''',
'''            0xfd if body.len() >= 2 => {\n                let data_component_id = u16::from_be_bytes([body[0], body[1]]);\n                stream.data_component_id = Some(data_component_id);\n                if data_component_id == 0x0012 {\n                    // Profile C caption remains a caption service. STD-B24 9.6 DMF/Timing syntax applies\n                    // to the ARIB caption/superimpose coding scheme identified by 0x0008.\n                    stream.is_caption = true;\n                } else if data_component_id == 0x0008 {\n                    if let Some(additional_info) = body.get(2).copied() {\n                        let dmf = (additional_info >> 4) & 0x0f;\n                        let timing = additional_info & 0x03;\n                        stream.caption_dmf = Some(dmf);\n                        stream.caption_timing = Some(timing);\n                        stream.caption_automatic_presentation = match dmf {\n                            0x03 => Some(true),\n                            0x0f => None, // caption-management data is authoritative when DMF varies.\n                            _ => Some(false),\n                        };\n                        // ARIB STD-B24 9.6.1 Table 9-16: 01=program-synchronous caption;\n                        // 00=asynchronous and 10=time-synchronous are superimpose timing modes.\n                        stream.is_caption = timing == 0x01;\n                        stream.is_superimpose = matches!(timing, 0x00 | 0x02);\n                    }\n                }\n            }\n''','DMF timing classification')

# DTO carries the parsed PMT facts into Kotlin.
p='arib_si_engine_rs/src/lib.rs'
rep(p,
'''    data_component_id: Option<u16>,\n    is_caption: bool,\n    is_superimpose: bool,\n''',
'''    data_component_id: Option<u16>,\n    caption_dmf: Option<u8>,\n    caption_timing: Option<u8>,\n    caption_automatic_presentation: Option<bool>,\n    is_caption: bool,\n    is_superimpose: bool,\n''','stream dto fields')
rep(p,
'''            data_component_id: stream.data_component_id,\n            is_caption: stream.is_caption,\n            is_superimpose: stream.is_superimpose,\n''',
'''            data_component_id: stream.data_component_id,\n            caption_dmf: stream.caption_dmf,\n            caption_timing: stream.caption_timing,\n            caption_automatic_presentation: stream.caption_automatic_presentation,\n            is_caption: stream.is_caption,\n            is_superimpose: stream.is_superimpose,\n''','stream dto mapping')

# 2) EIT data_contents_descriptor selector: language_tag + ISO639 + DMF.
p='arib_si_engine_rs/src/descriptors.rs'
rep(p,
'''    pub component_groups: Vec<ComponentGroupDescriptor>,\n    pub linkages: Vec<LinkageDescriptor>,\n    pub extended_items: Vec<ExtendedEventItem>,\n''',
'''    pub component_groups: Vec<ComponentGroupDescriptor>,\n    pub linkages: Vec<LinkageDescriptor>,\n    pub caption_selectors: Vec<CaptionSelectorDescriptor>,\n    pub extended_items: Vec<ExtendedEventItem>,\n''','caption selector storage')
rep(p,
'''pub struct LinkageDescriptor {\n    pub transport_stream_id: u16,\n    pub original_network_id: u16,\n    pub service_id: u16,\n    pub linkage_type: u8,\n    pub private_data: Vec<u8>,\n}\n\npub fn parse_event_descriptors''',
'''pub struct LinkageDescriptor {\n    pub transport_stream_id: u16,\n    pub original_network_id: u16,\n    pub service_id: u16,\n    pub linkage_type: u8,\n    pub private_data: Vec<u8>,\n}\n\n#[derive(Clone, Debug, Eq, PartialEq)]\npub struct CaptionLanguageAnnouncement {\n    pub language_tag: u8,\n    pub dmf: u8,\n    pub language_code: String,\n    pub automatic_presentation: Option<bool>,\n}\n\n#[derive(Clone, Debug, Eq, PartialEq)]\npub struct CaptionSelectorDescriptor {\n    pub data_component_id: u16,\n    pub component_tag: u8,\n    pub languages: Vec<CaptionLanguageAnnouncement>,\n}\n\npub fn parse_event_descriptors''','caption selector types')
rep(p,
'''            0x4a => if let Some(v) = parse_linkage_descriptor(body) { out.linkages.push(v); } else { out.diagnostics.push(descriptor_diagnostic(DescriptorParseStatus::MalformedLength, tag, cursor, len, body.len(), body, "linkage_descriptor is shorter than fixed fields")); },\n            _ => {\n''',
'''            0x4a => if let Some(v) = parse_linkage_descriptor(body) { out.linkages.push(v); } else { out.diagnostics.push(descriptor_diagnostic(DescriptorParseStatus::MalformedLength, tag, cursor, len, body.len(), body, "linkage_descriptor is shorter than fixed fields")); },\n            0xc7 => {\n                let is_caption_selector = body.get(0..2).map(|id| id == [0x00, 0x08]).unwrap_or(false);\n                if is_caption_selector {\n                    match parse_caption_data_contents_descriptor(body) {\n                        Some(v) => out.caption_selectors.push(v),\n                        None => out.diagnostics.push(descriptor_diagnostic(DescriptorParseStatus::MalformedLength, tag, cursor, len, body.len(), body, "caption data_contents_descriptor is malformed")),\n                    }\n                } else {\n                    out.unknown.push((tag, body.to_vec()));\n                    out.diagnostics.push(descriptor_diagnostic(DescriptorParseStatus::UnsupportedValue, tag, cursor, len, body.len(), body, "non-caption data_contents_descriptor is preserved for diagnostics only"));\n                }\n            },\n            _ => {\n''','parse c7')
# Insert parser before short-event parser.
rep(p,
'''fn parse_short_event(\n''',
'''fn parse_caption_data_contents_descriptor(body: &[u8]) -> Option<CaptionSelectorDescriptor> {\n    // STD-B10 6.2.28: data_component_id(2), entry_component(1), selector_length(1),\n    // selector, component refs, generic ISO639 language, text.  Reject malformed tail so a\n    // partially valid selector is not promoted from an invalid descriptor.\n    if body.len() < 9 {\n        return None;\n    }\n    let data_component_id = u16::from_be_bytes([body[0], body[1]]);\n    if data_component_id != 0x0008 {\n        return None;\n    }\n    let component_tag = body[2];\n    let selector_len = body[3] as usize;\n    let selector_end = 4usize.checked_add(selector_len)?;\n    if selector_end >= body.len() {\n        return None;\n    }\n    let selector = &body[4..selector_end];\n    if selector.is_empty() {\n        return None;\n    }\n    let num_languages = selector[0] as usize;\n    if selector.len() != 1usize.checked_add(num_languages.checked_mul(4)?)? {\n        return None;\n    }\n    let mut languages = Vec::with_capacity(num_languages);\n    let mut seen_tags = std::collections::BTreeSet::new();\n    for index in 0..num_languages {\n        let base = 1 + index * 4;\n        let flags = selector[base];\n        let language_tag = (flags >> 5) & 0x07;\n        let dmf = flags & 0x0f;\n        let code = &selector[base + 1..base + 4];\n        if !code.iter().all(u8::is_ascii_alphabetic) || !seen_tags.insert(language_tag) {\n            return None;\n        }\n        languages.push(CaptionLanguageAnnouncement {\n            language_tag,\n            dmf,\n            language_code: language(code),\n            automatic_presentation: match dmf {\n                0x03 => Some(true),\n                0x0f => None,\n                _ => Some(false),\n            },\n        });\n    }\n    let num_refs = *body.get(selector_end)? as usize;\n    let refs_end = selector_end.checked_add(1)?.checked_add(num_refs)?;\n    let generic_language_end = refs_end.checked_add(3)?;\n    let text_len_index = generic_language_end;\n    let text_len = *body.get(text_len_index)? as usize;\n    let text_end = text_len_index.checked_add(1)?.checked_add(text_len)?;\n    if text_end != body.len() {\n        return None;\n    }\n    Some(CaptionSelectorDescriptor {\n        data_component_id,\n        component_tag,\n        languages,\n    })\n}\n\nfn parse_short_event(\n''','caption selector parser')

# JSON event snapshot exposes the selector facts.
p='arib_si_engine_rs/src/lib.rs'
rep(p,
'''fn event_linkage_value(event: &EitEvent) -> serde_json::Value {\n''',
'''fn event_caption_selectors_value(event: &EitEvent) -> serde_json::Value {\n    serde_json::Value::Array(\n        event.descriptors.caption_selectors.iter().map(|selector| {\n            serde_json::json!({\n                "dataComponentId": selector.data_component_id,\n                "componentTag": selector.component_tag,\n                "languages": selector.languages.iter().map(|language| {\n                    serde_json::json!({\n                        "languageTag": language.language_tag,\n                        "languageCode": language.language_code,\n                        "dmf": language.dmf,\n                        "automaticPresentation": language.automatic_presentation,\n                        "parseStatus": "OK",\n                    })\n                }).collect::<Vec<_>>(),\n                "parseStatus": "OK",\n            })\n        }).collect(),\n    )\n}\n\nfn event_linkage_value(event: &EitEvent) -> serde_json::Value {\n''','caption selector json helper')
rep(p,
'''            "componentGroups": event_component_groups_value(event),\n            "linkage": event_linkage_value(event),\n''',
'''            "componentGroups": event_component_groups_value(event),\n            "captionSelectors": event_caption_selectors_value(event),\n            "linkage": event_linkage_value(event),\n''','caption selector event json')

# Kotlin typed facts.
p='tis/src/com/maleicacid/tvinput/aribsi/SiModels.kt'
rep(p,
'''    val dataComponentId: Int? = null,\n    val isCaption: Boolean = false,\n    val isSuperimpose: Boolean = false,\n''',
'''    val dataComponentId: Int? = null,\n    val captionDmf: Int? = null,\n    val captionTiming: Int? = null,\n    val captionAutomaticPresentation: Boolean? = null,\n    val isCaption: Boolean = false,\n    val isSuperimpose: Boolean = false,\n''','Kotlin stream caption facts')
rep(p,
'''data class AribParentalRating(\n''',
'''data class AribCaptionLanguage(\n    val languageTag: Int,\n    val languageCode: String,\n    val dmf: Int,\n    val automaticPresentation: Boolean? = null,\n    val parseStatus: String = "OK",\n)\n\ndata class AribCaptionSelector(\n    val dataComponentId: Int,\n    val componentTag: Int,\n    val languages: List<AribCaptionLanguage> = emptyList(),\n    val parseStatus: String = "OK",\n)\n\ndata class AribParentalRating(\n''','Kotlin caption selector types')
rep(p,
'''    val componentGroups: List<AribComponentGroupDescriptor> = emptyList(),\n    val linkage: List<AribLinkage> = emptyList(),\n''',
'''    val componentGroups: List<AribComponentGroupDescriptor> = emptyList(),\n    val captionSelectors: List<AribCaptionSelector> = emptyList(),\n    val linkage: List<AribLinkage> = emptyList(),\n''','Kotlin event selector field')

# Native JSON parsing.
p='tis/src/com/maleicacid/tvinput/aribsi/NativeAribSiParser.kt'
rep(p,
'''            dataComponentId = optIntOrNull(obj, "dataComponentId"),\n            isCaption = obj.optBoolean("isCaption"),\n            isSuperimpose = obj.optBoolean("isSuperimpose"),\n''',
'''            dataComponentId = optIntOrNull(obj, "dataComponentId"),\n            captionDmf = optIntOrNull(obj, "captionDmf"),\n            captionTiming = optIntOrNull(obj, "captionTiming"),\n            captionAutomaticPresentation = optBoolOrNull(obj, "captionAutomaticPresentation"),\n            isCaption = obj.optBoolean("isCaption"),\n            isSuperimpose = obj.optBoolean("isSuperimpose"),\n''','parse stream caption facts')
rep(p,
'''                componentGroups = parseComponentGroups(descriptorsObj.optJSONArray("componentGroups")),\n                linkage = parseLinkage(descriptorsObj.optJSONArray("linkage")),\n''',
'''                componentGroups = parseComponentGroups(descriptorsObj.optJSONArray("componentGroups")),\n                captionSelectors = parseCaptionSelectors(descriptorsObj.optJSONArray("captionSelectors")),\n                linkage = parseLinkage(descriptorsObj.optJSONArray("linkage")),\n''','parse selector in event')
rep(p,
'''private fun parseComponentGroups(array: JSONArray?): List<AribComponentGroupDescriptor> =\n''',
'''private fun parseCaptionSelectors(array: JSONArray?): List<AribCaptionSelector> =\n    (0 until (array?.length() ?: 0)).mapNotNull { index ->\n        val obj = array!!.optJSONObject(index) ?: return@mapNotNull null\n        val dataComponentId = obj.optInt("dataComponentId", -1)\n        val componentTag = obj.optInt("componentTag", -1)\n        if (dataComponentId !in 0..0xffff || componentTag !in 0..0xff) return@mapNotNull null\n        val languagesArray = obj.optJSONArray("languages")\n        val languages = (0 until (languagesArray?.length() ?: 0)).mapNotNull { languageIndex ->\n            val language = languagesArray!!.optJSONObject(languageIndex) ?: return@mapNotNull null\n            val languageTag = language.optInt("languageTag", -1)\n            val languageCode = language.optString("languageCode")\n            val dmf = language.optInt("dmf", -1)\n            if (languageTag !in 0..7 || languageCode.length != 3 || dmf !in 0..15) null else AribCaptionLanguage(\n                languageTag = languageTag,\n                languageCode = languageCode,\n                dmf = dmf,\n                automaticPresentation = optBoolOrNull(language, "automaticPresentation"),\n                parseStatus = language.optString("parseStatus", "OK"),\n            )\n        }.filter { it.parseStatus.equals("OK", ignoreCase = true) }.distinctBy { it.languageTag }\n        AribCaptionSelector(\n            dataComponentId = dataComponentId,\n            componentTag = componentTag,\n            languages = languages,\n            parseStatus = obj.optString("parseStatus", "OK"),\n        )\n    }.filter { it.parseStatus.equals("OK", ignoreCase = true) }\n\nprivate fun parseComponentGroups(array: JSONArray?): List<AribComponentGroupDescriptor> =\n''','selector parsing helper')

print('applied ARIB caption signaling core fixes')
