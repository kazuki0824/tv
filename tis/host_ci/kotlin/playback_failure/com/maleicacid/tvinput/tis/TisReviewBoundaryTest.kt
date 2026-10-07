// 実値の入力・期待値を本体定数と独立に記述する。
@file:Suppress("MagicNumber")

package com.maleicacid.tvinput.tis

import android.content.ContentValues
import android.content.Context
import android.content.ContextWrapper
import android.content.pm.ResolveInfo
import android.content.pm.ServiceInfo
import android.media.tv.TvInputInfo
import android.media.tv.TvInputManager
import com.maleicacid.tvinput.aribsi.AribElementaryStream
import com.maleicacid.tvinput.aribsi.AribService
import com.maleicacid.tvinput.aribsi.BroadcastSystem
import com.maleicacid.tvinput.aribsi.NativeAribCaptionFactParser
import com.maleicacid.tvinput.aribsi.ServiceRegistrationSnapshot
import com.maleicacid.tvinput.aribsi.ServiceSemanticFacts
import com.maleicacid.tvinput.aribsi.SmdSemanticFacts
import com.maleicacid.tvinput.aribsi.SmdSemanticState
import com.maleicacid.tvinput.aribsi.TransportKey
import com.maleicacid.tvinput.common.FrequencyHz
import com.maleicacid.tvinput.common.ServiceKey
import com.maleicacid.tvinput.common.TsPid
import com.maleicacid.tvinput.db.ChannelRecord
import org.junit.Test
import sun.misc.Unsafe
import java.util.concurrent.ConcurrentHashMap

