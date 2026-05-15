package com.maleicacid.tvinput.tis

internal fun ByteArray.utf8Text(): String = String(this, Charsets.UTF_8)

internal fun ByteArray.utf8Contains(value: String): Boolean = utf8Text().contains(value)
