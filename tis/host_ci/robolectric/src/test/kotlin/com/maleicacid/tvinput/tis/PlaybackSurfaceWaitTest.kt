@file:Suppress("MagicNumber")

package com.maleicacid.tvinput.tis

import com.maleicacid.tvinput.aribsi.AribElementaryStream
import com.maleicacid.tvinput.aribsi.AribService
import com.maleicacid.tvinput.aribsi.BroadcastSystem
import com.maleicacid.tvinput.aribsi.ServiceSemanticFacts
import com.maleicacid.tvinput.aribsi.SmdSemanticFacts
import com.maleicacid.tvinput.aribsi.SmdSemanticState
import com.maleicacid.tvinput.common.ServiceKey
import com.maleicacid.tvinput.common.TsPid
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(
    sdk = [35],
    instrumentedPackages = ["com.maleicacid.tvinput.aribsi"],
    shadows = [TunerFrontendLeaseTransitionTest.SiEngineBoundary::class],
)
class PlaybackSurfaceWaitTest {
    @Test fun surfaceWaitDoesNotTurnRepeatedLiveUpdatesIntoPlaybackAttempts() {
        withSurfaceWaitSession(audioOnly = false) { session, service ->
            val state = session.javaClass.getDeclaredField("playbackState").apply { isAccessible = true }
            val diagnostic =
                session.javaClass
                    .getDeclaredField("lastPlaybackGateDiagnostic")
                    .apply { isAccessible = true }
            val start =
                session.javaClass
                    .getDeclaredMethod("maybeStartPlayback", AribService::class.java)
                    .apply { isAccessible = true }
            for (before in listOf(PlaybackStartState.Idle, PlaybackStartState.Stopped)) {
                state.set(session, before)
                repeat(3) {
                    check(start.invoke(session, service) == false)
                    check(state.get(session) == before)
                    check((diagnostic.get(session) as String).contains("START_WAITING_SURFACE"))
                }
            }
        }
    }

    @Test fun audioOnlyPlaybackCanRemainStartedWithoutSurface() {
        withSurfaceWaitSession(audioOnly = true) { session, service ->
            val controller =
                session.javaClass
                    .getDeclaredField("tunerController")
                    .apply { isAccessible = true }
                    .get(session) as TunerController
            val selection = controller.selectAvStreams(service.serviceKey, service.pcrPid, service.streams)
            val signature =
                session.javaClass
                    .getDeclaredMethod(
                        "playbackSignatureFor",
                        AribService::class.java,
                        TunerController.AvStreamSelection::class.java,
                    ).apply { isAccessible = true }
                    .invoke(session, service, selection) as AvPlaybackSignature
            val before = PlaybackStartState.Started(signature, 7L)
            set(session, "playbackState", before)
            val start =
                session.javaClass
                    .getDeclaredMethod("maybeStartPlayback", AribService::class.java)
                    .apply { isAccessible = true }
            check(start.invoke(session, service) == true)
            check(
                session.javaClass
                    .getDeclaredField("playbackState")
                    .apply { isAccessible = true }
                    .get(session) == before,
            )
        }
    }

    // 実Sessionの開始判定を実行する。Controllerの資源は未接続で、不要な開始への到達を失敗にする。
    @Suppress("LongMethod")
    private fun withSurfaceWaitSession(
        audioOnly: Boolean,
        block: (MaleicacidLiveSession, AribService) -> Unit,
    ) {
        val context = RuntimeEnvironment.getApplication()
        val session = MaleicacidLiveSession(context, context, "surface-wait-test", "surface-wait-test")
        try {
            val key = ServiceKey(4, 1, 1)
            val stream = AribElementaryStream(TsPid(0x101), if (audioOnly) 0x03 else 0x02, null, null, null)
            val streams =
                if (audioOnly) {
                    listOf(stream)
                } else {
                    listOf(stream, AribElementaryStream(TsPid(0x102), 0x03, null, null, null))
                }
            val service =
                AribService(
                    serviceKey = key,
                    name = "test",
                    serviceType = if (audioOnly) 0x02 else 0x01,
                    pcrPid = TsPid(0x101),
                    freeCaMode = false,
                    streams = streams,
                )
            val facts =
                ServiceSemanticFacts(
                    serviceKey = key,
                    serviceType = service.serviceType,
                    pmtPidResolved = true,
                    pmtParsed = true,
                    pcrPidResolved = true,
                    elementaryStreams = service.streams,
                    requiresCas = false,
                    caDescriptorsResolved = true,
                    freeCaMode = false,
                    smd =
                        SmdSemanticFacts(
                            true,
                            true,
                            0x0300,
                            0,
                            3,
                            BroadcastSystem.ISDB_T,
                            0,
                            "",
                            SmdSemanticState.SUPPORTED_BROADCAST,
                            null,
                        ),
                    missingComponents = emptyList(),
                    semanticDiagnostics = emptyList(),
                    casFactsCanonicalJson = testCasFacts(false),
                )
            val programs =
                com.maleicacid.tvinput.aribsi.ProgramPublishSnapshot(
                    discoveryProfile = com.maleicacid.tvinput.aribsi.SiDiscoveryProfile.ISDB_T,
                    ingestSequence = 1L,
                    events = emptyList(),
                    updateWindows = emptyList(),
                    semanticFactsByServiceKey = mapOf(key to facts),
                    descriptorDiagnostics = emptyList(),
                    parserDiagnostics = emptyList(),
                )
            val snapshot =
                com.maleicacid.tvinput.aribsi.LivePlaybackSnapshot(
                    1L,
                    programs,
                    1L,
                    listOf(service),
                    emptyList(),
                    emptyMap(),
                    emptyList(),
                    mapOf(key to facts),
                    emptyList(),
                    emptyList(),
                )
            check(
                com.maleicacid.tvinput.aribsi.ServicePolicyEvaluator
                    .evaluateLive(snapshot, key)
                    .livePlaybackEligible(false),
            )
            set(session, "currentService", key)
            set(session, "latestLiveSnapshot", snapshot)
            block(session, service)
        } finally {
            session.onRelease()
        }
    }
}

private fun set(
    target: Any,
    name: String,
    value: Any,
) {
    target.javaClass
        .getDeclaredField(name)
        .apply { isAccessible = true }
        .set(target, value)
}