// 実TIS境界の失敗注入を同じhost fixtureで検査し、関数数だけを理由にfixtureを複製しない。
@Suppress("TooManyFunctions")
class TisReviewBoundaryTest {
    // 実公開入口から挿入・snapshot失敗・既存rollbackまでを一続きに検証する。
    @Suppress("LongMethod")
    @Test
    fun programSnapshotExceptionRetainsInsertedChannelsForRollback() {
        val unsafe =
            sun.misc.Unsafe::class.java
                .getDeclaredField("theUnsafe")
                .apply { isAccessible = true }
                .get(null) as sun.misc.Unsafe
        val controller = unsafe.allocateInstance(ChannelScanController::class.java) as ChannelScanController
        val engine = unsafe.allocateInstance(com.maleicacid.tvinput.aribsi.AribSiEngine::class.java)
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
        set("engine", engine) // 未初期化lockで後続Program snapshot取得を確実に失敗させる。
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
        check(failure is java.lang.reflect.InvocationTargetException && failure.cause is NullPointerException)
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

    private val unsafe =
        Unsafe::class.java
            .getDeclaredField("theUnsafe")
            .apply { isAccessible = true }
            .get(null) as Unsafe

    private fun <T> allocate(type: Class<T>): T = type.cast(unsafe.allocateInstance(type))

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

    @Test fun captionManagementAloneDeterminesAdvertisedLanguages() {
        val controller = allocate(TunerController::class.java)
        val languages = ConcurrentHashMap<TsPid, List<NativeAribCaptionFactParser.Language>>()
        set(controller, "captionLanguagesByPid", languages)
        val caption = AribElementaryStream(TsPid(0x130), 6, null, null, null, isCaption = true)
        val superimpose = caption.copy(elementaryPid = TsPid(0x138), isCaption = false, isSuperimpose = true)
        val streams = listOf(caption, superimpose)
        check(controller.tracksFor(streams).isEmpty())
        check(controller.superimposeTrackFor(streams) == null)
        val pending = controller.selectAvStreams(ServiceKey(4, 1, 1), null, streams)
        check(pending.subtitle == caption && pending.subtitleLanguageId == null)

        fun language(tag: Int) = NativeAribCaptionFactParser.Language(tag, "jpn", 0, true, null, 0, 0, 0)
        languages[caption.elementaryPid] = listOf(language(2))
        languages[superimpose.elementaryPid] = listOf(language(7))
        check(controller.tracksFor(streams).isEmpty() && controller.superimposeTrackFor(streams) == null)
        languages[caption.elementaryPid] = listOf(language(0), language(1), language(2))
        languages[superimpose.elementaryPid] = listOf(language(1))
        check(controller.tracksFor(streams).map { it.captionLanguageId } == listOf(1, 2))
        check(controller.superimposeTrackFor(streams)?.captionLanguageId == 2)
        languages.clear()
        check(controller.tracksFor(streams).isEmpty() && controller.superimposeTrackFor(streams) == null)
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
        val controller = allocate(TunerController::class.java)
        val session = allocate(MaleicacidLiveSession::class.java)
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
        val session = allocate(MaleicacidLiveSession::class.java)
        val signature = AvPlaybackSignature(ServiceKey(4, 1, 1), null, TsPid(0x101), 0x1b, null, null, false, true)
        val started = PlaybackStartState.Started(signature, 7L)
        set(session, "playbackState", started)
        val base = android.media.tv.TvInputService.Session::class.java
        base.getDeclaredField("mLock").apply { isAccessible = true }.set(session, Any())
        base.getDeclaredField("mPendingActions").apply { isAccessible = true }.set(session, arrayListOf<Runnable>())
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
        val manager = allocate(TvInputManager::class.java)
        val service =
            object : android.media.tv.ITvInputManager.Default() {
                override fun getTvInputList(userId: Int): List<TvInputInfo> = inputs
            }
        set(manager, "mService", service)
        val context =
            object : ContextWrapper(null) {
                override fun getPackageName() = "com.maleicacid.tvinput"

                override fun getSystemService(name: String): Any? =
                    when (name) {
                        Context.TV_INPUT_SERVICE -> manager
                        else -> null
                    }

                override fun getSystemServiceName(serviceClass: Class<*>): String = Context.TV_INPUT_SERVICE
            }

        fun input(id: String): TvInputInfo =
            allocate(TvInputInfo::class.java).also {
                set(it, "mId", id)
                set(
                    it,
                    "mService",
                    ResolveInfo().apply {
                        serviceInfo =
                            ServiceInfo().apply {
                                packageName = "com.maleicacid.tvinput"
                                name = MaleicacidTvInputService::class.java.name
                            }
                    },
                )
            }
        check(!TisInputIdResolver.isOwnInputId(context, "own/1"))
        inputs = listOf(input("own/1"))
        check(TisInputIdResolver.isOwnInputId(context, "own/1"))
        check(!TisInputIdResolver.isOwnInputId(context, "other/1"))
        inputs = inputs + input("own/2")
        check(!TisInputIdResolver.isOwnInputId(context, "own/1"))
        check(!TisInputIdResolver.isOwnInputId(context, "own/2"))
    }

    @Test fun holdingParentalPermissionDoesNotCommitVideoAvailability() {
        val session = allocate(MaleicacidLiveSession::class.java)
        val field =
            MaleicacidLiveSession::class.java
                .getDeclaredField("lastParentalAccessState")
                .apply { isAccessible = true }
        val allowed = field.type.enumConstants.single { it.toString() == "ALLOWED" }
        field.set(session, allowed)
        set(session, "playbackState", PlaybackStartState.Idle)
        // Framework Sessionの通知経路は未初期化。ここからavailabilityを呼べば試験が失敗する。
        MaleicacidLiveSession::class.java
            .getDeclaredMethod("holdPreviousParentalAccessState", String::class.java)
            .apply { isAccessible = true }
            .invoke(session, "provider query failed after recovery")
        check(field.get(session) == allowed)
        check(
            MaleicacidLiveSession::class.java
                .getDeclaredField("playbackState")
                .apply { isAccessible = true }
                .get(session) ==
                PlaybackStartState.Idle,
        )
    }
}
