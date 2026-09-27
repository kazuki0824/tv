package com.maleicacid.tvinput.aribsi.generated


data class ElementaryStreamDto(
    val elementaryPid: Int,
    val streamType: Int,
    val componentTag: Int?,
    val componentType: Int?,
    val streamContent: Int?,
    val languageCodes: kotlin.collections.List<String>,
    val dataComponentId: Int?,
    val captionDmf: Int?,
    val captionTiming: Int?,
    val automaticPresentationOnReception: Boolean?,
    val isCaption: Boolean,
    val isSuperimpose: Boolean,
    val codec: String?,
    val codecKind: com.maleicacid.tvinput.aribsi.generated.ElementaryStreamKindDto?,
    val codecFacts: com.maleicacid.tvinput.aribsi.generated.CodecFactsDto
) {
}

