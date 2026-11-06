// 実値の入力・期待値を本体定数と独立に記述する。
@file:Suppress("MagicNumber")

package com.maleicacid.tvinput.tis

import android.content.ComponentName
import android.content.ContentValues
import android.content.Context
import android.content.ContextWrapper
import android.media.tv.TvInputInfo
import android.media.tv.TvInputManager
import android.media.tv.TvTrackInfo
import com.maleicacid.tvinput.aribsi.AribComponentEntry
import com.maleicacid.tvinput.aribsi.AribElementaryStream
import com.maleicacid.tvinput.aribsi.AribService
import com.maleicacid.tvinput.aribsi.AribSiEngine
import com.maleicacid.tvinput.aribsi.BroadcastSystem
import com.maleicacid.tvinput.aribsi.ServiceRegistrationSnapshot
import com.maleicacid.tvinput.aribsi.ServiceSemanticFacts
import com.maleicacid.tvinput.aribsi.SmdSemanticFacts
import com.maleicacid.tvinput.aribsi.SmdSemanticState
import com.maleicacid.tvinput.aribsi.TransportKey
import com.maleicacid.tvinput.common.FrequencyHz
import com.maleicacid.tvinput.common.ServiceKey
import com.maleicacid.tvinput.common.TsPid
import com.maleicacid.tvinput.db.ChannelRecord
import org.junit.After
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config
import org.robolectric.annotation.Implements
import org.robolectric.util.ReflectionHelpers

// 実TIS境界の失敗注入と資源寿命を、正規生成した所有者で検査する。
@Suppress("TooManyFunctions")
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35], shadows = [TisReviewBoundaryTest.FrameworkInputManager::class])
class TisReviewBoundaryTest {
    private val resources = mutableListOf<AutoCloseable>()

    @After
    fun closeResources() {
        resources.asReversed().forEach { it.close() }
    }

    private fun session(): MaleicacidLiveSession {
        val context = RuntimeEnvironment.getApplication()
        return MaleicacidLiveSession(context, context, "review-test", "review-test").also {
            resources += AutoCloseable { it.onRelease() }
        }
    }

    @Implements(TvInputManager::class)
    class FrameworkInputManager

