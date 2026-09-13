// テストの入力・期待値を本体の定数と独立した具体値で記述する。
@file:Suppress("MagicNumber")

package com.maleicacid.tvinput.tis

import com.maleicacid.tvinput.common.FrequencyHz
import com.maleicacid.tvinput.common.ServiceKey
import com.maleicacid.tvinput.db.ChannelRecord

/** TvProviderWriter の channel upsert 方針確認用ベクトル。 */
object TvProviderWriterUpsertTestVectors {
    val key = ServiceKey(originalNetworkId = 4, transportStreamId = 16625, serviceId = 101)
    val insertChannel =
        ChannelRecord(
            key,
            serviceType = 0x01,
            displayNumber = "101",
            displayName = "NHK",
            frequencyHz = FrequencyHz(473_142_857L),
            casFactsCanonicalJson =
                com.maleicacid.tvinput.tis
                    .testCasFacts(false),
        )
    val updateChannel =
        ChannelRecord(
            key,
            serviceType = 0x01,
            displayNumber = "101",
            displayName = "NHK G",
            frequencyHz = FrequencyHz(473_142_857L),
            casFactsCanonicalJson =
                com.maleicacid.tvinput.tis
                    .testCasFacts(false),
        )
    val invalidServiceKey = ServiceKey.fromOrNull(originalNetworkId = -1, transportStreamId = 16625, serviceId = 101)
}
