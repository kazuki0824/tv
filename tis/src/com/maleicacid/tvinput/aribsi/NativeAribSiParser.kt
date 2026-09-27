package com.maleicacid.tvinput.aribsi

import com.maleicacid.tvinput.common.NetworkId16
import com.maleicacid.tvinput.common.ServiceId16
import com.maleicacid.tvinput.common.ServiceKey
import com.maleicacid.tvinput.common.TransportStreamId16
import com.maleicacid.tvinput.common.TsPid
import org.json.JSONArray
import org.json.JSONObject

private const val SERIES_U16_MAX = 65_535L
private const val SERIES_REPEAT_LABEL_MAX = 15L
private const val SERIES_PROGRAM_PATTERN_MAX = 7L
private const val SERIES_EPISODE_MAX = 4_095L

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
    private data class NativeTransaction(
        val collectionGeneration: Long,
        val ingestSequence: Long,
        val discoveryStage: Int,
        val broadcastClock: AribBroadcastClockFact?,
        val tableRequirements: List<TableRequirementStatus>,
        val catCaMetadata: List<CaMetadata>,
        val malformedCaDescriptorDiagnostics: List<MalformedCaDescriptorDiagnostic>,
        val malformedCaDescriptorCountByServiceId: Map<ServiceId16, Int>,
        val transportSemanticFacts: List<AribTransport>,
        val events: List<AribEvent>,
        val eitInstances: List<EitInstanceState>,
        val serviceSemanticFacts: List<ServiceSemanticFacts>,
        val parserDiagnostics: List<ParserDiagnostic>,
    ) {
        val services: List<AribService> get() =
            serviceSemanticFacts.map { facts ->
                AribService(
                    serviceKey = facts.serviceKey,
                    name = facts.name,
                    providerName = facts.providerName,
                    serviceType = facts.serviceType,
                    pmtPid = facts.pmtPid,
                    pcrPid = facts.pcrPid,
                    freeCaMode = facts.freeCaMode,
                    streams = facts.elementaryStreams,
                    serviceScopedCaDescriptors = facts.serviceScopedCaDescriptors,
                )
            }
        val caMetadata: List<CaMetadata> get() =
            serviceSemanticFacts.flatMap { facts ->
                facts.serviceScopedCaDescriptors.map { descriptor ->
                    CaMetadata(
                        serviceKey = facts.serviceKey,
                        caSystemId = descriptor.caSystemId,
                        ecmPid = descriptor.caPid,
                        emmPid = null,
                        elementaryPid = descriptor.esPid,
                        privateData = descriptor.privateData,
                        source =
                            if (descriptor.scope ==
                                CaDescriptorScope.ES
                            ) {
                                CaMetadataSource.ELEMENTARY_STREAM
                            } else {
                                CaMetadataSource.PROGRAM
                            },
                    )
                }
            } + catCaMetadata
        val pmtPids: Map<ServiceKey, TsPid> get() =
            serviceSemanticFacts
                .mapNotNull { facts ->
                    facts.pmtPid?.let { facts.serviceKey to it }
                }.toMap()
        val actualTransports: List<AribTransport> get() = transportSemanticFacts.filter { it.sdtActual }
    }

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
        val array = JSONArray(requireNativeString(nativeSnapshotPmtPidsForSectionFiltersJson(handle)))
        return (0 until array.length()).mapTo(linkedSetOf()) { index ->
            requireNotNull(TsPid.fromOrNull(array.getInt(index))) {
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
            parseDescriptorDiagnostics(event.descriptors.diagnostics.descriptorDiagnosticsCanonicalJson)
        }

    private fun parseDescriptorDiagnostics(raw: String): List<DescriptorDiagnostic> {
        val array = runCatching { JSONArray(raw.ifBlank { "[]" }) }.getOrNull() ?: return emptyList()
        return (0 until array.length()).mapNotNull { index ->
            val obj = array.optJSONObject(index) ?: return@mapNotNull null
            val scope = obj.optJSONObject("scope") ?: JSONObject()
            val descriptor = obj.optJSONObject("descriptor") ?: JSONObject()
            DescriptorDiagnostic(
                schema = obj.optString("schema"),
                schemaVersion = obj.optInt("schemaVersion", 0),
                severity = obj.optString("severity"),
                code = obj.optString("code"),
                scope =
                    DescriptorDiagnosticScope(
                        pid = TsPid.fromOrNull(optIntOrNull(scope, "pid")),
                        tableId = optIntOrNull(scope, "tableId"),
                        tableIdExtension = optIntOrNull(scope, "tableIdExtension"),
                        version = optIntOrNull(scope, "version"),
                        sectionNumber = optIntOrNull(scope, "sectionNumber"),
                        originalNetwork = NetworkId16.fromOrNull(optIntOrNull(scope, "originalNetworkId")),
                        transportStream = TransportStreamId16.fromOrNull(optIntOrNull(scope, "transportStreamId")),
                        service = ServiceId16.fromOrNull(optIntOrNull(scope, "serviceId")),
                        eventId = optIntOrNull(scope, "eventId"),
                    ),
                descriptor =
                    DescriptorDiagnosticDescriptor(
                        tag = descriptor.optInt("tag", -1),
                        name = optStringOrNull(descriptor, "name"),
                        offset = descriptor.optInt("offset", -1),
                        declaredLength = descriptor.optInt("declaredLength", -1),
                        actualRemainingLength = descriptor.optInt("actualRemainingLength", -1),
                        parseStatus = parseSiParseStatus(descriptor.getString("parseStatus")),
                        rawPrefixHex = descriptor.optString("rawPrefixHex"),
                    ),
                message = obj.optString("message"),
                rawJson = obj.toString(),
            )
        }
    }

    private fun readNativeTransaction(): NativeTransaction {
        check(handle != 0L) { "ネイティブ解析器は終了済みです" }
        return parseNativeTransactionJson(requireNativeString(nativeSnapshotBulkJson(handle)))
    }

    // この処理の規格値・ビット幅・単位換算・固定上限をリテラルのまま照合できる形に保つ。
    // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
    @Suppress("MagicNumber", "MaxLineLength")
    private fun parseNativeTransactionJson(raw: String): NativeTransaction {
        val root =
            try {
                JSONObject(raw)
            } catch (error: RuntimeException) {
                throw NativeSiException(
                    "JSON_ENCODING",
                    "SI snapshotのJSONが不正です: ${error.message.orEmpty()}",
                )
            }
        validateNativeTransaction(root)
        val serviceFacts = parseServiceSemanticFacts(root.optJSONArray("serviceSemanticFacts"))
        return NativeTransaction(
            collectionGeneration = root.getLong("collectionGeneration"),
            ingestSequence = root.getLong("ingestSequence"),
            discoveryStage = root.getInt("discoveryStage"),
            broadcastClock =
                root.optJSONObject("broadcastClock")?.let { clock ->
                    val tableId = clock.optInt("tableId", -1)
                    val mjd = clock.optInt("mjd", -1)
                    val millisOfDay = clock.optLong("millisOfDay", -1L)
                    if (tableId in setOf(0x70, 0x73) && mjd in 0..0xffff && millisOfDay in 0 until 24L * 60L * 60L * 1_000L) {
                        AribBroadcastClockFact(tableId, mjd, millisOfDay)
                    } else {
                        null
                    }
                },
            tableRequirements = parseTableRequirements(root.optJSONArray("tableRequirements")),
            catCaMetadata = parseCaMetadataList(root.optJSONArray("catCaMetadata")),
            malformedCaDescriptorDiagnostics = parseMalformedCaDescriptorDiagnostics(root.optJSONArray("malformedCaDescriptorDiagnostics")),
            malformedCaDescriptorCountByServiceId = parseMalformedCaDescriptorCounts(root.optJSONArray("malformedCaDescriptorCounts")),
            transportSemanticFacts = parseTransports(root.optJSONArray("transportSemanticFacts")),
            events = attachServiceComponentsToEvents(parseEvents(root.optJSONArray("events")), serviceFacts),
            eitInstances = parseEitInstanceStates(root.optJSONArray("eitInstances")),
            serviceSemanticFacts = serviceFacts,
            parserDiagnostics = parseParserDiagnostics(root.optJSONArray("parserDiagnostics")),
        )
    }

    private fun jsonEncodingError(detail: String): Nothing =
        throw NativeSiException(
            "JSON_ENCODING",
            detail,
        )

    private fun requireExactFields(
        obj: JSONObject,
        context: String,
        vararg expected: String,
    ) {
        val actual = obj.keys().asSequence().toSet()
        val required = expected.toSet()
        if (actual != required) {
            jsonEncodingError(
                "$context の項目集合が不正です missing=${required - actual} extra=${actual - required}",
            )
        }
    }

    private fun requireObject(
        obj: JSONObject,
        key: String,
        context: String,
    ): JSONObject {
        if (!obj.has(key) || obj.isNull(key)) {
            jsonEncodingError("$context.$key が欠落しています")
        }
        return obj.get(key) as? JSONObject
            ?: jsonEncodingError("$context.$key の型がobjectではありません")
    }

    private fun requireNullableObject(
        obj: JSONObject,
        key: String,
        context: String,
    ): JSONObject? {
        if (!obj.has(key)) jsonEncodingError("$context.$key が欠落しています")
        if (obj.isNull(key)) return null
        return obj.get(key) as? JSONObject
            ?: jsonEncodingError("$context.$key の型がobjectまたはnullではありません")
    }

    private fun requireArray(
        obj: JSONObject,
        key: String,
        context: String,
    ): JSONArray {
        if (!obj.has(key) || obj.isNull(key)) {
            jsonEncodingError("$context.$key が欠落しています")
        }
        return obj.get(key) as? JSONArray
            ?: jsonEncodingError("$context.$key の型がarrayではありません")
    }

    private fun requireString(
        obj: JSONObject,
        key: String,
        context: String,
    ): String {
        if (!obj.has(key) || obj.isNull(key)) {
            jsonEncodingError("$context.$key が欠落しています")
        }
        return obj.get(key) as? String
            ?: jsonEncodingError("$context.$key の型がstringではありません")
    }

    private fun requireNullableString(
        obj: JSONObject,
        key: String,
        context: String,
    ): String? {
        if (!obj.has(key)) jsonEncodingError("$context.$key が欠落しています")
        if (obj.isNull(key)) return null
        return obj.get(key) as? String
            ?: jsonEncodingError("$context.$key の型がstringまたはnullではありません")
    }

    private fun requireBoolean(
        obj: JSONObject,
        key: String,
        context: String,
    ): Boolean {
        if (!obj.has(key) || obj.isNull(key)) {
            jsonEncodingError("$context.$key が欠落しています")
        }
        return obj.get(key) as? Boolean
            ?: jsonEncodingError("$context.$key の型がbooleanではありません")
    }

    private fun requireNullableBoolean(
        obj: JSONObject,
        key: String,
        context: String,
    ): Boolean? {
        if (!obj.has(key)) jsonEncodingError("$context.$key が欠落しています")
        if (obj.isNull(key)) return null
        return obj.get(key) as? Boolean
            ?: jsonEncodingError("$context.$key の型がbooleanまたはnullではありません")
    }

    private fun requireInteger(
        obj: JSONObject,
        key: String,
        context: String,
        range: LongRange,
    ): Long {
        if (!obj.has(key) || obj.isNull(key)) {
            jsonEncodingError("$context.$key が欠落しています")
        }
        val value = obj.get(key)
        if (value !is Int && value !is Long) {
            jsonEncodingError("$context.$key の型がintegerではありません")
        }
        val number = (value as Number).toLong()
        if (number !in range) {
            jsonEncodingError("$context.$key が範囲外です value=$number")
        }
        return number
    }

    private fun requireNullableInteger(
        obj: JSONObject,
        key: String,
        context: String,
        range: LongRange,
    ): Long? {
        if (!obj.has(key)) jsonEncodingError("$context.$key が欠落しています")
        if (obj.isNull(key)) return null
        return requireInteger(obj, key, context, range)
    }

    private fun requireStringValue(
        obj: JSONObject,
        key: String,
        context: String,
        allowed: Set<String>,
    ): String {
        val value = requireString(obj, key, context)
        if (value !in allowed) {
            jsonEncodingError("$context.$key が未定義値です value=$value")
        }
        return value
    }

    private fun requireNullableStringValue(
        obj: JSONObject,
        key: String,
        context: String,
        allowed: Set<String>,
    ): String? {
        val value = requireNullableString(obj, key, context) ?: return null
        if (value !in allowed) {
            jsonEncodingError("$context.$key が未定義値です value=$value")
        }
        return value
    }

    private fun requireHex(
        value: String,
        context: String,
    ) {
        if (value.length % 2 != 0 || value.any { it.digitToIntOrNull(16) == null }) {
            jsonEncodingError("$context が偶数長hexではありません")
        }
    }

    private fun validateStringArray(
        array: JSONArray,
        context: String,
    ) {
        for (index in 0 until array.length()) {
            if (array.get(index) !is String) {
                jsonEncodingError("$context[$index] の型がstringではありません")
            }
        }
    }

    private fun validateIntegerArray(
        array: JSONArray,
        context: String,
        range: LongRange,
    ) {
        for (index in 0 until array.length()) {
            val value = array.get(index)
            if (value !is Int && value !is Long) {
                jsonEncodingError("$context[$index] の型がintegerではありません")
            }
            if ((value as Number).toLong() !in range) {
                jsonEncodingError("$context[$index] が範囲外です")
            }
        }
    }

    private fun validateObjectArray(
        array: JSONArray,
        context: String,
        validator: (JSONObject, String) -> Unit,
    ) {
        for (index in 0 until array.length()) {
            val item = array.get(index) as? JSONObject
                ?: jsonEncodingError("$context[$index] の型がobjectではありません")
            validator(item, "$context[$index]")
        }
    }

    private fun validateServiceKey(
        obj: JSONObject,
        context: String,
    ) {
        requireExactFields(
            obj,
            context,
            "originalNetworkId",
            "transportStreamId",
            "serviceId",
        )
        requireInteger(obj, "originalNetworkId", context, 0L..65_535L)
        requireInteger(obj, "transportStreamId", context, 0L..65_535L)
        requireInteger(obj, "serviceId", context, 0L..65_535L)
    }

    private fun validateBroadcastClock(
        obj: JSONObject,
        context: String,
    ) {
        requireExactFields(obj, context, "tableId", "mjd", "millisOfDay")
        requireInteger(obj, "tableId", context, 0L..255L)
        requireInteger(obj, "mjd", context, 0L..65_535L)
        requireInteger(obj, "millisOfDay", context, 0L..86_399_999L)
    }

    private fun validateTableRequirement(
        obj: JSONObject,
        context: String,
    ) {
        requireExactFields(
            obj,
            context,
            "component",
            "originalNetworkId",
            "transportStreamId",
            "serviceId",
            "required",
            "complete",
        )
        requireString(obj, "component", context)
        requireNullableInteger(obj, "originalNetworkId", context, 0L..65_535L)
        requireNullableInteger(obj, "transportStreamId", context, 0L..65_535L)
        requireNullableInteger(obj, "serviceId", context, 0L..65_535L)
        requireBoolean(obj, "required", context)
        requireBoolean(obj, "complete", context)
    }

    private fun validateCaMetadata(
        obj: JSONObject,
        context: String,
    ) {
        requireExactFields(
            obj,
            context,
            "serviceKey",
            "caSystemId",
            "ecmPid",
            "emmPid",
            "elementaryPid",
            "privateDataHex",
            "source",
        )
        requireNullableObject(obj, "serviceKey", context)?.let {
            validateServiceKey(it, "$context.serviceKey")
        }
        requireInteger(obj, "caSystemId", context, 0L..65_535L)
        requireNullableInteger(obj, "ecmPid", context, 0L..8_191L)
        requireNullableInteger(obj, "emmPid", context, 0L..8_191L)
        requireNullableInteger(obj, "elementaryPid", context, 0L..8_191L)
        requireHex(requireString(obj, "privateDataHex", context), "$context.privateDataHex")
        requireStringValue(
            obj,
            "source",
            context,
            setOf("CAT", "PROGRAM", "ES"),
        )
    }

    private fun validateMalformedCaDescriptor(
        obj: JSONObject,
        context: String,
    ) {
        requireExactFields(
            obj,
            context,
            "pid",
            "tableId",
            "tableIdExtension",
            "serviceId",
            "elementaryPid",
            "scope",
            "offset",
            "declaredLength",
            "actualRemainingLength",
            "reason",
            "rawPrefixHex",
        )
        requireInteger(obj, "pid", context, 0L..8_191L)
        requireInteger(obj, "tableId", context, 0L..255L)
        requireNullableInteger(obj, "tableIdExtension", context, 0L..65_535L)
        requireNullableInteger(obj, "serviceId", context, 0L..65_535L)
        requireNullableInteger(obj, "elementaryPid", context, 0L..8_191L)
        requireString(obj, "scope", context)
        requireInteger(obj, "offset", context, 0L..Long.MAX_VALUE)
        requireInteger(obj, "declaredLength", context, 0L..Long.MAX_VALUE)
        requireInteger(obj, "actualRemainingLength", context, 0L..Long.MAX_VALUE)
        requireString(obj, "reason", context)
        requireHex(requireString(obj, "rawPrefixHex", context), "$context.rawPrefixHex")
    }

    private fun validateMalformedCaCount(
        obj: JSONObject,
        context: String,
    ) {
        requireExactFields(obj, context, "serviceId", "count")
        requireInteger(obj, "serviceId", context, 0L..65_535L)
        requireInteger(obj, "count", context, 1L..Long.MAX_VALUE)
    }

    private fun validateTransport(
        obj: JSONObject,
        context: String,
    ) {
        requireExactFields(
            obj,
            context,
            "originalNetworkId",
            "transportStreamId",
            "networkName",
            "transportStreamName",
            "remoteControlKeyId",
            "sdtActual",
        )
        requireInteger(obj, "originalNetworkId", context, 0L..65_535L)
        requireInteger(obj, "transportStreamId", context, 0L..65_535L)
        requireNullableString(obj, "networkName", context)
        requireNullableString(obj, "transportStreamName", context)
        requireNullableInteger(obj, "remoteControlKeyId", context, 0L..255L)
        requireBoolean(obj, "sdtActual", context)
    }

    private fun validateAvc(
        obj: JSONObject,
        context: String,
    ) {
        requireExactFields(obj, context, "profileIdc", "constraintFlags", "levelIdc")
        requireInteger(obj, "profileIdc", context, 0L..255L)
        requireInteger(obj, "constraintFlags", context, 0L..255L)
        requireInteger(obj, "levelIdc", context, 0L..255L)
    }

    private fun validateAudioHeader(
        obj: JSONObject,
        context: String,
    ) {
        requireExactFields(
            obj,
            context,
            "audioObjectType",
            "samplingFrequency",
            "channelConfiguration",
            "extensionSamplingFrequency",
            "coreAudioObjectType",
            "channelCount",
        )
        requireInteger(obj, "audioObjectType", context, 0L..255L)
        requireInteger(obj, "samplingFrequency", context, 0L..4_294_967_295L)
        requireInteger(obj, "channelConfiguration", context, 0L..255L)
        requireNullableInteger(
            obj,
            "extensionSamplingFrequency",
            context,
            0L..4_294_967_295L,
        )
        requireNullableInteger(obj, "coreAudioObjectType", context, 0L..255L)
        requireNullableInteger(obj, "channelCount", context, 0L..255L)
    }

    private fun validateAudioExtension(
        obj: JSONObject,
        context: String,
    ) {
        requireExactFields(
            obj,
            context,
            "profileLevelIndications",
            "audioSpecificConfigHex",
            "header",
        )
        validateIntegerArray(
            requireArray(obj, "profileLevelIndications", context),
            "$context.profileLevelIndications",
            0L..255L,
        )
        requireNullableString(obj, "audioSpecificConfigHex", context)?.let {
            requireHex(it, "$context.audioSpecificConfigHex")
        }
        requireNullableObject(obj, "header", context)?.let {
            validateAudioHeader(it, "$context.header")
        }
    }

    private fun validateCodecFacts(
        obj: JSONObject,
        context: String,
    ) {
        requireExactFields(
            obj,
            context,
            "avc",
            "mpeg4AudioProfileAndLevel",
            "audioExtension",
            "malformed",
            "rawDescriptorsHex",
        )
        requireNullableObject(obj, "avc", context)?.let {
            validateAvc(it, "$context.avc")
        }
        requireNullableInteger(
            obj,
            "mpeg4AudioProfileAndLevel",
            context,
            0L..255L,
        )
        requireNullableObject(obj, "audioExtension", context)?.let {
            validateAudioExtension(it, "$context.audioExtension")
        }
        requireBoolean(obj, "malformed", context)
        requireHex(
            requireString(obj, "rawDescriptorsHex", context),
            "$context.rawDescriptorsHex",
        )
    }

    private fun validateElementaryStream(
        obj: JSONObject,
        context: String,
    ) {
        requireExactFields(
            obj,
            context,
            "codecFacts",
            "codecProfileLevel",
            "codecSignalingResolved",
            "codec",
            "codecKind",
            "elementaryPid",
            "streamType",
            "componentTag",
            "componentType",
            "streamContent",
            "languageCodes",
            "dataComponentId",
            "captionDmf",
            "captionTiming",
            "automaticPresentationOnReception",
            "isCaption",
            "isSuperimpose",
        )
        validateCodecFacts(requireObject(obj, "codecFacts", context), "$context.codecFacts")
        requireNullableString(obj, "codecProfileLevel", context)
        requireBoolean(obj, "codecSignalingResolved", context)
        requireNullableString(obj, "codec", context)
        requireNullableStringValue(
            obj,
            "codecKind",
            context,
            setOf("VIDEO", "AUDIO"),
        )
        requireInteger(obj, "elementaryPid", context, 0L..8_191L)
        requireInteger(obj, "streamType", context, 0L..255L)
        requireNullableInteger(obj, "componentTag", context, 0L..255L)
        requireNullableInteger(obj, "componentType", context, 0L..255L)
        requireNullableInteger(obj, "streamContent", context, 0L..255L)
        validateStringArray(
            requireArray(obj, "languageCodes", context),
            "$context.languageCodes",
        )
        requireNullableInteger(obj, "dataComponentId", context, 0L..65_535L)
        requireNullableInteger(obj, "captionDmf", context, 0L..255L)
        requireNullableInteger(obj, "captionTiming", context, 0L..255L)
        requireNullableBoolean(obj, "automaticPresentationOnReception", context)
        requireBoolean(obj, "isCaption", context)
        requireBoolean(obj, "isSuperimpose", context)
    }

    private fun validateServiceCaDescriptor(
        obj: JSONObject,
        context: String,
    ) {
        requireExactFields(
            obj,
            context,
            "caSystemId",
            "caPid",
            "scope",
            "esPid",
            "rawDescriptorHex",
            "privateDataHex",
        )
        requireInteger(obj, "caSystemId", context, 0L..65_535L)
        requireInteger(obj, "caPid", context, 0L..8_191L)
        requireStringValue(obj, "scope", context, setOf("PROGRAM", "ES"))
        requireNullableInteger(obj, "esPid", context, 0L..8_191L)
        requireHex(
            requireString(obj, "rawDescriptorHex", context),
            "$context.rawDescriptorHex",
        )
        requireHex(
            requireString(obj, "privateDataHex", context),
            "$context.privateDataHex",
        )
    }

    private fun validateSmd(
        obj: JSONObject,
        context: String,
    ) {
        requireExactFields(
            obj,
            context,
            "descriptorPresent",
            "syntaxValid",
            "systemManagementId",
            "broadcastingFlag",
            "broadcastingIdentifier",
            "broadcastSystem",
            "additionalBroadcastingIdentification",
            "additionalIdentificationInfoHex",
            "semanticState",
            "diagnostic",
        )
        requireBoolean(obj, "descriptorPresent", context)
        requireBoolean(obj, "syntaxValid", context)
        requireNullableInteger(obj, "systemManagementId", context, 0L..65_535L)
        requireNullableInteger(obj, "broadcastingFlag", context, 0L..3L)
        requireNullableInteger(obj, "broadcastingIdentifier", context, 0L..63L)
        requireNullableStringValue(
            obj,
            "broadcastSystem",
            context,
            setOf("ISDB_T", "ISDB_S_BS", "ISDB_S_110CS"),
        )
        requireNullableInteger(
            obj,
            "additionalBroadcastingIdentification",
            context,
            0L..255L,
        )
        requireHex(
            requireString(obj, "additionalIdentificationInfoHex", context),
            "$context.additionalIdentificationInfoHex",
        )
        requireStringValue(
            obj,
            "semanticState",
            context,
            setOf(
                "SUPPORTED_BROADCAST",
                "NON_BROADCAST",
                "UNDEFINED_BROADCAST_CLASS",
                "UNSUPPORTED_BROADCAST_SYSTEM",
                "UNDETERMINED_SMD",
            ),
        )
        requireNullableString(obj, "diagnostic", context)
    }

    private fun validateServiceSemanticFacts(
        obj: JSONObject,
        context: String,
    ) {
        requireExactFields(
            obj,
            context,
            "originalNetworkId",
            "transportStreamId",
            "serviceId",
            "serviceType",
            "pmtPidResolved",
            "pmtParsed",
            "pcrPidResolved",
            "elementaryStreams",
            "requiresCas",
            "casFactsCanonicalJson",
            "caDescriptorsResolved",
            "freeCaMode",
            "smd",
            "missingComponents",
            "semanticDiagnostics",
            "name",
            "providerName",
            "pmtPid",
            "pcrPid",
            "serviceScopedCaDescriptors",
        )
        requireInteger(obj, "originalNetworkId", context, 0L..65_535L)
        requireInteger(obj, "transportStreamId", context, 0L..65_535L)
        requireInteger(obj, "serviceId", context, 0L..65_535L)
        requireNullableInteger(obj, "serviceType", context, 0L..255L)
        requireBoolean(obj, "pmtPidResolved", context)
        requireBoolean(obj, "pmtParsed", context)
        requireBoolean(obj, "pcrPidResolved", context)
        validateObjectArray(
            requireArray(obj, "elementaryStreams", context),
            "$context.elementaryStreams",
            ::validateElementaryStream,
        )
        requireBoolean(obj, "requiresCas", context)
        requireString(obj, "casFactsCanonicalJson", context)
        requireBoolean(obj, "caDescriptorsResolved", context)
        requireNullableBoolean(obj, "freeCaMode", context)
        validateSmd(requireObject(obj, "smd", context), "$context.smd")
        validateStringArray(
            requireArray(obj, "missingComponents", context),
            "$context.missingComponents",
        )
        validateStringArray(
            requireArray(obj, "semanticDiagnostics", context),
            "$context.semanticDiagnostics",
        )
        requireNullableString(obj, "name", context)
        requireNullableString(obj, "providerName", context)
        requireNullableInteger(obj, "pmtPid", context, 0L..8_191L)
        requireNullableInteger(obj, "pcrPid", context, 0L..8_191L)
        validateObjectArray(
            requireArray(obj, "serviceScopedCaDescriptors", context),
            "$context.serviceScopedCaDescriptors",
            ::validateServiceCaDescriptor,
        )
    }

    private fun validateEitInstance(
        obj: JSONObject,
        context: String,
    ) {
        requireExactFields(
            obj,
            context,
            "tableId",
            "originalNetworkId",
            "transportStreamId",
            "serviceId",
            "version",
            "currentNextIndicator",
            "lastSectionNumber",
            "receivedSections",
            "missingSections",
            "safeSections",
            "complete",
            "inconsistent",
        )
        requireInteger(obj, "tableId", context, 0L..255L)
        requireInteger(obj, "originalNetworkId", context, 0L..65_535L)
        requireInteger(obj, "transportStreamId", context, 0L..65_535L)
        requireInteger(obj, "serviceId", context, 0L..65_535L)
        requireInteger(obj, "version", context, 0L..31L)
        requireBoolean(obj, "currentNextIndicator", context)
        requireInteger(obj, "lastSectionNumber", context, 0L..255L)
        validateIntegerArray(
            requireArray(obj, "receivedSections", context),
            "$context.receivedSections",
            0L..255L,
        )
        validateIntegerArray(
            requireArray(obj, "missingSections", context),
            "$context.missingSections",
            0L..255L,
        )
        validateIntegerArray(
            requireArray(obj, "safeSections", context),
            "$context.safeSections",
            0L..255L,
        )
        requireBoolean(obj, "complete", context)
        requireBoolean(obj, "inconsistent", context)
    }

    private fun validateParserDiagnostic(
        obj: JSONObject,
        context: String,
    ) {
        requireExactFields(obj, context, "code", "message", "severity")
        requireString(obj, "code", context)
        requireString(obj, "message", context)
        requireString(obj, "severity", context)
    }

    private fun validateProgramKey(
        obj: JSONObject,
        context: String,
    ) {
        requireExactFields(
            obj,
            context,
            "kind",
            "originalNetworkId",
            "transportStreamId",
            "serviceId",
            "eventId",
        )
        requireStringValue(obj, "kind", context, setOf("arib-event-v1"))
        requireInteger(obj, "originalNetworkId", context, 0L..65_535L)
        requireInteger(obj, "transportStreamId", context, 0L..65_535L)
        requireInteger(obj, "serviceId", context, 0L..65_535L)
        requireInteger(obj, "eventId", context, 0L..65_535L)
    }

    private fun validateTiming(
        obj: JSONObject,
        context: String,
    ) {
        requireExactFields(
            obj,
            context,
            "state",
            "rawStartTimeHex",
            "rawDurationHex",
            "startUtcMillis",
            "endUtcMillis",
            "durationMillis",
        )
        requireStringValue(
            obj,
            "state",
            context,
            setOf(
                "DEFINED",
                "UNDEFINED_TIME",
                "BOTH_TIMING_UNDEFINED",
                "MALFORMED_TIMING",
            ),
        )
        requireHex(requireString(obj, "rawStartTimeHex", context), "$context.rawStartTimeHex")
        requireHex(requireString(obj, "rawDurationHex", context), "$context.rawDurationHex")
        requireInteger(obj, "startUtcMillis", context, Long.MIN_VALUE..Long.MAX_VALUE)
        requireInteger(obj, "endUtcMillis", context, Long.MIN_VALUE..Long.MAX_VALUE)
        requireInteger(obj, "durationMillis", context, Long.MIN_VALUE..Long.MAX_VALUE)
    }

    private fun validateProgramSource(
        obj: JSONObject,
        context: String,
    ) {
        requireExactFields(
            obj,
            context,
            "pid",
            "tableId",
            "version",
            "sectionNumber",
            "lastSectionNumber",
        )
        requireInteger(obj, "pid", context, 0L..8_191L)
        requireInteger(obj, "tableId", context, 0L..255L)
        requireInteger(obj, "version", context, 0L..31L)
        requireInteger(obj, "sectionNumber", context, 0L..255L)
        requireInteger(obj, "lastSectionNumber", context, 0L..255L)
    }

    private fun validateSeries(
        obj: JSONObject,
        context: String,
    ) {
        requireExactFields(
            obj,
            context,
            "seriesId",
            "repeatLabel",
            "programPattern",
            "expireDateValid",
            "expireDate",
            "episodeNumber",
            "lastEpisodeNumber",
            "name",
            "parseStatus",
        )
        requireInteger(obj, "seriesId", context, 0L..65_535L)
        requireInteger(obj, "repeatLabel", context, 0L..255L)
        requireInteger(obj, "programPattern", context, 0L..255L)
        requireBoolean(obj, "expireDateValid", context)
        requireNullableInteger(obj, "expireDate", context, 0L..65_535L)
        requireInteger(obj, "episodeNumber", context, 0L..65_535L)
        requireInteger(obj, "lastEpisodeNumber", context, 0L..65_535L)
        requireNullableString(obj, "name", context)
        requireStringValue(obj, "parseStatus", context, setOf("OK"))
    }

    private fun validateComponentEntries(
        obj: JSONObject,
        context: String,
    ) {
        requireExactFields(obj, context, "video", "audio", "subtitle", "data")
        val video = requireArray(obj, "video", context)
        val audio = requireArray(obj, "audio", context)
        val subtitle = requireArray(obj, "subtitle", context)
        val data = requireArray(obj, "data", context)
        validateObjectArray(video, "$context.video", ::validateVideoComponent)
        validateObjectArray(audio, "$context.audio", ::validateAudioComponent)
        if (subtitle.length() != 0 || data.length() != 0) {
            jsonEncodingError("$context のsubtitle/data wire値は空でなければなりません")
        }
    }

    private fun validateVideoComponent(
        obj: JSONObject,
        context: String,
    ) {
        requireExactFields(
            obj,
            context,
            "streamContent",
            "componentTag",
            "componentType",
            "language",
            "text",
            "resolution",
            "scan",
            "aspect",
            "profileLevel",
            "sourceDescriptor",
            "parseStatus",
        )
        requireInteger(obj, "streamContent", context, 0L..255L)
        requireInteger(obj, "componentTag", context, 0L..255L)
        requireInteger(obj, "componentType", context, 0L..255L)
        requireString(obj, "language", context)
        requireString(obj, "text", context)
        requireNullableString(obj, "resolution", context)
        requireNullableString(obj, "scan", context)
        requireNullableString(obj, "aspect", context)
        if (!obj.isNull("profileLevel")) {
            jsonEncodingError("$context.profileLevel はnullでなければなりません")
        }
        requireStringValue(
            obj,
            "sourceDescriptor",
            context,
            setOf("component_descriptor"),
        )
        requireStringValue(obj, "parseStatus", context, setOf("OK"))
    }

    private fun validateAudioComponent(
        obj: JSONObject,
        context: String,
    ) {
        requireExactFields(
            obj,
            context,
            "streamContent",
            "componentTag",
            "componentType",
            "streamType",
            "language",
            "secondLanguage",
            "channelConfiguration",
            "channelCount",
            "simulcastGroupTag",
            "samplingRate",
            "samplingInfo",
            "sampleRateHz",
            "audioDescription",
            "hardOfHearing",
            "dualMono",
            "text",
            "sourceDescriptor",
            "main",
            "multiLingual",
            "qualityIndicator",
            "parseStatus",
        )
        requireInteger(obj, "streamContent", context, 0L..255L)
        requireInteger(obj, "componentTag", context, 0L..255L)
        requireInteger(obj, "componentType", context, 0L..255L)
        requireInteger(obj, "streamType", context, 0L..255L)
        requireString(obj, "language", context)
        requireNullableString(obj, "secondLanguage", context)
        requireNullableString(obj, "channelConfiguration", context)
        requireNullableInteger(obj, "channelCount", context, 0L..255L)
        requireInteger(obj, "simulcastGroupTag", context, 0L..255L)
        requireInteger(obj, "samplingRate", context, 0L..255L)
        requireNullableString(obj, "samplingInfo", context)
        requireNullableInteger(obj, "sampleRateHz", context, 0L..4_294_967_295L)
        requireBoolean(obj, "audioDescription", context)
        requireBoolean(obj, "hardOfHearing", context)
        requireBoolean(obj, "dualMono", context)
        requireString(obj, "text", context)
        requireStringValue(
            obj,
            "sourceDescriptor",
            context,
            setOf("audio_component_descriptor"),
        )
        requireBoolean(obj, "main", context)
        requireBoolean(obj, "multiLingual", context)
        requireInteger(obj, "qualityIndicator", context, 0L..255L)
        requireStringValue(obj, "parseStatus", context, setOf("OK"))
    }

    private fun validateEventDescriptors(
        obj: JSONObject,
        context: String,
    ) {
        requireExactFields(
            obj,
            context,
            "shortEvents",
            "extendedTexts",
            "extendedItems",
            "component",
            "audio",
            "genres",
            "eventGroups",
            "componentGroups",
            "linkage",
            "freeCaMode",
            "series",
            "seriesCandidates",
            "seriesCandidatesCanonicalJson",
            "components",
            "diagnostics",
            "parentalRatings",
        )
        validateObjectArray(
            requireArray(obj, "shortEvents", context),
            "$context.shortEvents",
        ) { item, itemContext ->
            requireExactFields(item, itemContext, "languageCode", "title", "text", "parseStatus")
            requireString(item, "languageCode", itemContext)
            requireString(item, "title", itemContext)
            requireString(item, "text", itemContext)
            requireStringValue(item, "parseStatus", itemContext, setOf("OK"))
        }
        validateObjectArray(
            requireArray(obj, "extendedTexts", context),
            "$context.extendedTexts",
        ) { item, itemContext ->
            requireExactFields(item, itemContext, "languageCode", "text", "parseStatus")
            requireString(item, "languageCode", itemContext)
            requireString(item, "text", itemContext)
            requireStringValue(item, "parseStatus", itemContext, setOf("OK"))
        }
        validateObjectArray(
            requireArray(obj, "extendedItems", context),
            "$context.extendedItems",
        ) { item, itemContext ->
            requireExactFields(item, itemContext, "languageCode", "description", "text")
            requireString(item, "languageCode", itemContext)
            requireString(item, "description", itemContext)
            requireString(item, "text", itemContext)
        }
        validateSingleTextObject(requireObject(obj, "component", context), "$context.component", "text")
        val audio = requireObject(obj, "audio", context)
        requireExactFields(audio, "$context.audio", "componentText", "language")
        requireString(audio, "componentText", "$context.audio")
        requireString(audio, "language", "$context.audio")
        validateGenres(requireObject(obj, "genres", context), "$context.genres")
        validateObjectArray(
            requireArray(obj, "eventGroups", context),
            "$context.eventGroups",
            ::validateEventGroup,
        )
        validateObjectArray(
            requireArray(obj, "componentGroups", context),
            "$context.componentGroups",
            ::validateComponentGroupDescriptor,
        )
        validateObjectArray(
            requireArray(obj, "linkage", context),
            "$context.linkage",
            ::validateLinkage,
        )
        validateFreeCaMode(requireObject(obj, "freeCaMode", context), "$context.freeCaMode")
        requireNullableObject(obj, "series", context)?.let {
            validateSeries(it, "$context.series")
        }
        validateObjectArray(
            requireArray(obj, "seriesCandidates", context),
            "$context.seriesCandidates",
            ::validateSeries,
        )
        requireNullableString(obj, "seriesCandidatesCanonicalJson", context)
        validateComponentEntries(requireObject(obj, "components", context), "$context.components")
        validateEventDiagnostics(requireObject(obj, "diagnostics", context), "$context.diagnostics")
        validateObjectArray(
            requireArray(obj, "parentalRatings", context),
            "$context.parentalRatings",
            ::validateParentalRating,
        )
    }

    private fun validateSingleTextObject(
        obj: JSONObject,
        context: String,
        key: String,
    ) {
        requireExactFields(obj, context, key)
        requireString(obj, key, context)
    }

    private fun validateGenres(
        obj: JSONObject,
        context: String,
    ) {
        requireExactFields(obj, context, "content", "genreSupplementText")
        validateObjectArray(requireArray(obj, "content", context), "$context.content") {
                item,
                itemContext,
            ->
            requireExactFields(
                item,
                itemContext,
                "level1",
                "level2",
                "userNibble",
                "aribName",
                "parseStatus",
            )
            requireInteger(item, "level1", itemContext, 0L..255L)
            requireInteger(item, "level2", itemContext, 0L..255L)
            requireInteger(item, "userNibble", itemContext, 0L..255L)
            requireString(item, "aribName", itemContext)
            requireStringValue(item, "parseStatus", itemContext, setOf("OK"))
        }
        requireString(obj, "genreSupplementText", context)
    }

    private fun validateEventGroup(
        obj: JSONObject,
        context: String,
    ) {
        requireExactFields(
            obj,
            context,
            "groupType",
            "events",
            "otherNetworkEvents",
            "privateDataHex",
            "parseStatus",
        )
        requireInteger(obj, "groupType", context, 0L..255L)
        validateObjectArray(requireArray(obj, "events", context), "$context.events") {
                item,
                itemContext,
            ->
            requireExactFields(item, itemContext, "serviceId", "eventId")
            requireInteger(item, "serviceId", itemContext, 0L..65_535L)
            requireInteger(item, "eventId", itemContext, 0L..65_535L)
        }
        validateObjectArray(
            requireArray(obj, "otherNetworkEvents", context),
            "$context.otherNetworkEvents",
        ) { item, itemContext ->
            requireExactFields(
                item,
                itemContext,
                "originalNetworkId",
                "transportStreamId",
                "serviceId",
                "eventId",
            )
            requireInteger(item, "originalNetworkId", itemContext, 0L..65_535L)
            requireInteger(item, "transportStreamId", itemContext, 0L..65_535L)
            requireInteger(item, "serviceId", itemContext, 0L..65_535L)
            requireInteger(item, "eventId", itemContext, 0L..65_535L)
        }
        requireHex(
            requireString(obj, "privateDataHex", context),
            "$context.privateDataHex",
        )
        requireStringValue(obj, "parseStatus", context, setOf("OK"))
    }

    private fun validateComponentGroupDescriptor(
        obj: JSONObject,
        context: String,
    ) {
        requireExactFields(obj, context, "componentGroupType", "groups", "parseStatus")
        requireInteger(obj, "componentGroupType", context, 0L..255L)
        validateObjectArray(requireArray(obj, "groups", context), "$context.groups") {
                item,
                itemContext,
            ->
            requireExactFields(item, itemContext, "componentGroupId", "componentTags")
            requireInteger(item, "componentGroupId", itemContext, 0L..255L)
            validateIntegerArray(
                requireArray(item, "componentTags", itemContext),
                "$itemContext.componentTags",
                0L..255L,
            )
        }
        requireStringValue(obj, "parseStatus", context, setOf("OK"))
    }

    private fun validateLinkage(
        obj: JSONObject,
        context: String,
    ) {
        requireExactFields(
            obj,
            context,
            "transportStreamId",
            "originalNetworkId",
            "serviceId",
            "linkageType",
            "privateDataPrefixHex",
            "parseStatus",
        )
        requireInteger(obj, "transportStreamId", context, 0L..65_535L)
        requireInteger(obj, "originalNetworkId", context, 0L..65_535L)
        requireInteger(obj, "serviceId", context, 0L..65_535L)
        requireInteger(obj, "linkageType", context, 0L..255L)
        requireHex(
            requireString(obj, "privateDataPrefixHex", context),
            "$context.privateDataPrefixHex",
        )
        requireStringValue(obj, "parseStatus", context, setOf("OK"))
    }

    private fun validateFreeCaMode(
        obj: JSONObject,
        context: String,
    ) {
        requireExactFields(obj, context, "raw", "scrambled", "parseStatus")
        requireInteger(obj, "raw", context, 0L..1L)
        requireBoolean(obj, "scrambled", context)
        requireStringValue(obj, "parseStatus", context, setOf("OK"))
    }

    private fun validateEventDiagnostics(
        obj: JSONObject,
        context: String,
    ) {
        requireExactFields(
            obj,
            context,
            "truncatedDescriptorLoop",
            "summary",
            "descriptorDiagnostics",
            "descriptorDiagnosticsCanonicalJson",
            "descriptorFactsCanonicalJson",
        )
        requireNullableObject(obj, "truncatedDescriptorLoop", context)?.let { loop ->
            requireExactFields(loop, "$context.truncatedDescriptorLoop", "declaredLength", "rawBytesHex", "parseStatus")
            requireInteger(
                loop,
                "declaredLength",
                "$context.truncatedDescriptorLoop",
                0L..Long.MAX_VALUE,
            )
            requireHex(
                requireString(loop, "rawBytesHex", "$context.truncatedDescriptorLoop"),
                "$context.truncatedDescriptorLoop.rawBytesHex",
            )
            requireStringValue(
                loop,
                "parseStatus",
                "$context.truncatedDescriptorLoop",
                setOf("TruncatedDescriptor"),
            )
        }
        requireString(obj, "summary", context)
        val rawDiagnostics = requireArray(obj, "descriptorDiagnostics", context)
        for (index in 0 until rawDiagnostics.length()) {
            if (rawDiagnostics.get(index) !is JSONObject) {
                jsonEncodingError("$context.descriptorDiagnostics[$index] の型がobjectではありません")
            }
        }
        requireString(obj, "descriptorDiagnosticsCanonicalJson", context)
        requireString(obj, "descriptorFactsCanonicalJson", context)
    }

    private fun validateParentalRating(
        obj: JSONObject,
        context: String,
    ) {
        requireExactFields(obj, context, "countryCode", "rawRatingByte", "parseStatus")
        val country = requireString(obj, "countryCode", context)
        if (country.length != 3) jsonEncodingError("$context.countryCode は3文字でなければなりません")
        requireInteger(obj, "rawRatingByte", context, 0L..255L)
        requireStringValue(obj, "parseStatus", context, setOf("OK"))
    }

    private fun validateEvent(
        obj: JSONObject,
        context: String,
    ) {
        requireExactFields(
            obj,
            context,
            "programKey",
            "eventId",
            "serviceKey",
            "stableIdentity",
            "timing",
            "title",
            "description",
            "extendedDescription",
            "eventScope",
            "source",
            "descriptors",
        )
        requireNullableObject(obj, "programKey", context)?.let {
            validateProgramKey(it, "$context.programKey")
        }
        requireInteger(obj, "eventId", context, 0L..65_535L)
        validateServiceKey(requireObject(obj, "serviceKey", context), "$context.serviceKey")
        requireNullableString(obj, "stableIdentity", context)
        validateTiming(requireObject(obj, "timing", context), "$context.timing")
        requireString(obj, "title", context)
        requireString(obj, "description", context)
        requireString(obj, "extendedDescription", context)
        requireStringValue(
            obj,
            "eventScope",
            context,
            setOf(
                "present_following_actual",
                "present_following_other",
                "schedule_actual",
                "schedule_other",
                "unknown",
            ),
        )
        validateProgramSource(requireObject(obj, "source", context), "$context.source")
        validateEventDescriptors(
            requireObject(obj, "descriptors", context),
            "$context.descriptors",
        )
    }

    private fun validateNativeTransaction(root: JSONObject) {
        requireExactFields(
            root,
            "SI snapshot",
            "schemaVersion",
            "collectionGeneration",
            "ingestSequence",
            "discoveryStage",
            "broadcastClock",
            "tableRequirements",
            "catCaMetadata",
            "malformedCaDescriptorDiagnostics",
            "malformedCaDescriptorCounts",
            "transportSemanticFacts",
            "events",
            "eitInstances",
            "serviceSemanticFacts",
            "parserDiagnostics",
        )
        requireInteger(
            root,
            "schemaVersion",
            "SI snapshot",
            SI_SNAPSHOT_SCHEMA_VERSION.toLong()..SI_SNAPSHOT_SCHEMA_VERSION.toLong(),
        )
        requireInteger(root, "collectionGeneration", "SI snapshot", 0L..Long.MAX_VALUE)
        requireInteger(root, "ingestSequence", "SI snapshot", 0L..Long.MAX_VALUE)
        requireInteger(
            root,
            "discoveryStage",
            "SI snapshot",
            0L..SiDiscoveryStage.COMPLETE.toLong(),
        )
        requireNullableObject(root, "broadcastClock", "SI snapshot")?.let {
            validateBroadcastClock(it, "SI snapshot.broadcastClock")
        }
        validateObjectArray(
            requireArray(root, "tableRequirements", "SI snapshot"),
            "SI snapshot.tableRequirements",
            ::validateTableRequirement,
        )
        validateObjectArray(
            requireArray(root, "catCaMetadata", "SI snapshot"),
            "SI snapshot.catCaMetadata",
            ::validateCaMetadata,
        )
        validateObjectArray(
            requireArray(root, "malformedCaDescriptorDiagnostics", "SI snapshot"),
            "SI snapshot.malformedCaDescriptorDiagnostics",
            ::validateMalformedCaDescriptor,
        )
        validateObjectArray(
            requireArray(root, "malformedCaDescriptorCounts", "SI snapshot"),
            "SI snapshot.malformedCaDescriptorCounts",
            ::validateMalformedCaCount,
        )
        validateObjectArray(
            requireArray(root, "transportSemanticFacts", "SI snapshot"),
            "SI snapshot.transportSemanticFacts",
            ::validateTransport,
        )
        validateObjectArray(
            requireArray(root, "events", "SI snapshot"),
            "SI snapshot.events",
            ::validateEvent,
        )
        validateObjectArray(
            requireArray(root, "eitInstances", "SI snapshot"),
            "SI snapshot.eitInstances",
            ::validateEitInstance,
        )
        validateObjectArray(
            requireArray(root, "serviceSemanticFacts", "SI snapshot"),
            "SI snapshot.serviceSemanticFacts",
            ::validateServiceSemanticFacts,
        )
        validateObjectArray(
            requireArray(root, "parserDiagnostics", "SI snapshot"),
            "SI snapshot.parserDiagnostics",
            ::validateParserDiagnostic,
        )
    }

    private fun parseStringArray(array: JSONArray?): List<String> =
        (0 until (array?.length() ?: 0)).mapNotNull { index ->
            array!!.optString(index).takeIf {
                it.isNotBlank()
            }
        }

    private fun parseTableRequirements(array: JSONArray?): List<TableRequirementStatus> =
        (0 until (array?.length() ?: 0)).mapNotNull { index ->
            val obj = array!!.optJSONObject(index) ?: return@mapNotNull null
            val component =
                obj.optString("component").takeIf { it.isNotBlank() }
                    ?: return@mapNotNull null
            TableRequirementStatus(
                component = component,
                originalNetworkId = optIntOrNull(obj, "originalNetworkId"),
                transportStreamId = optIntOrNull(obj, "transportStreamId"),
                serviceId = optIntOrNull(obj, "serviceId"),
                required = obj.optBoolean("required"),
                complete = obj.optBoolean("complete"),
            )
        }

    private fun parseSmdSemanticState(smd: JSONObject): SmdSemanticState =
        when (val value = smd.getString("semanticState")) {
            "SUPPORTED_BROADCAST" -> {
                SmdSemanticState.SUPPORTED_BROADCAST
            }

            "NON_BROADCAST" -> {
                SmdSemanticState.NON_BROADCAST
            }

            "UNDEFINED_BROADCAST_CLASS" -> {
                SmdSemanticState.UNDEFINED_BROADCAST_CLASS
            }

            "UNSUPPORTED_BROADCAST_SYSTEM" -> {
                SmdSemanticState.UNSUPPORTED_BROADCAST_SYSTEM
            }

            "UNDETERMINED_SMD" -> {
                SmdSemanticState.UNDETERMINED_SMD
            }

            else -> {
                throw NativeSiException(
                    "JSON_ENCODING",
                    "SI snapshotのsemanticStateが未知です: $value",
                )
            }
        }

    private fun parseSiParseStatus(value: String): SiParseStatus =
        when (value) {
            "OK" -> {
                SiParseStatus.OK
            }

            "MalformedLength" -> {
                SiParseStatus.MALFORMED_LENGTH
            }

            "TruncatedDescriptor" -> {
                SiParseStatus.TRUNCATED_DESCRIPTOR
            }

            "UnsupportedValue" -> {
                SiParseStatus.UNSUPPORTED_VALUE
            }

            "InvalidSequence" -> {
                SiParseStatus.INVALID_SEQUENCE
            }

            "UNRESOLVED" -> {
                SiParseStatus.UNRESOLVED
            }

            else -> {
                throw NativeSiException(
                    "JSON_ENCODING",
                    "SI snapshotのparseStatusが未知です: $value",
                )
            }
        }

    private fun parseEitTimingState(value: String): EitTimingState =
        when (value) {
            "DEFINED" -> {
                EitTimingState.DEFINED
            }

            "UNDEFINED_TIME" -> {
                EitTimingState.UNDEFINED_TIME
            }

            "BOTH_TIMING_UNDEFINED" -> {
                EitTimingState.BOTH_TIMING_UNDEFINED
            }

            "MALFORMED_TIMING" -> {
                EitTimingState.MALFORMED_TIMING
            }

            else -> {
                throw NativeSiException(
                    "JSON_ENCODING",
                    "SI snapshotのtiming stateが未知です: $value",
                )
            }
        }

    private fun parseElementaryStreamKind(value: String?): ElementaryStreamKind? =
        when (value) {
            null -> {
                null
            }

            "VIDEO" -> {
                ElementaryStreamKind.VIDEO
            }

            "AUDIO" -> {
                ElementaryStreamKind.AUDIO
            }

            else -> {
                throw NativeSiException(
                    "JSON_ENCODING",
                    "SI snapshotのcodecKindが未知です: $value",
                )
            }
        }

    private fun parseBroadcastSystem(smd: JSONObject): BroadcastSystem? {
        if (smd.isNull("broadcastSystem")) return null
        val value = smd.get("broadcastSystem")
        if (value !is String) {
            throw NativeSiException(
                "JSON_ENCODING",
                "SI snapshotのbroadcastSystem型が不正です",
            )
        }
        return when (value) {
            "ISDB_T" -> {
                BroadcastSystem.ISDB_T
            }

            "ISDB_S_BS" -> {
                BroadcastSystem.ISDB_S_BS
            }

            "ISDB_S_110CS" -> {
                BroadcastSystem.ISDB_S_110CS
            }

            else -> {
                throw NativeSiException(
                    "JSON_ENCODING",
                    "SI snapshotのbroadcastSystemが未知です: $value",
                )
            }
        }
    }

    private fun optIntOrNull(
        obj: JSONObject,
        key: String,
    ): Int? = if (obj.isNull(key)) null else obj.optInt(key)

    private fun optStringOrNull(
        obj: JSONObject,
        key: String,
    ): String? = if (obj.isNull(key)) null else obj.getString(key).takeIf { it.isNotBlank() }

    private fun optBoolOrNull(
        obj: JSONObject,
        key: String,
    ): Boolean? = if (obj.isNull(key)) null else obj.optBoolean(key)

    // この処理の規格値・ビット幅・単位換算・固定上限をリテラルのまま照合できる形に保つ。
    // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
    @Suppress("MagicNumber", "MaxLineLength")
    private fun hexToBytes(hex: String): ByteArray {
        if (hex.length % 2 != 0) return ByteArray(0)
        return ByteArray(hex.length / 2) { index -> hex.substring(index * 2, index * 2 + 2).toIntOrNull(16)?.toByte() ?: 0 }
    }

    private fun serviceKeyFrom(obj: JSONObject): ServiceKey? =
        ServiceKey.fromOrNull(
            originalNetworkId = obj.optInt("originalNetworkId", -1),
            transportStreamId = obj.optInt("transportStreamId", -1),
            serviceId = obj.optInt("serviceId", -1),
        )

    private fun parseStreams(array: JSONArray?): List<AribElementaryStream> =
        (0 until (array?.length() ?: 0)).mapNotNull { index ->
            val obj = array!!.optJSONObject(index) ?: return@mapNotNull null
            val pid = TsPid.fromOrNull(obj.optInt("elementaryPid", -1))
            val streamType = obj.optInt("streamType", -1)
            if (pid == null || streamType < 0) {
                null
            } else {
                AribElementaryStream(
                    elementaryPid = pid,
                    streamType = streamType,
                    componentTag = optIntOrNull(obj, "componentTag"),
                    componentType = optIntOrNull(obj, "componentType"),
                    streamContent = optIntOrNull(obj, "streamContent"),
                    languageCodes = parseStringArray(obj.optJSONArray("languageCodes")),
                    dataComponentId = optIntOrNull(obj, "dataComponentId"),
                    captionDmf = optIntOrNull(obj, "captionDmf"),
                    captionTiming = optIntOrNull(obj, "captionTiming"),
                    automaticPresentationOnReception = optBoolOrNull(obj, "automaticPresentationOnReception"),
                    isCaption = obj.optBoolean("isCaption"),
                    isSuperimpose = obj.optBoolean("isSuperimpose"),
                    codec = optStringOrNull(obj, "codec"),
                    codecKind = parseElementaryStreamKind(optStringOrNull(obj, "codecKind")),
                    codecFacts = parseCodecFacts(obj),
                )
            }
        }

    private fun parseCodecFacts(stream: JSONObject): AribCodecFacts {
        val facts = stream.optJSONObject("codecFacts") ?: return AribCodecFacts(resolved = false)
        val avc =
            facts.optJSONObject("avc")?.let {
                AribAvcSignaling(it.getInt("profileIdc"), it.getInt("constraintFlags"), it.getInt("levelIdc"))
            }
        val extension = facts.optJSONObject("audioExtension")
        val header =
            extension?.optJSONObject("header")?.let {
                AribAudioConfigHeader(
                    it.getInt("audioObjectType"),
                    it.getInt("samplingFrequency"),
                    it.getInt("channelConfiguration"),
                    optIntOrNull(it, "extensionSamplingFrequency"),
                    optIntOrNull(it, "coreAudioObjectType"),
                    optIntOrNull(it, "channelCount"),
                )
            }
        return AribCodecFacts(
            avc = avc,
            audioConfigHex = extension?.let { optStringOrNull(it, "audioSpecificConfigHex") },
            audioConfigHeader = header,
            rawDescriptorsHex = optStringOrNull(facts, "rawDescriptorsHex")?.takeIf { it.isNotEmpty() },
            profileLevel = optStringOrNull(stream, "codecProfileLevel"),
            resolved = stream.optBoolean("codecSignalingResolved", false),
        )
    }

    private fun parseCaDescriptors(array: JSONArray?): List<CaDescriptor> =
        (0 until (array?.length() ?: 0)).mapNotNull { index ->
            val obj = array!!.optJSONObject(index) ?: return@mapNotNull null
            val systemId = obj.optInt("caSystemId", -1)
            if (systemId < 0) {
                null
            } else {
                CaDescriptor(
                    caSystemId = systemId,
                    caPid = TsPid.fromOrNull(optIntOrNull(obj, "caPid")),
                    scope = if (obj.optString("scope") == "ES") CaDescriptorScope.ES else CaDescriptorScope.PROGRAM,
                    esPid = TsPid.fromOrNull(optIntOrNull(obj, "esPid")),
                    rawDescriptor = hexToBytes(obj.optString("rawDescriptorHex")),
                    privateData = hexToBytes(obj.optString("privateDataHex")),
                )
            }
        }

    // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
    @Suppress("MaxLineLength")
    private fun parseTransports(array: JSONArray?): List<AribTransport> =
        (0 until (array?.length() ?: 0)).mapNotNull { index ->
            val obj = array!!.optJSONObject(index) ?: return@mapNotNull null
            val onid = NetworkId16.fromOrNull(obj.optInt("originalNetworkId", -1))
            val tsid = TransportStreamId16.fromOrNull(obj.optInt("transportStreamId", -1))
            if (onid == null || tsid == null) {
                null
            } else {
                AribTransport(
                    originalNetwork = onid,
                    transportStream = tsid,
                    networkName = if (obj.isNull("networkName")) null else obj.getString("networkName"),
                    transportStreamName = if (obj.isNull("transportStreamName")) null else obj.getString("transportStreamName"),
                    sdtActual = obj.getBoolean("sdtActual"),
                    remoteControlKeyId = optIntOrNull(obj, "remoteControlKeyId"),
                )
            }
        }

    // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
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
                if (serviceComponents ==
                    null
                ) {
                    event.descriptors.components
                } else {
                    AribComponentProjectionPolicy.mergeEventAndServiceComponents(event.descriptors.components, serviceComponents)
                }
            event.copy(descriptors = event.descriptors.copy(components = components))
        }
    }

    // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
    @Suppress("MaxLineLength")
    private fun parseCaMetadataList(array: JSONArray?): List<CaMetadata> =
        (0 until (array?.length() ?: 0)).mapNotNull { index ->
            val obj = array!!.optJSONObject(index) ?: return@mapNotNull null
            val keyObj = obj.optJSONObject("serviceKey")
            val serviceKey = keyObj?.let { serviceKeyFrom(it) }
            val systemId = obj.optInt("caSystemId", -1)
            if (systemId < 0) {
                null
            } else {
                CaMetadata(
                    serviceKey = serviceKey,
                    caSystemId = systemId,
                    ecmPid = TsPid.fromOrNull(optIntOrNull(obj, "ecmPid")),
                    emmPid = TsPid.fromOrNull(optIntOrNull(obj, "emmPid")),
                    elementaryPid = TsPid.fromOrNull(optIntOrNull(obj, "elementaryPid")),
                    privateData = hexToBytes(obj.optString("privateDataHex")),
                    source = runCatching { CaMetadataSource.valueOf(obj.optString("source")) }.getOrDefault(CaMetadataSource.PROGRAM),
                )
            }
        }

    private fun parseMalformedCaDescriptorDiagnostics(array: JSONArray?): List<MalformedCaDescriptorDiagnostic> =
        (
            0 until
                (array?.length() ?: 0)
        ).mapNotNull { index ->
            val obj = array!!.optJSONObject(index) ?: return@mapNotNull null
            MalformedCaDescriptorDiagnostic(
                pid = TsPid.fromOrNull(obj.optInt("pid", -1)) ?: return@mapNotNull null,
                tableId = obj.optInt("tableId", -1),
                tableIdExtension = optIntOrNull(obj, "tableIdExtension"),
                service = ServiceId16.fromOrNull(optIntOrNull(obj, "serviceId")),
                elementaryPid = TsPid.fromOrNull(optIntOrNull(obj, "elementaryPid")),
                scope = obj.optString("scope"),
                offset = obj.optInt("offset", -1),
                declaredLength = obj.optInt("declaredLength", -1),
                actualRemainingLength = obj.optInt("actualRemainingLength", -1),
                reason = obj.optString("reason"),
                rawPrefixHex = obj.optString("rawPrefixHex"),
            ).takeIf { it.tableId >= 0 && it.offset >= 0 && it.reason.isNotBlank() }
        }

    private fun parseMalformedCaDescriptorCounts(array: JSONArray?): Map<ServiceId16, Int> =
        (0 until (array?.length() ?: 0))
            .mapNotNull { index ->
                val obj = array!!.optJSONObject(index) ?: return@mapNotNull null
                val serviceId = ServiceId16.fromOrNull(obj.optInt("serviceId", -1))
                val count = obj.optInt("count", 0)
                if (serviceId == null || count <= 0) null else serviceId to count
            }.toMap()

    // 同じ入力に対する分岐・項目写像を保持し、処理分割による状態の受け渡しを増やさない。
    // 同じ入力と資源寿命を扱う手順を一続きに確認できる形に保つ。
    // この処理の規格値・ビット幅・単位換算・固定上限をリテラルのまま照合できる形に保つ。
    // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
    @Suppress("CyclomaticComplexMethod", "LongMethod", "MagicNumber", "MaxLineLength")
    private fun parseEvents(array: JSONArray?): List<AribEvent> =
        (0 until (array?.length() ?: 0)).mapNotNull { index ->
            val obj = array!!.optJSONObject(index) ?: return@mapNotNull null
            val serviceKeyObj = obj.optJSONObject("serviceKey") ?: return@mapNotNull null
            val timingObj = obj.optJSONObject("timing") ?: return@mapNotNull null
            val key = serviceKeyFrom(serviceKeyObj) ?: return@mapNotNull null
            val eventId = obj.optInt("eventId", obj.optJSONObject("programKey")?.optInt("eventId", -1) ?: -1)
            val start = timingObj.optLong("startUtcMillis", 0L)
            val duration = timingObj.optLong("durationMillis", 0L)
            val descriptorsObj = obj.optJSONObject("descriptors") ?: JSONObject()
            val sourceObj = obj.optJSONObject("source") ?: JSONObject()
            val component = descriptorsObj.optJSONObject("component") ?: JSONObject()
            val audio = descriptorsObj.optJSONObject("audio") ?: JSONObject()
            val genres = descriptorsObj.optJSONObject("genres") ?: JSONObject()
            val freeCaMode = descriptorsObj.optJSONObject("freeCaMode") ?: JSONObject()
            val diagnostics = descriptorsObj.optJSONObject("diagnostics") ?: JSONObject()
            val series = descriptorsObj.optJSONObject("series")
            val shortEvents = parseShortEvents(descriptorsObj.optJSONArray("shortEvents"))
            val extendedTexts = parseExtendedTexts(descriptorsObj.optJSONArray("extendedTexts"))
            val extendedItems = parseExtendedItems(descriptorsObj.optJSONArray("extendedItems"))
            val selectedLanguage =
                shortEvents.firstOrNull()?.languageCode
                    ?: extendedTexts.firstOrNull()?.languageCode
                    ?: extendedItems.firstOrNull()?.languageCode
            val selectedShort = selectedLanguage?.let { language -> shortEvents.firstOrNull { it.languageCode == language } }
            val selectedExtended = selectedLanguage?.let { language -> extendedTexts.firstOrNull { it.languageCode == language } }
            val descriptorDiagnosticsCanonicalJson = diagnostics.optString("descriptorDiagnosticsCanonicalJson", "[]")
            if (eventId < 0) return@mapNotNull null
            AribEvent(
                serviceKey = key,
                stableIdentity = optStringOrNull(obj, "stableIdentity"),
                eventId = eventId,
                timingState = parseEitTimingState(timingObj.getString("state")),
                rawStartTimeHex = timingObj.optString("rawStartTimeHex"),
                rawDurationHex = timingObj.optString("rawDurationHex"),
                startTimeMillis = start,
                durationMillis = duration,
                title = if (shortEvents.isNotEmpty()) selectedShort?.title.orEmpty() else obj.optString("title"),
                description = if (shortEvents.isNotEmpty()) selectedShort?.text.orEmpty() else obj.optString("description"),
                extendedDescription =
                    if (extendedTexts.isNotEmpty()) {
                        selectedExtended?.text.orEmpty()
                    } else {
                        obj.optString(
                            "extendedDescription",
                        )
                    },
                eventScope = obj.optString("eventScope", "present_following"),
                source =
                    AribProgramSource(
                        pid = TsPid.fromOrNull(sourceObj.optInt("pid", 18)) ?: TsPid.EIT,
                        tableId = sourceObj.optInt("tableId", 0x4e),
                        version = sourceObj.optInt("version", 0),
                        sectionNumber = sourceObj.optInt("sectionNumber", 0),
                        lastSectionNumber = sourceObj.optInt("lastSectionNumber", 0),
                    ),
                descriptors =
                    AribEventDescriptors(
                        shortEvents = shortEvents,
                        extendedTexts = extendedTexts,
                        extendedItems = extendedItems,
                        componentText = optStringOrNull(component, "text"),
                        audioComponentText = optStringOrNull(audio, "componentText"),
                        contentGenres = parseContentGenres(genres.optJSONArray("content")),
                        genreSupplementText = optStringOrNull(genres, "genreSupplementText"),
                        eventGroups = parseEventGroups(descriptorsObj.optJSONArray("eventGroups")),
                        componentGroups = parseComponentGroups(descriptorsObj.optJSONArray("componentGroups")),
                        linkage = parseLinkage(descriptorsObj.optJSONArray("linkage")),
                        scrambled = if (freeCaMode.isNull("scrambled")) null else freeCaMode.optBoolean("scrambled"),
                        freeCaMode = parseFreeCaMode(freeCaMode),
                        series = parseSeries(series),
                        seriesCandidates = parseSeriesCandidates(descriptorsObj),
                        seriesCandidatesCanonicalJson = optStringOrNull(descriptorsObj, "seriesCandidatesCanonicalJson"),
                        parentalRatings = parseParentalRatings(descriptorsObj.optJSONArray("parentalRatings")),
                        components = parseComponents(descriptorsObj.optJSONObject("components")) ?: AribComponents(),
                        diagnostics =
                            AribEventDiagnostics(
                                summary = diagnostics.optString("summary"),
                                descriptorDiagnosticsCanonicalJson = descriptorDiagnosticsCanonicalJson,
                                descriptorFactsCanonicalJson = optStringOrNull(diagnostics, "descriptorFactsCanonicalJson"),
                                textDiagnostics = parseTextDiagnosticSummary(diagnostics.optString("summary")),
                                truncatedDescriptorLoop =
                                    diagnostics.optJSONObject("truncatedDescriptorLoop")?.let { loop ->
                                        AribTruncatedDescriptorLoop(
                                            loop.getInt("declaredLength"),
                                            loop.getString("rawBytesHex"),
                                            parseSiParseStatus(loop.getString("parseStatus")),
                                        )
                                    },
                            ),
                    ),
            )
        }

    // この処理の規格値・ビット幅・単位換算・固定上限をリテラルのまま照合できる形に保つ。
    @Suppress("MagicNumber")
    private fun parseShortEvents(array: JSONArray?): List<AribShortEventText> =
        (0 until (array?.length() ?: 0))
            .mapNotNull { index ->
                val obj = array!!.optJSONObject(index) ?: return@mapNotNull null
                val languageCode = obj.optString("languageCode")
                if (languageCode.length != 3) {
                    null
                } else {
                    AribShortEventText(
                        languageCode = languageCode,
                        title = obj.optString("title"),
                        text = obj.optString("text"),
                        parseStatus = parseSiParseStatus(obj.getString("parseStatus")),
                    )
                }
            }.filter { it.parseStatus == SiParseStatus.OK }
            .distinctBy { it.languageCode }

    // この処理の規格値・ビット幅・単位換算・固定上限をリテラルのまま照合できる形に保つ。
    @Suppress("MagicNumber")
    private fun parseExtendedTexts(array: JSONArray?): List<AribExtendedEventText> =
        (0 until (array?.length() ?: 0))
            .mapNotNull { index ->
                val obj = array!!.optJSONObject(index) ?: return@mapNotNull null
                val languageCode = obj.optString("languageCode")
                if (languageCode.length != 3) {
                    null
                } else {
                    AribExtendedEventText(
                        languageCode = languageCode,
                        text = obj.optString("text"),
                        parseStatus = parseSiParseStatus(obj.getString("parseStatus")),
                    )
                }
            }.filter { it.parseStatus == SiParseStatus.OK }
            .distinctBy { it.languageCode }

    // この処理の規格値・ビット幅・単位換算・固定上限をリテラルのまま照合できる形に保つ。
    @Suppress("MagicNumber")
    private fun parseExtendedItems(array: JSONArray?): List<AribExtendedItem> =
        (0 until (array?.length() ?: 0)).mapNotNull { index ->
            val obj = array!!.optJSONObject(index) ?: return@mapNotNull null
            val languageCode = obj.optString("languageCode")
            if (languageCode.length != 3) {
                null
            } else {
                AribExtendedItem(
                    languageCode = languageCode,
                    itemDescription = obj.optString("description"),
                    itemText = obj.optString("text"),
                )
            }
        }

    private fun parseParentalRatings(array: JSONArray?): List<AribParentalRating> =
        (0 until (array?.length() ?: 0)).mapNotNull { index ->
            val obj = array!!.optJSONObject(index) ?: return@mapNotNull null
            val country = obj.optString("countryCode")
            val raw = obj.optInt("rawRatingByte", -1)
            if (country.isBlank() || raw < 0) {
                null
            } else {
                AribParentalRating(
                    countryCode = country,
                    rawRatingByte = raw,
                    parseStatus = parseSiParseStatus(obj.getString("parseStatus")),
                )
            }
        }

    private fun parseContentGenres(array: JSONArray?): List<AribContentGenre> =
        (0 until (array?.length() ?: 0)).mapNotNull { index ->
            val obj = array!!.optJSONObject(index) ?: return@mapNotNull null
            val level1 = obj.optInt("level1", -1)
            val level2 = obj.optInt("level2", -1)
            if (level1 < 0 || level2 < 0) {
                null
            } else {
                AribContentGenre(
                    level1 = level1,
                    level2 = level2,
                    userNibble = obj.optInt("userNibble", 0),
                    aribName = obj.optString("aribName"),
                    parseStatus = parseSiParseStatus(obj.getString("parseStatus")),
                )
            }
        }

    // この処理の規格値・ビット幅・単位換算・固定上限をリテラルのまま照合できる形に保つ。
    @Suppress("MagicNumber")
    private fun parseEventGroups(array: JSONArray?): List<AribEventGroup> =
        (0 until (array?.length() ?: 0)).mapNotNull { index ->
            val obj = array!!.optJSONObject(index) ?: return@mapNotNull null
            val groupType = obj.optInt("groupType", -1)
            if (groupType !in 0..15) return@mapNotNull null
            val events = parseEventGroupReferences(obj.optJSONArray("events"))
            val otherNetworkEvents = parseOtherNetworkEventGroupReferences(obj.optJSONArray("otherNetworkEvents"))
            val privateDataHex = obj.optString("privateDataHex", "")
            if (!isEvenHex(privateDataHex)) return@mapNotNull null
            if (groupType == 4 || groupType == 5) {
                if (privateDataHex.isNotEmpty()) return@mapNotNull null
            } else if (otherNetworkEvents.isNotEmpty()) {
                return@mapNotNull null
            }
            AribEventGroup(
                groupType = groupType,
                events = events,
                otherNetworkEvents = otherNetworkEvents,
                privateDataHex = privateDataHex,
                parseStatus = parseSiParseStatus(obj.getString("parseStatus")),
            )
        }

    // この処理の規格値・ビット幅・単位換算・固定上限をリテラルのまま照合できる形に保つ。
    @Suppress("MagicNumber")
    private fun parseComponentGroups(array: JSONArray?): List<AribComponentGroupDescriptor> =
        (0 until (array?.length() ?: 0)).mapNotNull { index ->
            val obj = array!!.optJSONObject(index) ?: return@mapNotNull null
            val type = obj.optInt("componentGroupType", -1)
            if (type !in 0..7) return@mapNotNull null
            val groupsArray = obj.optJSONArray("groups")
            val groups =
                (0 until (groupsArray?.length() ?: 0)).mapNotNull { groupIndex ->
                    val group = groupsArray!!.optJSONObject(groupIndex) ?: return@mapNotNull null
                    val id = group.optInt("componentGroupId", -1)
                    if (id !in 0..15) return@mapNotNull null
                    val tagsArray = group.optJSONArray("componentTags")
                    val tags =
                        (0 until (tagsArray?.length() ?: 0)).mapNotNull { tagIndex ->
                            tagsArray!!.optInt(tagIndex, -1).takeIf { it in 0..0xff }
                        }
                    AribComponentGroup(componentGroupId = id, componentTags = tags)
                }
            AribComponentGroupDescriptor(
                componentGroupType = type,
                groups = groups,
                parseStatus = parseSiParseStatus(obj.getString("parseStatus")),
            )
        }

    // この処理の規格値・ビット幅・単位換算・固定上限をリテラルのまま照合できる形に保つ。
    @Suppress("MagicNumber")
    private fun parseEventGroupReferences(array: JSONArray?): List<AribEventGroupReference> =
        (0 until (array?.length() ?: 0)).mapNotNull { index ->
            val obj = array!!.optJSONObject(index) ?: return@mapNotNull null
            val service = ServiceId16.fromOrNull(obj.optInt("serviceId", -1)) ?: return@mapNotNull null
            val eventId = obj.optInt("eventId", -1).takeIf { it in 0..0xffff } ?: return@mapNotNull null
            AribEventGroupReference(service = service, eventId = eventId)
        }

    // この処理の規格値・ビット幅・単位換算・固定上限をリテラルのまま照合できる形に保つ。
    // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
    @Suppress("MagicNumber", "MaxLineLength")
    private fun parseOtherNetworkEventGroupReferences(array: JSONArray?): List<AribOtherNetworkEventGroupReference> =
        (0 until (array?.length() ?: 0)).mapNotNull { index ->
            val obj = array!!.optJSONObject(index) ?: return@mapNotNull null
            val originalNetwork = NetworkId16.fromOrNull(obj.optInt("originalNetworkId", -1)) ?: return@mapNotNull null
            val transportStream = TransportStreamId16.fromOrNull(obj.optInt("transportStreamId", -1)) ?: return@mapNotNull null
            val service = ServiceId16.fromOrNull(obj.optInt("serviceId", -1)) ?: return@mapNotNull null
            val eventId = obj.optInt("eventId", -1).takeIf { it in 0..0xffff } ?: return@mapNotNull null
            AribOtherNetworkEventGroupReference(
                originalNetwork = originalNetwork,
                transportStream = transportStream,
                service = service,
                eventId = eventId,
            )
        }

    // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
    @Suppress("MaxLineLength")
    private fun isEvenHex(value: String): Boolean = value.length % 2 == 0 && value.all { it.isDigit() || it.lowercaseChar() in 'a'..'f' }

    private fun parseLinkage(array: JSONArray?): List<AribLinkage> =
        (0 until (array?.length() ?: 0)).mapNotNull { index ->
            val obj = array!!.optJSONObject(index) ?: return@mapNotNull null
            val key =
                ServiceKey.fromOrNull(
                    originalNetworkId = obj.optInt("originalNetworkId", -1),
                    transportStreamId = obj.optInt("transportStreamId", -1),
                    serviceId = obj.optInt("serviceId", -1),
                ) ?: return@mapNotNull null
            AribLinkage(
                linkageType = obj.optInt("linkageType", -1),
                serviceKey = key,
                privateDataPrefixHex = obj.optString("privateDataPrefixHex", ""),
                parseStatus = parseSiParseStatus(obj.getString("parseStatus")),
            ).takeIf { it.linkageType >= 0 }
        }

    private fun parseFreeCaMode(obj: JSONObject): AribFreeCaMode? =
        if (obj.length() == 0) {
            null
        } else {
            AribFreeCaMode(
                raw = optIntOrNull(obj, "raw"),
                scrambled = optBoolOrNull(obj, "scrambled"),
                parseStatus = parseSiParseStatus(obj.getString("parseStatus")),
            )
        }

    private fun parseSeries(obj: JSONObject?): AribSeries? =
        obj?.let {
            AribSeries(
                seriesId = optIntOrNull(it, "seriesId"),
                repeatLabel = it.optInt("repeatLabel", 0),
                programPattern = it.optInt("programPattern", 0),
                expireDateValid = it.optBoolean("expireDateValid"),
                expireDate = optIntOrNull(it, "expireDate"),
                episodeNumber = optIntOrNull(it, "episodeNumber"),
                lastEpisodeNumber = optIntOrNull(it, "lastEpisodeNumber"),
                name = optStringOrNull(it, "name"),
                parseStatus = parseSiParseStatus(it.getString("parseStatus")),
            )
        }

    private fun parseSeriesCandidates(descriptors: JSONObject): List<AribSeries> {
        val array = requiredSeriesCandidatesArray(descriptors)
        return (0 until array.length()).map { index ->
            parseSeriesCandidate(requiredSeriesCandidateObject(array, index), index)
        }
    }

    private fun requiredSeriesCandidatesArray(descriptors: JSONObject): JSONArray {
        val value = if (descriptors.has("seriesCandidates")) descriptors.get("seriesCandidates") else null
        return value as? JSONArray
            ?: throw NativeSiException(
                "JSON_ENCODING",
                if (value == null) {
                    "SI snapshotのseriesCandidatesが欠落しています"
                } else {
                    "SI snapshotのseriesCandidates型がarrayではありません"
                },
            )
    }

    private fun requiredSeriesCandidateObject(
        array: JSONArray,
        index: Int,
    ): JSONObject =
        array.optJSONObject(index)
            ?: throw seriesCandidateEncodingError(index, "要素型がobjectではありません")

    private fun parseSeriesCandidate(
        candidate: JSONObject,
        index: Int,
    ): AribSeries {
        val validationErrors = seriesCandidateValidationErrors(candidate)
        if (validationErrors.isNotEmpty()) {
            throw seriesCandidateEncodingError(index, validationErrors.joinToString(", "))
        }
        return AribSeries(
            seriesId = candidate.getInt("seriesId"),
            repeatLabel = candidate.getInt("repeatLabel"),
            programPattern = candidate.getInt("programPattern"),
            expireDateValid = candidate.getBoolean("expireDateValid"),
            expireDate = if (candidate.isNull("expireDate")) null else candidate.getInt("expireDate"),
            episodeNumber = candidate.getInt("episodeNumber"),
            lastEpisodeNumber = candidate.getInt("lastEpisodeNumber"),
            name = if (candidate.isNull("name")) null else candidate.getString("name"),
            parseStatus = parseSiParseStatus(candidate.getString("parseStatus")),
        )
    }

    private fun seriesCandidateValidationErrors(candidate: JSONObject): List<String> =
        seriesCandidateNumericValidationErrors(candidate) +
            seriesCandidateMetadataValidationErrors(candidate)

    private fun seriesCandidateNumericValidationErrors(candidate: JSONObject): List<String> =
        listOfNotNull(
            integerFieldValidationError(candidate, "seriesId", 0L..SERIES_U16_MAX),
            integerFieldValidationError(candidate, "repeatLabel", 0L..SERIES_REPEAT_LABEL_MAX),
            integerFieldValidationError(candidate, "programPattern", 0L..SERIES_PROGRAM_PATTERN_MAX),
            integerFieldValidationError(candidate, "episodeNumber", 0L..SERIES_EPISODE_MAX),
            integerFieldValidationError(candidate, "lastEpisodeNumber", 0L..SERIES_EPISODE_MAX),
            expireDateValidationError(candidate),
        )

    private fun integerFieldValidationError(
        candidate: JSONObject,
        key: String,
        range: LongRange,
    ): String? {
        val missing = !candidate.has(key) || candidate.isNull(key)
        val number = if (missing) null else candidate.get(key) as? Number
        return when {
            missing -> {
                "$key が欠落しています"
            }

            number == null -> {
                "$key の型が数値ではありません"
            }

            !isIntegralNumberInRange(number, range) -> {
                "$key が整数値域 ${range.first}..${range.last} の外です"
            }

            else -> {
                null
            }
        }
    }

    private fun isIntegralNumberInRange(
        number: Number,
        range: LongRange,
    ): Boolean {
        val value = number.toDouble()
        val integral = value.isFinite() && value % 1.0 == 0.0
        return integral && value >= range.first && value <= range.last
    }

    private fun expireDateValidationError(candidate: JSONObject): String? {
        val validValue = if (candidate.has("expireDateValid")) candidate.get("expireDateValid") else null
        val hasExpireDate = candidate.has("expireDate")
        val expireDateIsNull = hasExpireDate && candidate.isNull("expireDate")
        return when {
            validValue !is Boolean -> "expireDateValid の型が不正です"
            !hasExpireDate -> "expireDate が欠落しています"
            validValue && expireDateIsNull -> "expireDateValid=true なのにexpireDateがnullです"
            !validValue && !expireDateIsNull -> "expireDateValid=false なのにexpireDateが存在します"
            validValue -> integerFieldValidationError(candidate, "expireDate", 0L..SERIES_U16_MAX)
            else -> null
        }
    }

    private fun seriesCandidateMetadataValidationErrors(candidate: JSONObject): List<String> =
        buildList {
            if (!isNullableStringField(candidate, "name")) {
                add("name の型が不正です")
            }
            if (!candidate.has("parseStatus") || candidate.get("parseStatus") != "OK") {
                add("parseStatus はOKでなければなりません")
            }
        }

    private fun isNullableStringField(
        candidate: JSONObject,
        key: String,
    ): Boolean =
        candidate.has(key) &&
            (candidate.isNull(key) || candidate.get(key) is String)

    private fun seriesCandidateEncodingError(
        index: Int,
        detail: String,
    ): NativeSiException =
        NativeSiException(
            "JSON_ENCODING",
            "SI snapshotのseriesCandidates[$index]が不正です: $detail",
        )

    private fun parseComponents(obj: JSONObject?): AribComponents? =
        obj?.let {
            AribComponents(
                video = parseComponentEntries(it.optJSONArray("video")),
                audio = parseComponentEntries(it.optJSONArray("audio")),
                subtitle = parseComponentEntries(it.optJSONArray("subtitle")),
                data = parseComponentEntries(it.optJSONArray("data")),
            )
        }

    private fun parseComponentEntries(array: JSONArray?): List<AribComponentEntry> =
        (0 until (array?.length() ?: 0)).mapNotNull { index ->
            val obj = array!!.optJSONObject(index) ?: return@mapNotNull null
            val pid = TsPid.fromOrNull(optIntOrNull(obj, "esPid"))
            if (pid == null && optIntOrNull(obj, "componentTag") == null) {
                null
            } else {
                AribComponentEntry(
                    esPid = pid,
                    streamType = optIntOrNull(obj, "streamType"),
                    streamContent = optIntOrNull(obj, "streamContent"),
                    componentTag = optIntOrNull(obj, "componentTag"),
                    componentType = optIntOrNull(obj, "componentType"),
                    codec = optStringOrNull(obj, "codec"),
                    language = optStringOrNull(obj, "language"),
                    secondLanguage = optStringOrNull(obj, "secondLanguage"),
                    channelConfiguration = optStringOrNull(obj, "channelConfiguration"),
                    simulcastGroupTag = optIntOrNull(obj, "simulcastGroupTag"),
                    samplingRate = optIntOrNull(obj, "samplingRate"),
                    samplingInfo = optStringOrNull(obj, "samplingInfo"),
                    text = optStringOrNull(obj, "text"),
                    sourceDescriptor = optStringOrNull(obj, "sourceDescriptor"),
                    resolution = optStringOrNull(obj, "resolution"),
                    scan = optStringOrNull(obj, "scan"),
                    aspect = optStringOrNull(obj, "aspect"),
                    profileLevel = optStringOrNull(obj, "profileLevel"),
                    dataComponentId = optIntOrNull(obj, "dataComponentId"),
                    captionServiceKind = optStringOrNull(obj, "captionServiceKind"),
                    main = optBoolOrNull(obj, "main"),
                    multiLingual = optBoolOrNull(obj, "multiLingual"),
                    qualityIndicator = optIntOrNull(obj, "qualityIndicator"),
                    parseStatus = parseSiParseStatus(obj.getString("parseStatus")),
                    channelCount = optIntOrNull(obj, "channelCount"),
                    sampleRateHz = optIntOrNull(obj, "sampleRateHz"),
                    audioDescription = optBoolOrNull(obj, "audioDescription"),
                    hardOfHearing = optBoolOrNull(obj, "hardOfHearing"),
                    dualMono = optBoolOrNull(obj, "dualMono"),
                )
            }
        }

    private fun parseEitInstanceStates(array: JSONArray?): List<EitInstanceState> =
        (0 until (array?.length() ?: 0)).map { index ->
            val obj = array!!.getJSONObject(index)

            fun numbers(name: String): List<Int> {
                val values = obj.getJSONArray(name)
                return (0 until values.length()).map(values::getInt)
            }
            EitInstanceState(
                serviceKey = requireNotNull(serviceKeyFrom(obj)),
                tableId = obj.getInt("tableId"),
                version = obj.getInt("version"),
                currentNextIndicator = obj.getBoolean("currentNextIndicator"),
                lastSectionNumber = obj.getInt("lastSectionNumber"),
                receivedSections = numbers("receivedSections"),
                missingSections = numbers("missingSections"),
                safeSections = numbers("safeSections"),
                complete = obj.getBoolean("complete"),
                inconsistent = obj.getBoolean("inconsistent"),
            )
        }

    // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
    @Suppress("MaxLineLength")
    private fun parseServiceSemanticFacts(array: JSONArray?): List<ServiceSemanticFacts> =
        (0 until (array?.length() ?: 0)).mapNotNull { index ->
            val obj = array!!.optJSONObject(index) ?: return@mapNotNull null
            val key = serviceKeyFrom(obj) ?: return@mapNotNull null
            val smd = obj.optJSONObject("smd") ?: JSONObject()
            ServiceSemanticFacts(
                name = if (obj.isNull("name")) null else obj.getString("name"),
                providerName = if (obj.isNull("providerName")) null else obj.getString("providerName"),
                pmtPid = TsPid.fromOrNull(optIntOrNull(obj, "pmtPid")),
                pcrPid = TsPid.fromOrNull(optIntOrNull(obj, "pcrPid")),
                serviceScopedCaDescriptors = parseCaDescriptors(obj.optJSONArray("serviceScopedCaDescriptors")),
                serviceKey = key,
                serviceType = optIntOrNull(obj, "serviceType"),
                pmtPidResolved = obj.optBoolean("pmtPidResolved"),
                pmtParsed = obj.optBoolean("pmtParsed"),
                pcrPidResolved = obj.optBoolean("pcrPidResolved"),
                elementaryStreams = parseStreams(obj.optJSONArray("elementaryStreams")),
                requiresCas = obj.optBoolean("requiresCas"),
                caDescriptorsResolved = obj.optBoolean("caDescriptorsResolved"),
                casFactsCanonicalJson = if (obj.isNull("casFactsCanonicalJson")) null else obj.getString("casFactsCanonicalJson"),
                freeCaMode = optBoolOrNull(obj, "freeCaMode"),
                smd =
                    SmdSemanticFacts(
                        descriptorPresent = smd.optBoolean("descriptorPresent"),
                        syntaxValid = smd.optBoolean("syntaxValid"),
                        systemManagementId = optIntOrNull(smd, "systemManagementId"),
                        broadcastingFlag = optIntOrNull(smd, "broadcastingFlag"),
                        broadcastingIdentifier = optIntOrNull(smd, "broadcastingIdentifier"),
                        broadcastSystem = parseBroadcastSystem(smd),
                        additionalBroadcastingIdentification = optIntOrNull(smd, "additionalBroadcastingIdentification"),
                        additionalIdentificationInfoHex = smd.optString("additionalIdentificationInfoHex"),
                        semanticState = parseSmdSemanticState(smd),
                        diagnostic = optStringOrNull(smd, "diagnostic"),
                    ),
                missingComponents = parseStringArray(obj.optJSONArray("missingComponents")),
                semanticDiagnostics = parseStringArray(obj.optJSONArray("semanticDiagnostics")),
            )
        }

    private fun parseParserDiagnostics(array: JSONArray?): List<ParserDiagnostic> =
        (0 until (array?.length() ?: 0)).mapNotNull { index ->
            val obj = array!!.optJSONObject(index) ?: return@mapNotNull null
            val code = obj.optString("code").takeIf { it.isNotBlank() } ?: return@mapNotNull null
            ParserDiagnostic(
                code = code,
                message = obj.optString("message"),
                severity = optStringOrNull(obj, "severity"),
            )
        }

    private fun parseTextDiagnosticSummary(raw: String): List<String> =
        raw
            .split(' ', '\n')
            .filter { it.contains("unknownCount=") || it.contains("component=") || it.contains("audio=") }

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

    private external fun nativeSnapshotBulkJson(handle: Long): String?

    private external fun nativeSnapshotPmtPidsForSectionFiltersJson(handle: Long): String?

    private external fun nativeDecodeAribString(bytes: ByteArray): String?

    private external fun nativeDecodeAribStringDiagnosticSummary(bytes: ByteArray): String?

    companion object {
        private const val SI_SNAPSHOT_SCHEMA_VERSION = 2

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
