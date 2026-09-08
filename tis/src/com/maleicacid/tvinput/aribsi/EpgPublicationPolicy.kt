package com.maleicacid.tvinput.aribsi

import com.maleicacid.tvinput.common.ServiceKey

/** 現在の収集に属する放送事実から、Program公開と削除の根拠を作る唯一のowner。 */
internal class EpgPublicationPolicy {
    data class Publication(
        val events: List<AribEvent>,
        val windows: List<AribEpgUpdateWindow>,
        val authoritativeProgramKeysByService: Map<ServiceKey, Set<String>>,
    )

    private var collection: Pair<Int, Long>? = null
    private val observedBounds = linkedMapOf<ServiceKey, Pair<Long, Long>>()

    fun project(
        profile: Int,
        generation: Long,
        events: List<AribEvent>,
        instances: List<EitInstanceState>,
    ): Publication {
        if (collection != (profile to generation)) {
            observedBounds.clear()
            collection = profile to generation
        }
        val selected = events.filter { event ->
            EpgSectionPolicy.accepts(profile, event.source.tableId, event.source.sectionNumber) &&
                instances.any { it.currentNextIndicator && !it.inconsistent && it.tableId == event.source.tableId &&
                    it.serviceKey == event.serviceKey && it.version == event.source.version &&
                    event.source.sectionNumber in it.safeSections }
        }
        val byService = selected.groupBy { it.serviceKey }
        val authoritative = linkedMapOf<ServiceKey, Set<String>>()
        val windows = mutableListOf<AribEpgUpdateWindow>()
        instances.filter { it.tableId == 0x4e && it.currentNextIndicator }.forEach { instance ->
            val current = byService[instance.serviceKey].orEmpty()
            val requiredLast = EpgSectionPolicy.requiredLast(profile, instance)
            val complete = EpgSectionPolicy.isComplete(profile, instance)
            val safe = complete && (0..requiredLast).all { it in instance.safeSections } &&
                current.all { preservesIdentity(it) }
            val keys = current.filter(::preservesIdentity).map { identity(it) }.toSet()
            if (safe) authoritative[instance.serviceKey] = keys
            val timed = current.filter { isProgramRow(profile, it) }.mapNotNull { event ->
                runCatching { Math.addExact(event.startTimeMillis, event.durationMillis) }.getOrNull()
                    ?.takeIf { it > event.startTimeMillis && event.startTimeMillis > 0L }
                    ?.let { event.startTimeMillis to it }
            }
            val previous = observedBounds[instance.serviceKey]
            val bounds = (timed + listOfNotNull(previous)).takeIf { it.isNotEmpty() }?.let { ranges ->
                ranges.minOf { it.first } to ranges.maxOf { it.second }
            }
            // 旧版の時刻は区間の端点にだけ使い、キー集合と削除権限は必ず現版から再計算する。
            if (complete && bounds != null) {
                observedBounds[instance.serviceKey] = bounds
                windows += AribEpgUpdateWindow(
                    serviceKey = instance.serviceKey,
                    windowStartMillis = bounds.first,
                    windowEndMillis = bounds.second,
                    validProgramStableIdentities = keys.toList().sorted(),
                    deletionAuthoritative = safe,
                )
            }
        }
        return Publication(selected, windows, authoritative)
    }

    companion object {
        fun isProgramRow(profile: Int, event: AribEvent): Boolean =
            EpgSectionPolicy.accepts(profile, event.source.tableId, event.source.sectionNumber) && event.timingState == "DEFINED"

        private fun preservesIdentity(event: AribEvent): Boolean =
            event.timingState == "DEFINED" || event.timingState == "UNDEFINED_TIME"

        private fun identity(event: AribEvent): String = ProviderDataBridge.buildProgramKey(event.serviceKey, event.eventId)
    }
}
