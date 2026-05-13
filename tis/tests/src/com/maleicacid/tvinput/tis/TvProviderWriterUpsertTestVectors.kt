package com.maleicacid.tvinput.tis

import com.maleicacid.tvinput.common.ServiceKey
import com.maleicacid.tvinput.db.ChannelRecord

/** TvProviderWriter の channel upsert 方針確認用ベクトル。 */
object TvProviderWriterUpsertTestVectors {
    val key = ServiceKey(originalNetworkId = 4, transportStreamId = 16625, serviceId = 101)
    val insertChannel = ChannelRecord(key, displayNumber = "101", displayName = "NHK", frequencyHz = 473_142_857L)
    val updateChannel = ChannelRecord(key, displayNumber = "101", displayName = "NHK G", frequencyHz = 473_142_857L)
    val invalidChannel = ChannelRecord(ServiceKey(originalNetworkId = -1, transportStreamId = 16625, serviceId = 101), displayNumber = "101", displayName = "不正", frequencyHz = 473_142_857L)
}
