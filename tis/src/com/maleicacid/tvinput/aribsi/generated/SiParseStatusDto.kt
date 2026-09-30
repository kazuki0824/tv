package com.maleicacid.tvinput.aribsi.generated


sealed class SiParseStatusDto {

    object OK : SiParseStatusDto()

    object MalformedLength : SiParseStatusDto()

    object TruncatedDescriptor : SiParseStatusDto()

    object UnsupportedValue : SiParseStatusDto()

    object InvalidSequence : SiParseStatusDto()

    object UNRESOLVED : SiParseStatusDto()
}