    // 実公開入口から挿入・snapshot失敗・既存rollbackまでを一続きに検証する。
    @Suppress("LongMethod")
    @Test
    fun programSnapshotExceptionRetainsInsertedChannelsForRollback() {
        // 初期化済みのSIエンジンを正常に解放し、解放後snapshotの失敗を注入する。
        val engine =
            AribSiEngine(RuntimeEnvironment.getApplication())
                .apply { close() }
        val controller =
            ChannelScanController(RuntimeEnvironment.getApplication(), "input.test", engine, ScanPurpose.SETUP_SCAN)
        resources += controller
        val key = ServiceKey(4, 0x4010, 101)
        val store =
            object : TvProviderWriter.ChannelStore {
                val channels = linkedMapOf<Long, ContentValues>()

                // 標準整形後に残る型付きstore契約の宣言だけ行長を許容する。
                @Suppress("MaxLineLength")
                override fun indexExistingChannelIds(keys: Set<ServiceKey>): Result<Map<ServiceKey, Long>> = Result.success(emptyMap())

                // 標準整形後に残る型付きstore契約の宣言だけ行長を許容する。
                @Suppress("MaxLineLength")
                override fun indexInitialBrowsablePendingChannelIds(keys: Set<ServiceKey>): Result<Map<ServiceKey, Long>> =
                    Result.success(emptyMap())

                override fun insertChannel(values: ContentValues): Result<Long?> {
                    channels[1L] = ContentValues(values)
                    return Result.success(1L)
                }

                override fun updateChannel(
                    channelId: Long,
                    values: ContentValues,
                ): Result<Int> = error("既存行更新は対象外")
            }
        val rollbackIds = linkedSetOf<Long>()
        val rollbackStore =
            object : TvProviderWriter.ChannelStore by store {
                override fun deleteChannels(channelIds: Set<Long>): Result<Int> {
                    channelIds.forEach { store.channels.remove(it) }
                    return Result.success(channelIds.size)
                }
            }
        val writer = TvProviderWriter("input.test", rollbackStore, testOnly = true)

        fun set(
            name: String,
            value: Any,
        ) {
            ChannelScanController::class.java
                .getDeclaredField(name)
                .apply { isAccessible = true }
                .set(controller, value)
        }
        set("tvProviderWriter", writer)
        set("engine", engine) // close済みnative parserのsnapshot拒否は本番でも起こり得る。
        set(
            "currentCandidate",
            ScanCandidate(ChannelRecord.DELIVERY_SYSTEM_ISDB_T, FrequencyHz(473_142_857L), displayChannel = "13"),
        )
        val facts =
            ServiceSemanticFacts(
                serviceKey = key,
                serviceType = 0x01,
                pmtPidResolved = true,
                pmtParsed = true,
                pcrPidResolved = true,
                elementaryStreams = listOf(AribElementaryStream(TsPid(0x101), 0x1b, null, null, null)),
                requiresCas = false,
                caDescriptorsResolved = true,
                freeCaMode = false,
                smd =
                    SmdSemanticFacts(
                        descriptorPresent = true,
                        syntaxValid = true,
                        systemManagementId = 0x0300,
                        broadcastingFlag = 0,
                        broadcastingIdentifier = 3,
                        broadcastSystem = BroadcastSystem.ISDB_T,
                        additionalBroadcastingIdentification = 0,
                        additionalIdentificationInfoHex = "",
                        semanticState = SmdSemanticState.SUPPORTED_BROADCAST,
                        diagnostic = null,
                    ),
                missingComponents = emptyList(),
                semanticDiagnostics = emptyList(),
                casFactsCanonicalJson = testCasFacts(false),
            )
        val service =
            AribService(
                serviceKey = key,
                name = "NHK",
                serviceType = 0x01,
                pmtPid = TsPid(0x100),
                pcrPid = TsPid(0x101),
                freeCaMode = false,
                streams = facts.elementaryStreams,
            )
        val snapshot =
            ServiceRegistrationSnapshot(
                1,
                emptyList(),
                listOf(service),
                setOf(TransportKey(key.originalNetwork, key.transportStream)),
                emptyList(),
                mapOf(key to facts),
                emptyList(),
            )
        val publish =
            ChannelScanController::class.java
                .getDeclaredMethod(
                    "publishCurrentServiceSnapshot",
                    ChannelScanController.PublishMode::class.java,
                    Set::class.java,
                    ServiceRegistrationSnapshot::class.java,
                    Function1::class.java,
                ).apply { isAccessible = true }
        val failure =
            runCatching {
                publish.invoke(controller, ChannelScanController.PublishMode.SETUP_SCAN, null, snapshot, { id: Long ->
                    rollbackIds.add(id)
                    Unit
                })
            }.exceptionOrNull()
        check(failure is java.lang.reflect.InvocationTargetException && failure.cause is IllegalStateException)
        check(rollbackIds == setOf(1L) && store.channels.keys == rollbackIds)
        check(writer.finalizeSetupChannels(false, rollbackIds, emptySet()).isSuccess)
        check(store.channels.isEmpty())
    }

    @Test fun liveTuneWithoutCurrentServiceFactsRemainsPending() {
        val pending =
            com.maleicacid.tvinput.aribsi.ServicePolicyDecision(
                ServiceKey(4, 1, 1),
                false,
                false,
                false,
                listOf("NO_CURRENT_SERVICE_SEMANTIC_FACTS"),
                com.maleicacid.tvinput.aribsi.ServicePolicyState.PENDING,
            )
        check(MaleicacidLiveSession.initialLiveSiPending(pending))
        // serviceが既に観測済みでも、PMT/PCR/ES等の未完成factは待機を継続する。
        check(MaleicacidLiveSession.initialLiveSiPending(pending.copy(reasons = listOf("NO_VALID_PMT"))))
        check(MaleicacidLiveSession.initialLiveSiPending(pending.copy(reasons = listOf("NO_PCR_PID"))))
        check(
            !MaleicacidLiveSession.initialLiveSiPending(
                pending.copy(state = com.maleicacid.tvinput.aribsi.ServicePolicyState.UNSUPPORTED),
            ),
        )
    }

