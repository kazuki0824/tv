package com.maleicacid.tvinput.tis

/** 現AudioTrackだけがfull restartを要求できる。停止中のnullと初回route確定は変更ではない。 */
internal class AudioRouteChangeGate(
    private val generation: Long,
    private val track: Any,
    private var routeId: Int?,
) {
    fun accepts(currentGeneration: Long, currentTrack: Any?): Boolean =
        generation == currentGeneration && track === currentTrack

    fun onRouteChanged(currentGeneration: Long, currentTrack: Any?, nextRouteId: Int?, restart: () -> Unit) {
        if (!accepts(currentGeneration, currentTrack) || nextRouteId == null) return
        val previous = routeId
        routeId = nextRouteId
        if (previous != null && previous != nextRouteId) restart()
    }
}
