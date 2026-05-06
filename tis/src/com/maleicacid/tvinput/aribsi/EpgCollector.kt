package com.maleicacid.tvinput.aribsi

class EpgCollector {
    fun snapshot(events: List<AribEvent>): List<AribEvent> = events.sortedWith(compareBy<AribEvent> { it.serviceKey.originalNetworkId }
        .thenBy { it.serviceKey.transportStreamId }
        .thenBy { it.serviceKey.serviceId }
        .thenBy { it.startTimeMillis })
}