    @Test fun dualMonoUiTracksSelectPresentationsWithoutInventingAudioEs() {
        val base =
            TunerController.TisTrack(
                id = "audio:274:16",
                type = TvTrackInfo.TYPE_AUDIO,
                pid = TsPid(274),
                streamType = 0x0f,
                componentTag = 0x10,
                componentType = null,
                language = "jpn",
            )
        val unchanged = TunerSelectionPolicy.withDualMonoPresentations(listOf(base)) { null }
        check(unchanged == listOf(base))
        val descriptor =
            AribComponentEntry(
                esPid = base.pid,
                componentTag = base.componentTag,
                language = "jpn",
                secondLanguage = "eng",
                dualMono = true,
            )
        val advertised = TunerSelectionPolicy.withDualMonoPresentations(listOf(base)) { descriptor }
        check(advertised.map { it.id } == listOf("audio:274:16:main", "audio:274:16:sub", "audio:274:16:main_sub"))
        val expectedPresentations =
            listOf(
                PlaybackPipeline.DualMonoPresentation.MAIN,
                PlaybackPipeline.DualMonoPresentation.SUB,
                PlaybackPipeline.DualMonoPresentation.MAIN_SUB,
            )
        check(advertised.map { it.audioPresentation } == expectedPresentations)
        check(advertised.map { it.pid }.distinct() == listOf(base.pid))
        check(advertised.all { TunerSelectionPolicy.physicalAudioTrackId(it) == base.id })
        check(advertised[1].language == "eng")
        val nonDualMono =
            TunerSelectionPolicy.withDualMonoPresentations(listOf(base)) { descriptor.copy(dualMono = false) }
        check(nonDualMono == listOf(base))
    }

    @Test fun videoTrackGeometryRequiresExactPositiveDimensions() {
        check(VideoTrackMetadataPolicy.project(null).width == null)
        val exact = PlaybackPipeline.VideoFormatInfo(0x1b, "video/avc", 1440, 1080)
        val projected = VideoTrackMetadataPolicy.project(null, exact)
        check(projected.width == 1440 && projected.height == 1080)
        check(VideoTrackMetadataPolicy.project(null, exact.copy(width = 0)).width == null)
        check(VideoTrackMetadataPolicy.project(null, exact.copy(height = -1)).height == null)
        val output =
            android.media.MediaFormat.createVideoFormat("video/avc", 1920, 1088).apply {
                setInteger("crop-left", 0)
                setInteger("crop-right", 1919)
                setInteger("crop-top", 0)
                setInteger("crop-bottom", 1079)
            }
        val decoded = PlaybackPipeline.decodedVideoFormatInfo(PlaybackPipeline.VideoCodecKind.AVC, output)
        check(decoded?.width == 1920 && decoded.height == 1080)
        output.setInteger("crop-right", 1920)
        check(PlaybackPipeline.decodedVideoFormatInfo(PlaybackPipeline.VideoCodecKind.AVC, output) == null)
    }

    @Test fun pmtPidAndConfigurationChangesCannotReuseAnotherVideoTrackGeometry() {
        val session = session()
        val controller = ReflectionHelpers.getField<TunerController>(session, "tunerController")
        val key = ServiceKey(4, 1, 1)
        val first = AribElementaryStream(TsPid(0x101), 0x1b, null, null, null)
        val second = first.copy(elementaryPid = TsPid(0x102))
        val service =
            com.maleicacid.tvinput.aribsi
                .AribService(
                    serviceKey = key,
                    name = "test",
                    pcrPid = TsPid(0x100),
                    freeCaMode = false,
                    streams = listOf(first, second),
                )
        val source =
            AvPlaybackSignature(
                key,
                service.pcrPid,
                first.elementaryPid,
                first.streamType,
                null,
                null,
                true,
                false,
                videoConfiguration = DecoderConfigurationIdentity.from(first),
            )
        val exact = PlaybackPipeline.VideoFormatInfo(0x1b, "video/avc", 1440, 1080)
        set(session, "playbackState", PlaybackStartState.Started(source, 7L))
        set(session, "videoTrackFormat", 7L to exact)
        val method =
            MaleicacidLiveSession::class.java
                .getDeclaredMethod(
                    "exactVideoFormatForTrack",
                    service.javaClass,
                    TunerController.TisTrack::class.java,
                ).apply { isAccessible = true }

        fun project(
            current: com.maleicacid.tvinput.aribsi.AribService,
            stream: AribElementaryStream,
        ): Any? = method.invoke(session, current, controller.tracksFor(listOf(stream)).single())
        check(project(service, first) == exact)
        // PMT更新後も旧playback generationが有効な、再起動直前の順序を再現する。
        check(project(service.copy(streams = listOf(second)), second) == null)
        check(project(service, second) == null)
        check(project(service.copy(serviceKey = ServiceKey(4, 1, 2)), first) == null)
        val changed =
            first.copy(
                codecFacts =
                    first.codecFacts.copy(
                        avc =
                            com.maleicacid.tvinput.aribsi
                                .AribAvcSignaling(100, 0, 40),
                    ),
            )
        check(project(service.copy(streams = listOf(changed)), changed) == null)
        set(session, "playbackState", PlaybackStartState.Started(source.copy(videoPid = second.elementaryPid), 8L))
        check(project(service, second) == null)
        set(session, "videoTrackFormat", 8L to exact.copy(width = 1920))
        check((project(service, second) as PlaybackPipeline.VideoFormatInfo).width == 1920)
    }

