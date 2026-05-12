package com.maleicacid.tvinput.tis

import org.junit.Test

class TunerControllerSectionBoundsTest {
    @Test fun sectionEventDataLengthDecisionIsFixedAt4096Bytes() {
        check(TunerController.sectionDataLengthDecisionForTest(0) == TunerController.SectionDataLengthDecision.MALFORMED)
        check(TunerController.sectionDataLengthDecisionForTest(-1) == TunerController.SectionDataLengthDecision.MALFORMED)
        check(TunerController.sectionDataLengthDecisionForTest(1) == TunerController.SectionDataLengthDecision.ACCEPT)
        check(TunerController.sectionDataLengthDecisionForTest(4096) == TunerController.SectionDataLengthDecision.ACCEPT)
        check(TunerController.sectionDataLengthDecisionForTest(4097) == TunerController.SectionDataLengthDecision.OVERSIZED)
    }
}
