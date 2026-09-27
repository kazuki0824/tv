package com.maleicacid.tvinput.aribsi

import com.maleicacid.tvinput.common.ServiceKey
import com.maleicacid.tvinput.common.TsPid
import org.json.JSONObject

private typealias NativeTransaction = NativeSiSnapshot

class NativeParserCleanupException(
    val status: Int,
) : IllegalStateException("ネイティブ解析器の解放に失敗しました status=$status")

enum class NativeSiFailureReason {
    MODULE_ABNORMAL,
    REGISTRY_POISONED,
    PARSER_POISONED,
    INVALID_HANDLE,
    JNI_INPUT,
    JSON_ENCODING,
    JNI_OUTPUT,
}

class NativeSiException(
    reasonCode: String,
    message: String,
) : IllegalStateException(message) {
    val reason: NativeSiFailureReason = NativeSiFailureReason.valueOf(reasonCode)
}

// 同じ所有者の状態と解放順を維持し、行数だけを理由に責務を分割しない。
// 同じ状態・境界を扱う操作群を一つの所有者に保つ。
@Suppress("LargeClass", "TooManyFunctions")
class NativeAribSiParser : AutoCloseable {
    private var handle: Long = nativeCreate()
    private var discoveryProfile: Int = 0
    private val epgPublication = EpgPublicationPolicy()

    fun buildChannelProviderData(requestJson: String): String {
        val result = nativeBuildChannelProviderData(requestJson)
        return requireNativeString(result)
    }

    fun buildProgramProviderData(requestJson: String): String {
        val result = nativeBuildProgramProviderData(requestJson)
        return requireNativeString(result)
    }

    fun buildProgramKey(
        onid: Int,
        tsid: Int,
        sid: Int,
        eventId: Int,
    ): String = requireNativeString(nativeBuildProgramKey(onid, tsid, sid, eventId))

    fun normalizeProgramProviderData(providerData: ByteArray): String {
        val result = nativeNormalizeProgramProviderData(providerData)
        return requireNativeString(result)
    }

    fun extractProgramKeyResult(providerData: ByteArray): String {
        val result = nativeExtractProgramKeyResult(providerData)
        return requireNativeString(result)
    }

    fun decodeChannelProviderData(providerData: ByteArray): String {
        val result = nativeDecodeChannelProviderData(providerData)
        return requireNativeString(result)
    }

    fun ingestSection(
        pid: TsPid,
        section: ByteArray,
    ): Int {
        check(handle != 0L) { "ネイティブ解析器は終了済みです" }
        return nativeIngestSection(handle, pid.value, section)
    }

    fun lastStatus(): Int = nativeLastStatus(handle)

    @Synchronized
    fun broadcastClockSnapshot(): AribBroadcastClockFact? = readNativeTransaction().broadcastClock

    @Synchronized
    fun pmtPidsForSectionFilters(): Set<TsPid> {
        check(handle != 0L) { "ネイティブ解析器は終了済みです" }
        val values =
            nativeSnapshotPmtPidsForSectionFilters(handle)
                ?: throw NativeSiException("JNI_OUTPUT", "JNIがPMT PID snapshotを返しませんでした")
        return values.mapIndexedTo(linkedSetOf()) { index, value ->
            requireNotNull(TsPid.fromOrNull(value)) {
                "native PMT PID が範囲外です index=$index"
            }
        }
    }

    fun setDiscoveryProfile(profile: Int) {
        check(nativeSetDiscoveryProfile(handle, profile) == SiStatus.OK) {
            "SI discovery profileを設定できません profile=$profile"
        }
        discoveryProfile = profile
    }

    @Synchronized
    fun takeProgramPublishSnapshot(): ProgramPublishSnapshot = buildProgramPublishSnapshot(readNativeTransaction())

    @Synchronized
    fun programStateSnapshot(): ProgramPublishSnapshot = buildProgramPublishSnapshot(readNativeTransaction())

    // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
    @Suppress("MaxLineLength")
    @Synchronized
    fun serviceRegistrationSnapshot(): ServiceRegistrationSnapshot {
        val snapshot = readNativeTransaction()
        return ServiceRegistrationSnapshot(
            discoveryStage = snapshot.discoveryStage,
            tableRequirements = snapshot.tableRequirements,
            services = snapshot.services,
            actualTransports = snapshot.actualTransports.map { TransportKey(it.originalNetwork, it.transportStream) }.toSet(),
            actualTransportMetadata = snapshot.actualTransports,
            semanticFactsByServiceKey = snapshot.serviceSemanticFacts.associateBy { it.serviceKey },
            diagnostics = snapshot.parserDiagnostics,
            eitInstances = snapshot.eitInstances,
        )
    }

