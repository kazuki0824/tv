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
        val selected = events.filter { selectedForPublication(profile, it, instances) }
        val byService = selected.groupBy { it.serviceKey }
        val authoritative = linkedMapOf<ServiceKey, Set<String>>()
        val windows = mutableListOf<AribEpgUpdateWindow>()
        instances.filter(::isActualPresentFollowing).forEach { instance ->
            val current = byService[instance.serviceKey].orEmpty()
            val complete = EpgSectionPolicy.isComplete(profile, instance)
            val safe = deletionIsSafe(profile, instance, current, complete)
            val keys = current.filter(::preservesIdentity).map { identity(it) }.toSet()
            if (safe) authoritative[instance.serviceKey] = keys
            val bounds = mergedBounds(profile, current, observedBounds[instance.serviceKey])
            // 旧版の時刻は区間の端点にだけ使い、キー集合と削除権限は必ず現版から再計算する。
            if (complete && bounds != null) {
                observedBounds[instance.serviceKey] = bounds
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
        return Publication(selected, windows, authoritative)
    }

    private fun selectedForPublication(
        profile: Int,
        event: AribEvent,
        instances: List<EitInstanceState>,
    ): Boolean =
        EpgSectionPolicy.accepts(profile, event.source.tableId, event.source.sectionNumber) &&
            instances.any {
                it.currentNextIndicator && !it.inconsistent &&
                    it.tableId == event.source.tableId && it.serviceKey == event.serviceKey &&
                    it.version == event.source.version && event.source.sectionNumber in it.safeSections
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
                    ?.takeIf { it > event.startTimeMillis && event.startTimeMillis > 0L }
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
        ): Boolean = EpgSectionPolicy.accepts(profile, event.source.tableId, event.source.sectionNumber) && event.timingState == "DEFINED"

        // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
        @Suppress("MaxLineLength")
        private fun preservesIdentity(event: AribEvent): Boolean = event.timingState == "DEFINED" || event.timingState == "UNDEFINED_TIME"

        // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
        @Suppress("MaxLineLength")
        private fun identity(event: AribEvent): String = ProviderDataBridge.buildProgramKey(event.serviceKey, event.eventId)
    }
}
