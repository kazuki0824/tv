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

    private data class VersionBounds(
        val version: Int,
        val current: Pair<Long, Long>?,
        val previous: Pair<Long, Long>?,
    )

    private val observedBounds = linkedMapOf<ServiceKey, VersionBounds>()

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
        val selected = safeEvents(profile, events, instances)
        val presentFollowing = selected.filter { it.source.tableId == EIT_PRESENT_FOLLOWING_ACTUAL_TABLE_ID }
        val byService = presentFollowing.groupBy { it.serviceKey }
        val authoritative = linkedMapOf<ServiceKey, Set<String>>()
        val windows = mutableListOf<AribEpgUpdateWindow>()
        instances.filter(::isActualPresentFollowing).forEach { instance ->
            val current = byService[instance.serviceKey].orEmpty()
            val complete = EpgSectionPolicy.isComplete(profile, instance)
            val safe = deletionIsSafe(profile, instance, current, complete)
            val keys = current.filter(::preservesIdentity).map { identity(it) }.toSet()
            if (safe) authoritative[instance.serviceKey] = keys
            val observed = observedBounds[instance.serviceKey]
            val currentBounds = mergedBounds(profile, current, null)
            val previousBounds = if (observed?.version == instance.version) observed.previous else observed?.current
            val bounds = mergedBounds(profile, current, previousBounds)
            // 直前完成版だけを保持する。同じ版の再投影でもold/new区間を変えない。
            if (complete) {
                observedBounds[instance.serviceKey] = VersionBounds(instance.version, currentBounds, previousBounds)
            }
            if (complete && bounds != null) {
                windows +=
                    AribEpgUpdateWindow(
                        serviceKey = instance.serviceKey,
                        windowStartMillis = bounds.first,
                        windowEndMillis = bounds.second,
                        validProgramStableIdentities = keys.toList().sorted(),
                        deletionAuthoritative = safe,
                    )
            }
        }
        val occupied = presentFollowing.mapTo(hashSetOf()) { it.serviceKey to it.eventId }
        val schedule =
            selected
                .filter { event ->
                    EpgSectionPolicy.isSchedule(event.source.tableId) && event.title.isNotBlank() &&
                        isProgramRow(profile, event)
                }.sortedBy { it.source.tableId }
                .filter { occupied.add(it.serviceKey to it.eventId) }
                .filter { event ->
                    byService[event.serviceKey].orEmpty().none { pf ->
                        pf.startTimeMillis < event.startTimeMillis + event.durationMillis &&
                            event.startTimeMillis < pf.startTimeMillis + pf.durationMillis
                    }
                }
        return Publication(presentFollowing + schedule, windows, authoritative)
    }

    private fun safeEvents(
        profile: Int,
        events: List<AribEvent>,
        instances: List<EitInstanceState>,
    ): List<AribEvent> {
        val active =
            instances
                .filter { it.currentNextIndicator && !it.inconsistent }
                .associateBy { it.serviceKey to it.tableId }
        return events.filter { event ->
            val source = event.source
            val instance = active[event.serviceKey to source.tableId]
            instance != null && source.version == instance.version &&
                source.sectionNumber in instance.safeSections &&
                EpgSectionPolicy.accepts(profile, source.tableId, source.sectionNumber)
        }
    }

    private fun deletionIsSafe(
        profile: Int,
        instance: EitInstanceState,
        current: List<AribEvent>,
        complete: Boolean,
    ): Boolean {
        val requiredLast = EpgSectionPolicy.requiredLast(profile, instance)
        return complete &&
            (0..requiredLast).all { it in instance.safeSections } &&
            current.all(::preservesIdentity)
    }

    private fun mergedBounds(
        profile: Int,
        current: List<AribEvent>,
        previous: Pair<Long, Long>?,
    ): Pair<Long, Long>? {
        val timed =
            current.filter { isProgramRow(profile, it) }.mapNotNull { event ->
                runCatching { Math.addExact(event.startTimeMillis, event.durationMillis) }
                    .getOrNull()
                    ?.takeIf { event.startTimeMillis in 1L until it }
                    ?.let { event.startTimeMillis to it }
            }
        return (timed + listOfNotNull(previous)).takeIf { it.isNotEmpty() }?.let { ranges ->
            ranges.minOf { it.first } to ranges.maxOf { it.second }
        }
    }

    companion object {
        private const val EIT_PRESENT_FOLLOWING_ACTUAL_TABLE_ID = 0x4e

        private fun isActualPresentFollowing(instance: EitInstanceState): Boolean =
            instance.tableId == EIT_PRESENT_FOLLOWING_ACTUAL_TABLE_ID && instance.currentNextIndicator

        // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
        @Suppress("MaxLineLength")
        fun isProgramRow(
            profile: Int,
            event: AribEvent,
        ): Boolean =
            EpgSectionPolicy.accepts(
                profile,
                event.source.tableId,
                event.source.sectionNumber,
            ) &&
                event.timingState == EitTimingState.DEFINED

        // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
        @Suppress("MaxLineLength")
        private fun preservesIdentity(event: AribEvent): Boolean =
            event.timingState == EitTimingState.DEFINED ||
                event.timingState == EitTimingState.UNDEFINED_TIME

        // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
        @Suppress("MaxLineLength")
        private fun identity(event: AribEvent): String = ProviderDataBridge.buildProgramKey(event.serviceKey, event.eventId)
    }
}
