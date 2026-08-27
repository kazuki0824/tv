package com.maleicacid.tvinput.tis

import org.junit.Test

class TunerControllerSectionBoundsTest {
    @Test fun sectionEventDataLengthDecisionIsFixedAt4096Bytes() {
        check(SectionFilterPolicy.dataLengthDecision(0) == SectionFilterPolicy.DataLengthDecision.MALFORMED)
        check(SectionFilterPolicy.dataLengthDecision(-1) == SectionFilterPolicy.DataLengthDecision.MALFORMED)
        check(SectionFilterPolicy.dataLengthDecision(1) == SectionFilterPolicy.DataLengthDecision.ACCEPT)
        check(SectionFilterPolicy.dataLengthDecision(4096) == SectionFilterPolicy.DataLengthDecision.ACCEPT)
        check(SectionFilterPolicy.dataLengthDecision(4097) == SectionFilterPolicy.DataLengthDecision.OVERSIZED)
    }
}
