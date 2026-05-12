package com.maleicacid.tvinput.aribsi

import org.junit.Test

class NativeAribSiParserJsonR51FixTest {
    @Test fun extendedItemsJsonHandlesEscapedText() {
        val items = NativeAribSiParser.parseExtendedItemsJsonForTest("[{\"description\":\"出\\\"演\",\"text\":\"A\\nB\\\\C{}\"}]")
        check(items.single().itemDescription == "出\"演")
        check(items.single().itemText == "A\nB\\C{}")
    }

    @Test fun malformedExtendedItemsJsonReturnsEmptyAndIncrementsCounter() {
        val before = NativeAribSiParserDiagnostics.extendedItemJsonParseErrors.get()
        val items = NativeAribSiParser.parseExtendedItemsJsonForTest("[{bad]")
        check(items.isEmpty())
        check(NativeAribSiParserDiagnostics.extendedItemJsonParseErrors.get() == before + 1)
    }
}
