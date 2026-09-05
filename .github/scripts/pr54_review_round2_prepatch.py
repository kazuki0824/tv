from pathlib import Path

p = Path('.github/scripts/pr54_review_round2_once.py')
text = p.read_text()
start_marker = 'p = Path("tis/src/com/maleicacid/tvinput/tis/PlaybackPipeline.kt")\ntext = p.read_text()\n# AvStreamSelectionにcaptionDiscoveryを追加する。\n'
start = text.index(start_marker)
next_marker = "old = '''        val subtitle = selection.subtitle"
next_pos = text.index(next_marker, start)
block = text[start:next_pos]
fixed = block.replace(
    'p = Path("tis/src/com/maleicacid/tvinput/tis/PlaybackPipeline.kt")',
    'p = Path("tis/src/com/maleicacid/tvinput/tis/TunerController.kt")',
    1,
).replace(
    'val audioComponentType: Int? = audio?.componentType,',
    'val audioComponentType: Int? = null,',
)
fixed += 'p.write_text(text)\n\np = Path("tis/src/com/maleicacid/tvinput/tis/PlaybackPipeline.kt")\ntext = p.read_text()\n'
text = text[:start] + fixed + text[next_pos:]
text = text.replace('!obsolete_section_keys.contains(key)', '!obsolete_section_keys.contains(*key)')

old_script = '''        val selector = when (channel.streamSelector.type) {
            StreamSelectorType.RELATIVE -> StreamSelector.tsid(channel.serviceKey.transportStreamId)
            else -> channel.streamSelector
        }
        val selectorValue = selector.value'''
actual_source = '''        val selector = when (channel.streamSelector.type) {
            com.maleicacid.tvinput.common.StreamSelectorType.RELATIVE -> StreamSelector.tsid(channel.serviceKey.transportStreamId)
            else -> channel.streamSelector
        }'''
if old_script not in text:
    raise SystemExit('F08 main script置換対象が見つかりません')
text = text.replace(old_script, actual_source, 1)

old_bs = '''                    val discovered = JapanIsdbScanPlan.explicitBsCandidatesFromScan(candidate, discovery.streamIds)
                    if (discovered.isNotEmpty()) {
                        diagnostics += "BS stream-ID discovery ${candidate.displayChannel}: ${discovered.size} streams"
                        discovered
                    } else {
                        val fallback = JapanIsdbScanPlan.fallbackBsCandidates(candidate)
                        diagnostics += "BS stream-ID discovery fallback ${candidate.displayChannel}: ${fallback.size} versioned TSID candidates result=${discovery.resultCode} message=${discovery.message}"
                        fallback
                    }'''
actual_bs = '''                val discovered = JapanIsdbScanPlan.explicitBsCandidatesFromScan(candidate, discovery.streamIds)
                if (discovered.isNotEmpty()) {
                    discovered
                } else {
                    val fallback = JapanIsdbScanPlan.fallbackBsCandidates(candidate)
                    diagnostics += ScanDiagnostic(candidate, "AOSP BS scanでstream IDを取得できないためversioned TSID fallbackを使用します result=${discovery.resultCode} message=${discovery.message} fallbackCount=${fallback.size}")
                    fallback
                }'''
if old_bs not in text:
    raise SystemExit('F15 main script置換対象が見つかりません')
text = text.replace(old_bs, actual_bs, 1)
p.write_text(text)