    @Synchronized
    fun casDiscoverySnapshot(): CasDiscoverySnapshot {
        val snapshot = readNativeTransaction()
        return CasDiscoverySnapshot(
            services = snapshot.services,
            caMetadata = snapshot.caMetadata,
            pmtPids = snapshot.pmtPids,
            catEmmPids =
                snapshot.catCaMetadata
                    .mapNotNull { it.emmPid }
                    .distinct()
                    .sorted(),
            diagnostics = descriptorDiagnosticsFromEvents(snapshot.events),
            malformedCaDescriptorDiagnostics = snapshot.malformedCaDescriptorDiagnostics,
        )
    }

    @Synchronized
    fun livePlaybackSnapshot(): LivePlaybackSnapshot {
        val snapshot = readNativeTransaction()
        return LivePlaybackSnapshot(
            collectionGeneration = snapshot.collectionGeneration,
            programs = buildProgramPublishSnapshot(snapshot),
            ingestSequence = snapshot.ingestSequence,
            services = snapshot.services,
            caMetadata = snapshot.caMetadata,
            pmtPids = snapshot.pmtPids,
            catEmmPids =
                snapshot.catCaMetadata
                    .mapNotNull { it.emmPid }
                    .distinct()
                    .sorted(),
            semanticFactsByServiceKey = snapshot.serviceSemanticFacts.associateBy { it.serviceKey },
            descriptorDiagnostics = descriptorDiagnosticsFromEvents(snapshot.events),
            parserDiagnostics = snapshot.parserDiagnostics,
            malformedCaDescriptorDiagnostics = snapshot.malformedCaDescriptorDiagnostics,
        )
    }

    // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
    @Suppress("MaxLineLength")
    private fun buildProgramPublishSnapshot(snapshot: NativeTransaction): ProgramPublishSnapshot {
        val publication =
            epgPublication.project(
                discoveryProfile,
                snapshot.collectionGeneration,
                snapshot.events,
                snapshot.eitInstances,
            )
        val publishedEvents = publication.events.toSet()
        return ProgramPublishSnapshot(
            discoveryProfile = discoveryProfile,
            ingestSequence = snapshot.ingestSequence,
            events = publication.events,
            updateWindows = publication.windows,
            authoritativeProgramKeysByService = publication.authoritativeProgramKeysByService,
            eitInstances = snapshot.eitInstances,
            excludedEventDescriptorFacts =
                snapshot.events.filter { it !in publishedEvents }.map { event ->
                    ExcludedEventDescriptorFacts(event.serviceKey, event.stableIdentity, event.eventId, event.source, event.descriptors)
                },
            semanticFactsByServiceKey = snapshot.serviceSemanticFacts.associateBy { it.serviceKey },
            descriptorDiagnostics = descriptorDiagnosticsFromEvents(snapshot.events),
            parserDiagnostics = snapshot.parserDiagnostics,
            malformedCaDescriptorCountByServiceId = snapshot.malformedCaDescriptorCountByServiceId,
        )
    }

    private fun descriptorDiagnosticsFromEvents(events: List<AribEvent>): List<DescriptorDiagnostic> =
        events.flatMap { event ->
            event.descriptors.diagnostics.descriptorDiagnostics
        }

    private fun readNativeTransaction(): NativeTransaction {
        check(handle != 0L) { "ネイティブ解析器は終了済みです" }
        val snapshot =
            nativeSnapshotBulkTyped(handle)
                ?: throw NativeSiException("JNI_OUTPUT", "JNIがtyped SI snapshotを返しませんでした")
        return snapshot.copy(
            events = attachServiceComponentsToEvents(snapshot.events, snapshot.serviceSemanticFacts),
        )
    }

    // Service stream事実をevent componentへ投影するTIS policyだけをKotlin側に残す。
    @Suppress("MaxLineLength")
    private fun attachServiceComponentsToEvents(
        events: List<AribEvent>,
        services: List<ServiceSemanticFacts>,
    ): List<AribEvent> {
        if (events.isEmpty() || services.isEmpty()) return events
        val componentsByService =
            services.associate {
                it.serviceKey to
                    AribComponentProjectionPolicy.componentsForStreams(it.elementaryStreams)
            }
        return events.map { event ->
            val serviceComponents = componentsByService[event.serviceKey]
            val components =
                if (serviceComponents == null) {
                    event.descriptors.components
                } else {
                    AribComponentProjectionPolicy.mergeEventAndServiceComponents(
                        event.descriptors.components,
                        serviceComponents,
                    )
                }
            event.copy(descriptors = event.descriptors.copy(components = components))
        }
    }

