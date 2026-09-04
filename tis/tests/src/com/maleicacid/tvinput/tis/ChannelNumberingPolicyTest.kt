package com.maleicacid.tvinput.tis

import com.maleicacid.tvinput.aribsi.AribService
import com.maleicacid.tvinput.common.FrequencyHz
import com.maleicacid.tvinput.common.ServiceKey
import com.maleicacid.tvinput.common.StreamSelector
import com.maleicacid.tvinput.common.StreamSelectorType
import com.maleicacid.tvinput.db.ChannelRecord
import org.junit.Test

class ChannelNumberingPolicyTest {
    @Test fun terrestrialUsesRemoteKeyAndStableBranch() {
        val candidate = ScanCandidate(ChannelRecord.DELIVERY_SYSTEM_ISDB_T, FrequencyHz(473_142_857L), displayChannel = "13", physicalChannel = 13)
        val service = AribService(ServiceKey(1, 2, 0x0400), "svc")
        check(ChannelNumberingPolicy.displayNumber(service, 1, candidate) == "011")
    }

    @Test fun terrestrialWithoutRemoteKeyFallsBackToServiceId() {
        val candidate = ScanCandidate(ChannelRecord.DELIVERY_SYSTEM_ISDB_T, FrequencyHz(473_142_857L), displayChannel = "13", physicalChannel = 13)
        val service = AribService(ServiceKey(1, 2, 101), "svc")
        check(ChannelNumberingPolicy.displayNumber(service, null, candidate) == "101")
    }

    @Test fun satelliteUsesBandAndServiceIdWithoutCsStreamSelector() {
        val candidate = ScanCandidate(ChannelRecord.DELIVERY_SYSTEM_ISDB_S, FrequencyHz(1_613_000_000L), displayChannel = "CS1", satelliteBand = "110CS")
        val service = AribService(ServiceKey(1, 2, 333), "svc")
        check(candidate.streamSelector.type == StreamSelectorType.NONE)
        check(ChannelNumberingPolicy.displayNumber(service, null, candidate) == "CS-333")
    }

    @Test fun earthPt1BsRejectsRelativeSelector() {
        val failed = runCatching {
            ScanCandidate(ChannelRecord.DELIVERY_SYSTEM_ISDB_S, FrequencyHz(1_318_000_000L), streamSelector = StreamSelector.relative(1), displayChannel = "BS15/1", satelliteBand = "BS", backendHint = "earth_pt1")
        }.isFailure
        check(failed)
    }

    @Test fun cs110RejectsStreamSelector() {
        val failed = runCatching {
            ScanCandidate(ChannelRecord.DELIVERY_SYSTEM_ISDB_S, FrequencyHz(1_613_000_000L), streamSelector = StreamSelector.tsid(16400), displayChannel = "CS1", satelliteBand = "110CS")
        }.isFailure
        check(failed)
    }

    @Test fun px4BsAllowsRelativeSelector() {
        val candidate = ScanCandidate(ChannelRecord.DELIVERY_SYSTEM_ISDB_S, FrequencyHz(1_318_000_000L), streamSelector = StreamSelector.relative(1), displayChannel = "BS15/1", satelliteBand = "BS", backendHint = "px4")
        check(candidate.streamSelector.type == StreamSelectorType.RELATIVE)
    }
}
