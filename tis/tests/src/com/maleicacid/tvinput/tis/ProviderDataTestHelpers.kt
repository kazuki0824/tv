package com.maleicacid.tvinput.tis

internal fun ByteArray.utf8Text(): String = String(this, Charsets.UTF_8)

internal fun ByteArray.utf8Contains(value: String): Boolean = utf8Text().contains(value)

// 手作りの現行request fixtureにも、未解決とclearを区別できる明示的な放送根拠を与える。
internal fun testCasFacts(requiresCas: Boolean = false): String =
    if (requiresCas) {
        """{"pmtPid":256,"parseStatus":"OK","sdtFreeCaMode":true,"descriptors":[{"scop""" +
            """e":"PROGRAM","esPid":null,"caSystemId":5,"caPid":512,"rawDescriptorHex":"09""" +
            """040005e200"}]}"""
    } else {
        """{"pmtPid":null,"parseStatus":"PMT_UNRESOLVED","sdtFreeCaMode":null,"descriptors":[]}"""
    }
