package com.maleicacid.tvinput.tis

import java.util.Locale

/**
 * TvTrackInfo.Builder.setLanguage() に渡す ISO 639-1 / ISO 639-2/T code を正規化する。
 * ISO 表をフルスクラッチせず、java.util.Locale の ISO language data と
 * 旧 ISO 639-2/B alias の最小差分だけを使う。
 */
object LanguageCodeNormalizer {
    private val iso2ToIso3T: Map<String, String> = Locale.getISOLanguages().mapNotNull { iso2 ->
        runCatching { iso2.lowercase(Locale.ROOT) to Locale(iso2).getISO3Language().lowercase(Locale.ROOT) }.getOrNull()
    }.toMap()

    private val iso3T: Set<String> = iso2ToIso3T.values.toSet()

    private val bibliographicAliases = mapOf(
        "alb" to "sqi",
        "arm" to "hye",
        "baq" to "eus",
        "bur" to "mya",
        "chi" to "zho",
        "cze" to "ces",
        "dut" to "nld",
        "fre" to "fra",
        "geo" to "kat",
        "ger" to "deu",
        "gre" to "ell",
        "ice" to "isl",
        "mac" to "mkd",
        "mao" to "mri",
        "may" to "msa",
        "per" to "fas",
        "rum" to "ron",
        "slo" to "slk",
        "tib" to "bod",
        "wel" to "cym",
    )

    fun normalizeForTvTrackLanguage(value: String?): String? {
        val code = value?.trim()?.lowercase(Locale.ROOT)?.takeIf { it.isNotBlank() } ?: return null
        return when (code.length) {
            2 -> iso2ToIso3T[code]
            3 -> bibliographicAliases[code] ?: code.takeIf { it in iso3T }
            else -> null
        }
    }
}
