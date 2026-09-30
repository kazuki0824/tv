package com.maleicacid.tvinput.aribsi.generated


sealed class SiParseStatusDto {

    object Ok : SiParseStatusDto()

    object MalformedLength : SiParseStatusDto()

    object TruncatedDescriptor : SiParseStatusDto()

    object UnsupportedValue : SiParseStatusDto()

    object InvalidSequence : SiParseStatusDto()

    object Unresolved : SiParseStatusDto()
}