    @Test fun livePolicyRequiresCurrentLinkageOnlyForResolvedCasServices() {
        val policy =
            com.maleicacid.tvinput.aribsi
                .ServicePolicyDecision(ServiceKey(4, 1, 1), true, true, true, emptyList())
        check(!policy.clearLivePlaybackStaticallyEligible)
        check(!policy.livePlaybackEligible(false) && policy.livePlaybackEligible(true))
        check(!policy.copy(caDescriptorsResolved = false).livePlaybackEligible(true))
        check(!policy.copy(registrationReady = false).livePlaybackEligible(true))
        check(policy.copy(requiresCas = false).livePlaybackEligible(false))
    }

    @Test fun casUnavailableAllowsSameSignatureToRestartAfterRapidRecovery() {
        val session = session()
        val signature = AvPlaybackSignature(ServiceKey(4, 1, 1), null, TsPid(0x101), 0x1b, null, null, false, true)
        val started = PlaybackStartState.Started(signature, 7L)
        set(session, "playbackState", started)
        val unavailable =
            PlaybackPipeline.PlaybackUnavailable(
                PlaybackPipeline.PlaybackUnavailableReason.CAS_NO_KEY,
                "ECM failed before immediate recovery",
                7L,
            )
        val method =
            MaleicacidLiveSession::class.java
                .getDeclaredMethod(
                    "handlePlaybackUnavailable",
                    PlaybackPipeline.PlaybackUnavailable::class.java,
                ).apply { isAccessible = true }
        method.invoke(session, unavailable.copy(generation = 6L))
        val field = MaleicacidLiveSession::class.java.getDeclaredField("playbackState").apply { isAccessible = true }
        check(field.get(session) == started)
        method.invoke(session, unavailable)
        val stopped = field.get(session) as PlaybackStartState
        check(stopped == PlaybackStartState.Stopped)
        check(PlaybackStartTransitions.shouldAttempt(stopped, signature))
    }

    @Test fun setupRejectsAmbiguousFrameworkInputRegistration() {
        var inputs = emptyList<TvInputInfo>()
        val service =
            object : android.media.tv.ITvInputManager.Default() {
                override fun getTvInputList(userId: Int): List<TvInputInfo> = inputs
            }
        val manager = TvInputManager(service, 0)
        val context =
            object : ContextWrapper(RuntimeEnvironment.getApplication()) {
                override fun getSystemService(name: String): Any? =
                    when (name) {
                        Context.TV_INPUT_SERVICE -> manager
                        else -> null
                    }

                override fun getSystemServiceName(serviceClass: Class<*>): String = Context.TV_INPUT_SERVICE
            }

        fun input(id: String): TvInputInfo =
            TvInputInfo
                .Builder(
                    RuntimeEnvironment.getApplication(),
                    ComponentName(RuntimeEnvironment.getApplication(), MaleicacidTvInputService::class.java),
                ).build()
                .also { set(it, "mId", id) }
        check(!TisInputIdResolver.isOwnInputId(context, "own/1"))
        inputs = listOf(input("own/1"))
        check(TisInputIdResolver.isOwnInputId(context, "own/1")) {
            "package=${context.packageName} inputs=${manager.tvInputList.map {
                Triple(
                    it.id,
                    it.serviceInfo.packageName,
                    it.serviceInfo.name,
                )
            }}"
        }
        check(!TisInputIdResolver.isOwnInputId(context, "other/1"))
        inputs = inputs + input("own/2")
        check(!TisInputIdResolver.isOwnInputId(context, "own/1"))
        check(!TisInputIdResolver.isOwnInputId(context, "own/2"))
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
