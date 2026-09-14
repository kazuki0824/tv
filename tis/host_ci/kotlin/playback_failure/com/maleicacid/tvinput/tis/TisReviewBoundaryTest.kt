// 実値の入力・期待値を本体定数と独立に記述する。
@file:Suppress("MagicNumber")

package com.maleicacid.tvinput.tis

import android.content.Context
import android.content.ContextWrapper
import android.content.pm.ResolveInfo
import android.content.pm.ServiceInfo
import android.media.tv.TvInputInfo
import android.media.tv.TvInputManager
import com.maleicacid.tvinput.aribsi.AribElementaryStream
import com.maleicacid.tvinput.aribsi.NativeAribCaptionFactParser
import com.maleicacid.tvinput.common.ServiceKey
import com.maleicacid.tvinput.common.TsPid
import org.junit.Test
import sun.misc.Unsafe
import java.util.concurrent.ConcurrentHashMap

class TisReviewBoundaryTest {
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