    fun decodeAribString(bytes: ByteArray): String = requireNativeString(nativeDecodeAribString(bytes))

    fun decodeAribStringDiagnosticSummary(bytes: ByteArray): String {
        val result = nativeDecodeAribStringDiagnosticSummary(bytes)
        return requireNativeString(result)
    }

    override fun close() {
        val current = handle
        if (current != 0L) {
            val status = nativeDestroy(current)
            if (status != SiStatus.OK) throw NativeParserCleanupException(status)
            handle = 0L
        }
    }

    private external fun nativeBuildChannelProviderData(requestJson: String): String?

    private external fun nativeBuildProgramProviderData(requestJson: String): String?

    private external fun nativeBuildProgramKey(
        onid: Int,
        tsid: Int,
        sid: Int,
        eventId: Int,
    ): String?

    private external fun nativeNormalizeProgramProviderData(providerData: ByteArray): String?

    private external fun nativeExtractProgramKeyResult(providerData: ByteArray): String?

    private external fun nativeDecodeChannelProviderData(providerData: ByteArray): String?

    private external fun nativeCreate(): Long

    private external fun nativeDestroy(handle: Long): Int

    private external fun nativeIngestSection(
        handle: Long,
        pid: Int,
        section: ByteArray,
    ): Int

    private external fun nativeLastStatus(handle: Long): Int

    private external fun nativeSetDiscoveryProfile(
        handle: Long,
        profile: Int,
    ): Int

    private external fun nativeSnapshotBulkTyped(handle: Long): NativeSiSnapshot?

    private external fun nativeSnapshotPmtPidsForSectionFilters(handle: Long): IntArray?

    private external fun nativeDecodeAribString(bytes: ByteArray): String?

    private external fun nativeDecodeAribStringDiagnosticSummary(bytes: ByteArray): String?

    companion object {
        private fun requireNativeString(value: String?): String {
            if (value == null) {
                throw NativeSiException("JNI_OUTPUT", "JNIが例外なしのnullを返しました")
            }
            return value
        }

        // この処理の規格値・ビット幅・単位換算・固定上限をリテラルのまま照合できる形に保つ。
        @Suppress("MagicNumber")
        private fun codecConfigBytes(
            value: String,
            maximumBytes: Int,
        ): ByteArray {
            require(
                value.isNotEmpty() && value.length <= maximumBytes * 2 && value.length % 2 == 0 &&
                    value.all { it.digitToIntOrNull(16) != null },
            ) {
                "AudioSpecificConfigのhexが不正です"
            }
            return value.chunked(2).map { it.toInt(16).toByte() }.toByteArray()
        }

        // この処理の規格値・ビット幅・単位換算・固定上限をリテラルのまま照合できる形に保つ。
        // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
        @Suppress("MagicNumber", "MaxLineLength")
        internal fun probeAacConfiguration(
            adts: ByteArray,
            ascHex: String?,
        ): AribAacConfiguration? {
            val raw = requireNativeString(nativeProbeAacConfiguration(adts, ascHex?.let { codecConfigBytes(it, 255) }))
            val result = JSONObject(raw)
            return when (result.getString("status")) {
                "PENDING" -> {
                    null
                }

                "INVALID" -> {
                    throw IllegalArgumentException(result.getString("reason"))
                }

                "READY" -> {
                    result.getJSONObject("configuration").let {
                        AribAacConfiguration(
                            it.getInt("audioObjectType"),
                            it.getInt("samplingFrequency"),
                            if (it.isNull("extensionSamplingFrequency")) null else it.getInt("extensionSamplingFrequency"),
                            it.getInt("channelConfiguration"),
                            it.getInt("channelCount"),
                            codecConfigBytes(it.getString("audioSpecificConfigHex"), 512),
                        )
                    }
                }

                else -> {
                    error("codec構成probeが未知の状態を返しました")
                }
            }
        }

        @JvmStatic private external fun nativeProbeAacConfiguration(
            adts: ByteArray,
            asc: ByteArray?,
        ): String?

        init {
            System.loadLibrary("maleicacid_arib_si_engine_jni")
        }
    }
}
