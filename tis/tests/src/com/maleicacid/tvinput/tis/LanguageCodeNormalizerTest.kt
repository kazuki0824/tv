package com.maleicacid.tvinput.tis

import org.junit.Test

class LanguageCodeNormalizerTest {
    @Test fun normalizesIso639WithoutScratchTable() {
        check(LanguageCodeNormalizer.normalizeForTvTrackLanguage(null) == null)
        check(LanguageCodeNormalizer.normalizeForTvTrackLanguage("") == null)
        check(LanguageCodeNormalizer.normalizeForTvTrackLanguage("invalid") == null)
        check(LanguageCodeNormalizer.normalizeForTvTrackLanguage("jpn") == "jpn")
        check(LanguageCodeNormalizer.normalizeForTvTrackLanguage("eng") == "eng")
        check(LanguageCodeNormalizer.normalizeForTvTrackLanguage("ja") == "jpn")
        check(LanguageCodeNormalizer.normalizeForTvTrackLanguage("fre") == "fra")
        check(LanguageCodeNormalizer.normalizeForTvTrackLanguage("ger") == "deu")
    }
}
