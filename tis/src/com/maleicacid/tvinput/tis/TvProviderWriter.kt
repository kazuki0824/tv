package com.maleicacid.tvinput.tis

import android.content.ContentProviderOperation
import android.content.ContentUris
import android.content.ContentValues
import android.content.Context
import android.media.tv.TvContract
import android.net.Uri
import android.util.Log
import com.maleicacid.tvinput.aribsi.ProviderDataBridge
import com.maleicacid.tvinput.common.LogTags
import com.maleicacid.tvinput.common.ServiceKey
import com.maleicacid.tvinput.db.ChannelRecord
import com.maleicacid.tvinput.db.ProgramRecord
import java.security.MessageDigest

// 同じ状態・境界を扱う操作群を一つの所有者に保つ。
@Suppress("TooManyFunctions")
class TvProviderWriter private constructor(
    private val inputId: String,
    private val channelStore: ChannelStore,
) {
    constructor(context: Context, inputId: String) : this(inputId, AndroidTvProviderChannelStore(context, inputId))

    constructor(
        inputId: String,
        channelStore: ChannelStore,
        @Suppress("UNUSED_PARAMETER") testOnly: Boolean,
    ) : this(inputId, channelStore)

    data class Diagnostic(
        val serviceKey: ServiceKey?,
        val operation: String,
        val message: String,
    )

    data class GenreReadbackDiagnostic(
        val serviceKey: ServiceKey,
        val programId: Long,
        val directlySetCanonicalGenre: String?,
        val readBackCanonicalGenre: String?,
        val readFailure: String?,
    )

    data class UpsertResult(
        val inserted: Int,
        val updated: Int,
        val failures: List<Diagnostic>,
        val deleted: Int = 0,
        val succeededServiceKeys: Set<ServiceKey> = emptySet(),
        val genreDiagnostics: List<GenreReadbackDiagnostic> = emptyList(),
    )

    data class ExistingProgramIndexEntry(
        val programId: Long,
        val startTimeMillis: Long,
        val endTimeMillis: Long,
    )

    data class ProgramUpsertRequest(
        val existingProgramId: Long?,
        val values: ContentValues,
    )

    data class ProgramUpsertOutcome(
        val programId: Long?,
        val updated: Boolean,
    )

    interface ChannelStore {
        fun findExistingChannelId(key: ServiceKey): Result<Long?>

        fun indexExistingChannelIds(keys: Set<ServiceKey>): Result<Map<ServiceKey, Long>> =
            runCatching {
                keys
                    .mapNotNull { key ->
                        findExistingChannelId(key)
                            .getOrThrow()
                            ?.let { id -> key to id }
                    }.toMap()
            }

        fun insertChannel(values: ContentValues): Result<Long?>

        fun updateChannel(
            channelId: Long,
            values: ContentValues,
        ): Result<Int>

        fun indexExistingProgramsForWindow(
            channelId: Long,
            windowStartMs: Long,
            windowEndMs: Long,
        ): Result<Map<String, Long>> = Result.success(emptyMap())

        fun indexExistingProgramEntriesForWindow(
            channelId: Long,
            windowStartMs: Long,
            windowEndMs: Long,
        ): Result<Map<String, List<ExistingProgramIndexEntry>>> =
            Result.failure(
                UnsupportedOperationException(
                    "この store は時刻付き program index に対応しません",
                ),
            )

        /**
         * channel 全体の既存 Program row を stable programKey で引ける形で返す。
         * EPG 更新区間 より意図的に広く取得し、start / end time が現在 window の外へ
         * 移動した event も、duplicate insert ではなく stable ONID / TSID / SID / event identity で更新する。
         */
        fun indexExistingProgramsForService(channelId: Long): Result<Map<String, Long>> =
            indexExistingProgramsForWindow(channelId, Long.MIN_VALUE, Long.MAX_VALUE)

        fun upsertProgramsBatch(requests: List<ProgramUpsertRequest>): Result<List<ProgramUpsertOutcome>> =
            Result.failure(UnsupportedOperationException("この store は program batch書込みに対応しません"))

        fun readCanonicalGenres(
            channelId: Long,
            programIds: Set<Long>,
        ): Result<Map<Long, String?>> = Result.failure(UnsupportedOperationException("この store はジャンル一括読戻しに対応しません"))

        fun deleteObsoletePrograms(
            channelId: Long,
            validProgramKeys: Set<String>,
            windowStartMs: Long,
            windowEndMs: Long,
        ): Result<Int> = Result.failure(UnsupportedOperationException("この store はobsolete program削除に対応しません"))

        fun listExistingChannels(): Result<List<ChannelRecord>> = Result.success(emptyList())
    }

    // 同じ入力と資源寿命を扱う手順を一続きに確認できる形に保つ。
    // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
    @Suppress("LongMethod", "MaxLineLength")
    fun upsertChannels(channels: List<ChannelRecord>): UpsertResult {
        var inserted = 0
        var updated = 0
        val failures = mutableListOf<Diagnostic>()
        val existingIds =
            channelStore.indexExistingChannelIds(channels.mapTo(linkedSetOf()) { it.serviceKey }).getOrElse { error ->
                return UpsertResult(
                    inserted = 0,
                    updated = 0,
                    failures = channels.map { Diagnostic(it.serviceKey, "query", error.message.orEmpty()) },
                )
            }
        channels.forEach { channel ->
            val validation = validate(channel)
            if (validation != null) {
                failures += validation
                return@forEach
            }
            val providerData =
                when (val built = ProviderDataBridge.buildChannelProviderData(channel)) {
                    is ProviderDataBridge.Success -> {
                        built.bytes
                    }

                    is ProviderDataBridge.Failure -> {
                        failures += Diagnostic(channel.serviceKey, "provider-data", "${built.errorCode}: ${built.errorMessage}")
                        return@forEach
                    }
                }
            val values = channelValues(channel, providerData)
            val existingId = existingIds[channel.serviceKey]
            if (existingId == null) {
                val insertedIdResult = channelStore.insertChannel(values)
                if (insertedIdResult.isFailure) {
                    failures +=
                        Diagnostic(channel.serviceKey, "insert", insertedIdResult.exceptionOrNull()?.message.orEmpty())
                    return@forEach
                }
                if (insertedIdResult.getOrNull() ==
                    null
                ) {
                    failures += Diagnostic(channel.serviceKey, "insert", "provider が null URI を返しました")
                } else {
                    inserted++
                }
            } else {
                values.remove(TvContract.Channels.COLUMN_TYPE)
                val updateResult = channelStore.updateChannel(existingId, values)
                if (updateResult.isFailure) {
                    failures +=
                        Diagnostic(channel.serviceKey, "update", updateResult.exceptionOrNull()?.message.orEmpty())
                    return@forEach
                }
                if (updateResult.getOrNull() == null ||
                    updateResult.getOrNull()!! <= 0
                ) {
                    failures += Diagnostic(channel.serviceKey, "update", "provider 更新対象行なし id=$existingId")
                } else {
                    updated++
                }
            }
        }
        Log.i(LogTags.TIS, "channel登録結果 inputId=$inputId inserted=$inserted updated=$updated failures=${failures.size}")
        return UpsertResult(inserted, updated, failures)
    }

    fun upsertPrograms(programs: List<ProgramRecord>): UpsertResult =
        upsertProgramsForWindows(
            programs = programs,
            windows =
                programs.groupBy { it.serviceKey }.map { (key, values) ->
                    ProgramPublishCoordinator.EpgUpdateWindow(
                        serviceKey = key,
                        windowStartMs = values.minOf { it.startTimeMillis },
                        windowEndMs =
                            values.mapNotNull(::checkedProgramEndTimeMillis).maxOrNull()
                                ?: values.maxOf { it.startTimeMillis },
                        validProgramKeys = values.map { programIdentity(it) }.toSet(),
                        deletionAuthoritative = false,
                    )
                },
        )

    internal data class PreparedServicePrograms(
        val serviceKey: ServiceKey,
        val channelId: Long,
        val programs: List<Pair<ProgramRecord, ContentValues>>,
        val windows: List<ProgramPublishCoordinator.EpgUpdateWindow>,
    )

    internal class PreparedProgramPublication(
        val services: List<PreparedServicePrograms>,
        val failures: List<Diagnostic>,
    ) {
        // 再生成や仮channelIdを使わず、このまま書く最終ContentValuesから計算する。
        val fingerprint: String? = if (failures.isEmpty()) publicationFingerprint(services) else null
    }

    // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
    @Suppress("MaxLineLength")
    internal fun prepareProgramPublication(
        programs: List<ProgramRecord>,
        windows: List<ProgramPublishCoordinator.EpgUpdateWindow>,
        verifiedEmptyServiceKeys: Set<ServiceKey> = emptySet(),
    ): PreparedProgramPublication {
        val failures = mutableListOf<Diagnostic>()
        val programsByService = programs.groupBy { it.serviceKey }
        val windowsByService = windows.groupBy { it.serviceKey }
        val targetKeys = programsByService.keys + windowsByService.keys + verifiedEmptyServiceKeys
        val channelIds =
            channelStore.indexExistingChannelIds(targetKeys).getOrElse { error ->
                return PreparedProgramPublication(
                    emptyList(),
                    targetKeys.map { key -> Diagnostic(key, "program-channel-query", error.message.orEmpty()) },
                )
            }
        val services =
            targetKeys.mapNotNull { key ->
                val channelId = channelIds[key]
                if (channelId == null) {
                    failures += Diagnostic(key, "program-channel-query", "program 登録対象 channel がありません")
                    return@mapNotNull null
                }
                val rows =
                    programsByService[key].orEmpty().mapNotNull row@{ program ->
                        validate(program)?.let {
                            failures += it
                            return@row null
                        }
                        val providerData =
                            when (val built = ProviderDataBridge.buildProgramProviderData(program)) {
                                is ProviderDataBridge.Success -> {
                                    built.bytes
                                }

                                is ProviderDataBridge.Failure -> {
                                    failures += Diagnostic(key, "program-provider-data", "${built.errorCode}: ${built.errorMessage}")
                                    return@row null
                                }
                            }
                        val values =
                            programValues(
                                channelId,
                                program,
                                clearAbsentOptionalColumns =
                                    hasAuthoritativeOptionalColumnSnapshot(
                                        program,
                                        windowsByService[key].orEmpty(),
                                    ),
                                providerData = providerData,
                            )
                        program to values
                    }
                if (failures.any { it.serviceKey == key }) {
                    null
                } else {
                    PreparedServicePrograms(key, channelId, rows, windowsByService[key].orEmpty())
                }
            }
        return PreparedProgramPublication(services, failures)
    }

    fun upsertProgramsForWindows(
        programs: List<ProgramRecord>,
        windows: List<ProgramPublishCoordinator.EpgUpdateWindow>,
    ): UpsertResult = upsertPreparedPrograms(prepareProgramPublication(programs, windows))

    // 同じ入力に対する分岐・項目写像を保持し、処理分割による状態の受け渡しを増やさない。
    // 同じ入力と資源寿命を扱う手順を一続きに確認できる形に保つ。
    // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
    // 候補の処理と入れ子の資源寿命を同じ手順内で確認できる構造を保つ。
    @Suppress("CyclomaticComplexMethod", "LongMethod", "MaxLineLength", "NestedBlockDepth")
    internal fun upsertPreparedPrograms(publication: PreparedProgramPublication): UpsertResult {
        var inserted = 0
        var updated = 0
        var deleted = 0
        val failures = publication.failures.toMutableList()
        val genreDiagnostics = mutableListOf<GenreReadbackDiagnostic>()
        val succeededServiceKeys = linkedSetOf<ServiceKey>()
        publication.services.forEach { service ->
            val serviceKey = service.serviceKey
            val channelId = service.channelId
            val failureCountBeforeService = failures.size
            val preparationFailed = publication.failures.any { it.serviceKey == serviceKey }
            val serviceWindows = service.windows
            if (!preparationFailed && service.programs.isEmpty() && serviceWindows.isEmpty()) {
                val existingPrograms = channelStore.indexExistingProgramsForService(channelId)
                if (existingPrograms.isFailure) {
                    failures += Diagnostic(serviceKey, "program-index-query", existingPrograms.exceptionOrNull()?.message.orEmpty())
                }
                // 完成した空EITでも区間を捏造しない。所有channelとProgram問い合わせだけを確認し、既存行を保持する。
            }
            val sortedPrograms = service.programs.sortedBy { it.first.startTimeMillis }
            val existingPrograms: MutableMap<String, MutableList<ExistingProgramIndexEntry>>? =
                if (sortedPrograms.isEmpty()) {
                    linkedMapOf()
                } else {
                    val guardStart =
                        sortedPrograms.minOf { (program, _) ->
                            runCatching { Math.subtractExact(program.startTimeMillis, EVENT_ID_REUSE_GUARD_MS) }
                                .getOrDefault(Long.MIN_VALUE)
                        }
                    val guardEnd =
                        sortedPrograms.maxOf { (program, _) ->
                            val programEnd = checkNotNull(checkedProgramEndTimeMillis(program))
                            runCatching { Math.addExact(programEnd, EVENT_ID_REUSE_GUARD_MS) }
                                .getOrDefault(Long.MAX_VALUE)
                        }
                    val indexResult = channelStore.indexExistingProgramEntriesForWindow(channelId, guardStart, guardEnd)
                    if (indexResult.isFailure) {
                        failures += Diagnostic(serviceKey, "program-index-query", indexResult.exceptionOrNull()?.message.orEmpty())
                        null
                    } else {
                        indexResult.getOrThrow().mapValuesTo(linkedMapOf()) { (_, entries) -> entries.toMutableList() }
                    }
                }
            if (existingPrograms != null) {
                data class PendingWrite(
                    val program: ProgramRecord,
                    val values: ContentValues,
                    val key: String,
                    val programEnd: Long,
                    val existingId: Long?,
                )
                val publicationKeys = linkedSetOf<String>()
                val writes =
                    sortedPrograms.mapNotNull { (program, values) ->
                        val key = programIdentity(program)
                        if (!publicationKeys.add(key)) {
                            failures += Diagnostic(serviceKey, "program-batch", "同一publication内に重複program keyがあります key=$key")
                            return@mapNotNull null
                        }
                        val programEnd = checkNotNull(checkedProgramEndTimeMillis(program))
                        val guardStart =
                            runCatching { Math.subtractExact(program.startTimeMillis, EVENT_ID_REUSE_GUARD_MS) }
                                .getOrDefault(Long.MIN_VALUE)
                        val guardEnd =
                            runCatching { Math.addExact(programEnd, EVENT_ID_REUSE_GUARD_MS) }
                                .getOrDefault(Long.MAX_VALUE)
                        val existingId =
                            existingPrograms[key]
                                .orEmpty()
                                .filter { entry ->
                                    entry.endTimeMillis > guardStart && entry.startTimeMillis < guardEnd
                                }.maxByOrNull { it.programId }
                                ?.programId
                        PendingWrite(program, values, key, programEnd, existingId)
                    }
                if (failures.size == failureCountBeforeService) {
                    val batch =
                        channelStore.upsertProgramsBatch(
                            writes.map { ProgramUpsertRequest(it.existingId, it.values) },
                        )
                    if (batch.isFailure) {
                        val operation =
                            when {
                                writes.all { it.existingId == null } -> "program-insert"
                                writes.all { it.existingId != null } -> "program-update"
                                else -> "program-batch"
                            }
                        failures += Diagnostic(serviceKey, operation, batch.exceptionOrNull()?.message.orEmpty())
                    } else {
                        val outcomes = batch.getOrThrow()
                        if (outcomes.size != writes.size) {
                            failures += Diagnostic(serviceKey, "program-batch", "provider batch結果数が一致しません")
                        } else {
                            val genreTargets = linkedMapOf<Long, ContentValues>()
                            writes.zip(outcomes).forEach { (write, outcome) ->
                                val programId = outcome.programId
                                if (programId == null) {
                                    failures +=
                                        Diagnostic(
                                            serviceKey,
                                            if (write.existingId == null) "program-insert" else "program-update",
                                            "provider batchが対象rowを確定できませんでした",
                                        )
                                } else {
                                    if (write.existingId == null) {
                                        inserted++
                                        existingPrograms.getOrPut(write.key) { mutableListOf() } +=
                                            ExistingProgramIndexEntry(
                                                programId,
                                                write.program.startTimeMillis,
                                                write.programEnd,
                                            )
                                    } else {
                                        updated++
                                    }
                                    genreTargets[programId] = write.values
                                }
                            }
                            if (genreTargets.isNotEmpty()) {
                                val readback = channelStore.readCanonicalGenres(channelId, genreTargets.keys)
                                genreTargets.forEach { (programId, values) ->
                                    val directlySet = values.getAsString(TvContract.Programs.COLUMN_CANONICAL_GENRE)
                                    genreDiagnostics +=
                                        GenreReadbackDiagnostic(
                                            serviceKey,
                                            programId,
                                            directlySet,
                                            readback.getOrNull()?.get(programId),
                                            readback.exceptionOrNull()?.message,
                                        )
                                }
                            }
                        }
                    }
                }
            }
            if (!preparationFailed && failures.size == failureCountBeforeService) {
                serviceWindows.filter { it.deletionAuthoritative }.forEach { window ->
                    val deleteResult =
                        channelStore.deleteObsoletePrograms(
                            channelId,
                            window.validProgramKeys,
                            window.windowStartMs,
                            window.windowEndMs,
                        )
                    if (deleteResult.isFailure) {
                        failures += Diagnostic(serviceKey, "program-delete-obsolete", deleteResult.exceptionOrNull()?.message.orEmpty())
                    } else {
                        deleted += deleteResult.getOrNull() ?: 0
                    }
                }
            }
            if (!preparationFailed && failures.size == failureCountBeforeService) {
                succeededServiceKeys += serviceKey
            }
        }
        Log.i(LogTags.TIS, "program登録結果 inputId=$inputId inserted=$inserted updated=$updated deleted=$deleted failures=${failures.size}")
        return UpsertResult(
            inserted,
            updated,
            failures,
            deleted = deleted,
            succeededServiceKeys = succeededServiceKeys,
            genreDiagnostics = genreDiagnostics,
        )
    }

    sealed class ExistingServiceKeysResult {
        data class Success(
            val keys: Set<ServiceKey>,
        ) : ExistingServiceKeysResult()

        data class Failure(
            val diagnostics: List<Diagnostic>,
        ) : ExistingServiceKeysResult()
    }

    // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
    @Suppress("MaxLineLength")
    fun existingServiceKeysResult(keys: Iterable<ServiceKey>): ExistingServiceKeysResult {
        val requested = keys.toSet()
        val indexed = channelStore.indexExistingChannelIds(requested)
        return if (indexed.isSuccess) {
            ExistingServiceKeysResult.Success(indexed.getOrThrow().keys)
        } else {
            val message = indexed.exceptionOrNull()?.message.orEmpty()
            ExistingServiceKeysResult.Failure(requested.map { key -> Diagnostic(key, "channel-query", message) })
        }
    }

    fun existingChannelsResult(): Result<List<ChannelRecord>> =
        channelStore
            .listExistingChannels()
            .onFailure { error -> Log.w(LogTags.TIS, "既存channel復元に失敗しました inputId=$inputId", error) }

    fun programValuesForTest(
        channelId: Long,
        program: ProgramRecord,
    ): ContentValues =
        programValues(
            channelId,
            program,
            clearAbsentOptionalColumns = true,
            providerData = (ProviderDataBridge.buildProgramProviderData(program) as ProviderDataBridge.Success).bytes,
        )

    internal fun publicationInputSignature(
        program: ProgramRecord,
        windows: List<ProgramPublishCoordinator.EpgUpdateWindow>,
    ): Result<String> =
        runCatching {
            val providerData =
                when (val built = ProviderDataBridge.buildProgramProviderData(program)) {
                    is ProviderDataBridge.Success -> {
                        built.bytes
                    }

                    is ProviderDataBridge.Failure -> {
                        error("${built.errorCode}: ${built.errorMessage}")
                    }
                }
            signatureForContentValues(
                programValues(
                    channelId = PUBLICATION_INPUT_SENTINEL_CHANNEL_ID,
                    program = program,
                    clearAbsentOptionalColumns = hasAuthoritativeOptionalColumnSnapshot(program, windows),
                    providerData = providerData,
                ),
            )
        }

    // この処理の規格値・ビット幅・単位換算・固定上限をリテラルのまま照合できる形に保つ。
    @Suppress("MagicNumber")
    private fun validate(channel: ChannelRecord): Diagnostic? {
        val key = channel.serviceKey
        return when {
            key.serviceId !in 1..0xffff -> {
                Diagnostic(key, "validate", "不正な service_id=${key.serviceId}")
            }

            key.transportStreamId !in 0..0xffff -> {
                Diagnostic(key, "validate", "不正な transport_stream_id=${key.transportStreamId}")
            }

            key.originalNetworkId !in 0..0xffff -> {
                Diagnostic(key, "validate", "不正な original_network_id=${key.originalNetworkId}")
            }

            channel.deliverySystem != ChannelRecord.DELIVERY_SYSTEM_ISDB_T &&
                channel.deliverySystem != ChannelRecord.DELIVERY_SYSTEM_ISDB_S -> {
                Diagnostic(
                    key,
                    "validate",
                    "対象外 deliverySystem=${channel.deliverySystem}",
                )
            }

            inputId.isBlank() -> {
                Diagnostic(key, "validate", "inputId が空です")
            }

            else -> {
                null
            }
        }
    }

    // この処理の規格値・ビット幅・単位換算・固定上限をリテラルのまま照合できる形に保つ。
    // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
    @Suppress("MagicNumber", "MaxLineLength")
    private fun validate(program: ProgramRecord): Diagnostic? =
        when {
            program.eventId !in 0..0xffff -> Diagnostic(program.serviceKey, "program-validate", "不正な eventId=${program.eventId}")
            program.startTimeMillis <= 0L -> Diagnostic(program.serviceKey, "program-validate", "不正な start=${program.startTimeMillis}")
            program.durationMillis <= 0L -> Diagnostic(program.serviceKey, "program-validate", "不正な duration=${program.durationMillis}")
            checkedProgramEndTimeMillis(program) == null -> Diagnostic(program.serviceKey, "program-validate", "番組時刻が overflow しました")
            else -> null
        }

    // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
    @Suppress("MaxLineLength")
    private fun channelValues(
        channel: ChannelRecord,
        providerData: ByteArray,
    ): ContentValues =
        ContentValues().apply {
            put(TvContract.Channels.COLUMN_INPUT_ID, inputId)
            put(TvContract.Channels.COLUMN_TYPE, channelType(channel.deliverySystem))
            put(TvContract.Channels.COLUMN_SERVICE_TYPE, channel.serviceType.toString())
            put(TvContract.Channels.COLUMN_DISPLAY_NUMBER, channel.displayNumber.ifBlank { channel.serviceKey.serviceId.toString() })
            put(TvContract.Channels.COLUMN_DISPLAY_NAME, channel.displayName.ifBlank { fallbackName(channel.serviceKey) })
            put(TvContract.Channels.COLUMN_ORIGINAL_NETWORK_ID, channel.serviceKey.originalNetworkId)
            put(TvContract.Channels.COLUMN_TRANSPORT_STREAM_ID, channel.serviceKey.transportStreamId)
            put(TvContract.Channels.COLUMN_SERVICE_ID, channel.serviceKey.serviceId)
            put(TvContract.Channels.COLUMN_SEARCHABLE, 1)
            put(TvContract.Channels.COLUMN_INTERNAL_PROVIDER_DATA, providerData)
        }

    // 同じ入力に対する分岐・項目写像を保持し、処理分割による状態の受け渡しを増やさない。
    // 同じ入力と資源寿命を扱う手順を一続きに確認できる形に保つ。
    // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
    // 動的な引数列を既存の可変長APIへ渡すため、一時配列のコピーを許容する。
    // 現行mapperの明示写像値をencodeする。未制約のProgramRecord値域は設計上の逸脱許可対象。
    @Suppress("AndroidLintWrongConstant", "CyclomaticComplexMethod", "LongMethod", "MaxLineLength", "SpreadOperator")
    private fun programValues(
        channelId: Long,
        program: ProgramRecord,
        clearAbsentOptionalColumns: Boolean,
        providerData: ByteArray,
    ): ContentValues =
        ContentValues().apply {
            put(TvContract.Programs.COLUMN_CHANNEL_ID, channelId)
            if (program.title.isBlank()) {
                if (clearAbsentOptionalColumns) putNull(TvContract.Programs.COLUMN_TITLE)
            } else {
                put(TvContract.Programs.COLUMN_TITLE, program.title)
            }
            put(TvContract.Programs.COLUMN_EVENT_ID, program.eventId)
            put(TvContract.Programs.COLUMN_START_TIME_UTC_MILLIS, program.startTimeMillis)
            put(TvContract.Programs.COLUMN_END_TIME_UTC_MILLIS, Math.addExact(program.startTimeMillis, program.durationMillis))
            if (program.shortDescription.isBlank()) {
                if (clearAbsentOptionalColumns) putNull(TvContract.Programs.COLUMN_SHORT_DESCRIPTION)
            } else {
                put(TvContract.Programs.COLUMN_SHORT_DESCRIPTION, program.shortDescription)
            }
            if (program.description.isBlank()) {
                if (clearAbsentOptionalColumns) putNull(TvContract.Programs.COLUMN_LONG_DESCRIPTION)
            } else {
                put(TvContract.Programs.COLUMN_LONG_DESCRIPTION, program.description)
            }
            val videoWidth = program.videoWidth?.takeIf { it > 0 }
            val videoHeight = program.videoHeight?.takeIf { it > 0 }
            if (videoWidth != null && videoHeight != null) {
                put(TvContract.Programs.COLUMN_VIDEO_WIDTH, videoWidth)
                put(TvContract.Programs.COLUMN_VIDEO_HEIGHT, videoHeight)
            } else if (clearAbsentOptionalColumns) {
                putNull(TvContract.Programs.COLUMN_VIDEO_WIDTH)
                putNull(TvContract.Programs.COLUMN_VIDEO_HEIGHT)
            }
            val audioLanguages =
                program.descriptors.components.audio
                    .asSequence()
                    .flatMap { sequenceOf(it.language, it.secondLanguage) }
                    .mapNotNull(LanguageCodeNormalizer::normalizeForTvTrackLanguage)
                    .distinct()
                    .toList()
            if (audioLanguages.isEmpty() && clearAbsentOptionalColumns) {
                putNull(TvContract.Programs.COLUMN_AUDIO_LANGUAGE)
            } else if (audioLanguages.isNotEmpty()) {
                put(TvContract.Programs.COLUMN_AUDIO_LANGUAGE, audioLanguages.joinToString(","))
            }
            if (program.descriptors.broadcastGenre.isNullOrBlank()) {
                if (clearAbsentOptionalColumns) putNull(TvContract.Programs.COLUMN_BROADCAST_GENRE)
            } else {
                put(TvContract.Programs.COLUMN_BROADCAST_GENRE, TvContract.Programs.Genres.encode(program.descriptors.broadcastGenre))
            }
            val canonicalGenres = program.canonicalGenres.distinct().sorted()
            if (canonicalGenres.isEmpty()) {
                if (clearAbsentOptionalColumns) putNull(TvContract.Programs.COLUMN_CANONICAL_GENRE)
            } else {
                put(TvContract.Programs.COLUMN_CANONICAL_GENRE, TvContract.Programs.Genres.encode(*canonicalGenres.toTypedArray()))
            }
            if (program.contentRatings.isEmpty()) {
                if (clearAbsentOptionalColumns) putNull(TvContract.Programs.COLUMN_CONTENT_RATING)
            } else {
                put(
                    TvContract.Programs.COLUMN_CONTENT_RATING,
                    program.contentRatings
                        .distinct()
                        .sorted()
                        .joinToString(","),
                )
            }
            when (val scrambled = program.descriptors.scrambled) {
                null -> if (clearAbsentOptionalColumns) putNull(COLUMN_SCRAMBLED)
                else -> put(COLUMN_SCRAMBLED, if (scrambled) 1 else 0)
            }
            putSeriesColumns(program, clearAbsentOptionalColumns)
            put(TvContract.Programs.COLUMN_INTERNAL_PROVIDER_DATA, providerData)
        }

    private fun ContentValues.putSeriesColumns(
        program: ProgramRecord,
        clearAbsentOptionalColumns: Boolean,
    ) {
        val candidateSeriesIds =
            program.descriptors.seriesCandidates
                .asSequence()
                .filter { it.parseStatus == com.maleicacid.tvinput.aribsi.SiParseStatus.OK }
                .mapNotNull { it.seriesId }
                .distinct()
                .toList()
        val singleSeriesId = program.descriptors.series?.seriesId ?: candidateSeriesIds.singleOrNull()
        when {
            candidateSeriesIds.size > 1 -> {
                if (clearAbsentOptionalColumns) putNull(COLUMN_SERIES_ID)
                put(COLUMN_MULTI_SERIES_ID, candidateSeriesIds.joinToString(","))
            }

            singleSeriesId != null -> {
                put(COLUMN_SERIES_ID, singleSeriesId)
                if (clearAbsentOptionalColumns) putNull(COLUMN_MULTI_SERIES_ID)
            }

            clearAbsentOptionalColumns -> {
                putNull(COLUMN_SERIES_ID)
                putNull(COLUMN_MULTI_SERIES_ID)
            }
        }

        val episodeNumber = program.descriptors.series?.episodeNumber
        if (episodeNumber == null || episodeNumber <= 0) {
            if (clearAbsentOptionalColumns) putNull(COLUMN_EPISODE_DISPLAY_NUMBER)
        } else {
            put(COLUMN_EPISODE_DISPLAY_NUMBER, episodeNumber.toString())
        }
    }

    private fun hasAuthoritativeOptionalColumnSnapshot(
        program: ProgramRecord,
        windows: List<ProgramPublishCoordinator.EpgUpdateWindow>,
    ): Boolean {
        val programEnd = checkedProgramEndTimeMillis(program) ?: return false
        val key = programIdentity(program)
        return windows.any { window ->
            window.deletionAuthoritative && key in window.validProgramKeys &&
                program.startTimeMillis < window.windowEndMs && programEnd > window.windowStartMs
        }
    }

    private fun programIdentity(program: ProgramRecord): String = ProviderDataBridge.buildProgramKey(program)

    private fun checkedProgramEndTimeMillis(program: ProgramRecord): Long? =
        runCatching { Math.addExact(program.startTimeMillis, program.durationMillis) }.getOrNull()

    // 同じ状態・境界を扱う操作群を一つの所有者に保つ。
    @Suppress("TooManyFunctions")
    companion object {
        /**
         * テスト専用 assertion 用に維持する フィールド 名。本番 provider-data は
         * ProviderDataBridge / Rust だけが生成・正規化する。
         */
        const val PROGRAM_KEY_FIELD = "programKey"
        internal const val EVENT_ID_REUSE_GUARD_MS = 24L * 60L * 60L * 1_000L
        private const val PUBLICATION_INPUT_SENTINEL_CHANNEL_ID = 0L
        const val COLUMN_SCRAMBLED = "scrambled"
        const val COLUMN_SERIES_ID = "series_id"
        const val COLUMN_MULTI_SERIES_ID = "multi_series_id"
        const val COLUMN_EPISODE_DISPLAY_NUMBER = "episode_display_number"
        private val SIGNATURE_COLUMNS =
            listOf(
                TvContract.Programs.COLUMN_CHANNEL_ID,
                TvContract.Programs.COLUMN_TITLE,
                TvContract.Programs.COLUMN_EPISODE_TITLE,
                TvContract.Programs.COLUMN_EVENT_ID,
                TvContract.Programs.COLUMN_SHORT_DESCRIPTION,
                TvContract.Programs.COLUMN_LONG_DESCRIPTION,
                TvContract.Programs.COLUMN_START_TIME_UTC_MILLIS,
                TvContract.Programs.COLUMN_END_TIME_UTC_MILLIS,
                TvContract.Programs.COLUMN_AUDIO_LANGUAGE,
                TvContract.Programs.COLUMN_VIDEO_WIDTH,
                TvContract.Programs.COLUMN_VIDEO_HEIGHT,
                TvContract.Programs.COLUMN_BROADCAST_GENRE,
                TvContract.Programs.COLUMN_CANONICAL_GENRE,
                TvContract.Programs.COLUMN_CONTENT_RATING,
                COLUMN_SCRAMBLED,
                COLUMN_SERIES_ID,
                COLUMN_MULTI_SERIES_ID,
                COLUMN_EPISODE_DISPLAY_NUMBER,
                TvContract.Programs.COLUMN_INTERNAL_PROVIDER_DATA,
            )

        fun programKeyForTest(program: ProgramRecord): String = ProviderDataBridge.buildProgramKey(program)

        fun programProviderDataForTest(program: ProgramRecord): String =
            (ProviderDataBridge.buildProgramProviderData(program) as ProviderDataBridge.Success).json

        fun parseProgramKey(providerData: ByteArray?): String? = ProviderDataBridge.extractProgramKey(providerData)

        // 入力拒否・未準備・失敗を発生点で返し、成功経路を深い入れ子にしない。
        @Suppress("ReturnCount")
        fun providerDataMatchesService(
            providerData: ByteArray?,
            serviceKey: ServiceKey?,
        ): Boolean {
            if (serviceKey == null) return true
            val key = ProviderDataBridge.extractProgramKeyResult(providerData) ?: return false
            return key.serviceKey == serviceKey
        }

        internal fun shouldDeleteOwnedObsoleteProgramRow(
            ownerPackage: String?,
            ownPackage: String,
            programKey: String?,
            validProgramKeys: Set<String>,
        ): Boolean = ownerPackage == ownPackage && (programKey == null || programKey !in validProgramKeys)

        private fun MessageDigest.appendField(
            column: String,
            value: Any?,
        ) {
            val bytes =
                when (value) {
                    null -> byteArrayOf()
                    is ByteArray -> value
                    else -> value.toString().toByteArray(Charsets.UTF_8)
                }
            update(column.toByteArray(Charsets.UTF_8))
            update(0.toByte())
            update(bytes.size.toString().toByteArray(Charsets.US_ASCII))
            update(0.toByte())
            update(bytes)
        }

        private fun MessageDigest.appendRow(values: ContentValues) {
            SIGNATURE_COLUMNS.forEach { column -> appendField(column, values.get(column)) }
        }

        private fun MessageDigest.lowercaseHex(): String = digest().joinToString("") { "%02x".format(it) }

        internal fun publicationFingerprint(services: List<PreparedServicePrograms>): String {
            val digest = MessageDigest.getInstance("SHA-256")
            val sorted =
                services.sortedWith(
                    compareBy<PreparedServicePrograms> { it.serviceKey.originalNetworkId }
                        .thenBy { it.serviceKey.transportStreamId }
                        .thenBy { it.serviceKey.serviceId },
                )
            digest.appendField("serviceCount", sorted.size)
            sorted.forEach { service ->
                digest.appendField("channelId", service.channelId)
                digest.appendField("rowCount", service.programs.size)
                service.programs
                    .sortedWith(
                        compareBy<Pair<ProgramRecord, ContentValues>> { it.first.eventId }
                            .thenBy { it.first.startTimeMillis },
                    ).forEach { (_, values) -> digest.appendRow(values) }
                digest.appendField("windowCount", service.windows.size)
                service.windows
                    .sortedWith(
                        compareBy<ProgramPublishCoordinator.EpgUpdateWindow> { it.windowStartMs }
                            .thenBy { it.windowEndMs }
                            .thenBy { it.deletionAuthoritative },
                    ).forEach { window ->
                        digest.appendField("windowStartMs", window.windowStartMs)
                        digest.appendField("windowEndMs", window.windowEndMs)
                        digest.appendField("deletionAuthoritative", window.deletionAuthoritative)
                        digest.appendField("validProgramKeyCount", window.validProgramKeys.size)
                        window.validProgramKeys.sorted().forEach { key -> digest.appendField("validProgramKey", key) }
                    }
            }
            return digest.lowercaseHex()
        }

        internal fun signatureForContentValues(values: ContentValues): String =
            MessageDigest.getInstance("SHA-256").apply { appendRow(values) }.lowercaseHex()

        // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
        @Suppress("MaxLineLength")
        fun signatureForProgramForTest(
            channelId: Long,
            program: ProgramRecord,
        ): String {
            val writer =
                TvProviderWriter(
                    "test",
                    object : ChannelStore {
                        override fun findExistingChannelId(key: ServiceKey): Result<Long?> = Result.success(channelId)

                        override fun insertChannel(values: ContentValues): Result<Long?> = Result.success(channelId)

                        override fun updateChannel(
                            channelId: Long,
                            values: ContentValues,
                        ): Result<Int> = Result.success(1)
                    },
                    testOnly = true,
                )
            return signatureForContentValues(
                writer.programValues(
                    channelId,
                    program,
                    clearAbsentOptionalColumns = true,
                    providerData = (ProviderDataBridge.buildProgramProviderData(program) as ProviderDataBridge.Success).bytes,
                ),
            )
        }
    }

    // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
    @Suppress("MaxLineLength")
    private fun fallbackName(key: ServiceKey): String = "service-${key.originalNetworkId}-${key.transportStreamId}-${key.serviceId}"

    private fun channelType(deliverySystem: String): String =
        when (deliverySystem) {
            ChannelRecord.DELIVERY_SYSTEM_ISDB_T -> TvContract.Channels.TYPE_ISDB_T
            ChannelRecord.DELIVERY_SYSTEM_ISDB_S -> TvContract.Channels.TYPE_ISDB_S
            else -> TvContract.Channels.TYPE_OTHER
        }

    // 同じ状態・境界を扱う操作群を一つの所有者に保つ。
    @Suppress("TooManyFunctions")
    private class AndroidTvProviderChannelStore(
        private val context: Context,
        private val inputId: String,
    ) : ChannelStore {
        private companion object {
            const val CHANNEL_ID_COLUMN_INDEX = 0
            const val ORIGINAL_NETWORK_ID_COLUMN_INDEX = 1
            const val TRANSPORT_STREAM_ID_COLUMN_INDEX = 2
            const val SERVICE_ID_COLUMN_INDEX = 3
            const val PROGRAM_ID_COLUMN_INDEX = 0
            const val PROGRAM_PROVIDER_DATA_COLUMN_INDEX = 1
            const val PROGRAM_START_TIME_COLUMN_INDEX = 2
            const val PROGRAM_END_TIME_COLUMN_INDEX = 3
            const val PROGRAM_PROVIDER_BATCH_SIZE = 64
        }

        // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
        @Suppress("MaxLineLength")
        override fun findExistingChannelId(key: ServiceKey): Result<Long?> =
            runCatching {
                val projection =
                    arrayOf(
                        TvContract.Channels._ID,
                        TvContract.Channels.COLUMN_ORIGINAL_NETWORK_ID,
                        TvContract.Channels.COLUMN_TRANSPORT_STREAM_ID,
                        TvContract.Channels.COLUMN_SERVICE_ID,
                    )
                val uri = TvContract.buildChannelsUriForInput(inputId)
                val cursor =
                    context.contentResolver.query(uri, projection, null, null, null)
                        ?: error("TvProvider channel query returned null cursor")
                cursor.use { rows ->
                    var found: Long? = null
                    while (rows.moveToNext()) {
                        if (rows.getInt(ORIGINAL_NETWORK_ID_COLUMN_INDEX) == key.originalNetworkId &&
                            rows.getInt(TRANSPORT_STREAM_ID_COLUMN_INDEX) == key.transportStreamId &&
                            rows.getInt(SERVICE_ID_COLUMN_INDEX) == key.serviceId
                        ) {
                            found = rows.getLong(CHANNEL_ID_COLUMN_INDEX)
                            break
                        }
                    }
                    found
                }
            }.onFailure { Log.w(LogTags.TIS, "既存 channel 検索に失敗しました key=$key", it) }

        override fun indexExistingChannelIds(keys: Set<ServiceKey>): Result<Map<ServiceKey, Long>> =
            runCatching {
                if (keys.isEmpty()) return@runCatching emptyMap()
                val projection =
                    arrayOf(
                        TvContract.Channels._ID,
                        TvContract.Channels.COLUMN_ORIGINAL_NETWORK_ID,
                        TvContract.Channels.COLUMN_TRANSPORT_STREAM_ID,
                        TvContract.Channels.COLUMN_SERVICE_ID,
                    )
                val wanted = keys.toHashSet()
                val out = linkedMapOf<ServiceKey, Long>()
                val cursor =
                    context.contentResolver.query(
                        TvContract.buildChannelsUriForInput(inputId),
                        projection,
                        null,
                        null,
                        null,
                    ) ?: error("TvProviderのチャンネル索引照会がnull cursorを返しました")
                cursor.use { rows ->
                    while (rows.moveToNext()) {
                        val key =
                            ServiceKey(
                                rows.getInt(ORIGINAL_NETWORK_ID_COLUMN_INDEX),
                                rows.getInt(TRANSPORT_STREAM_ID_COLUMN_INDEX),
                                rows.getInt(SERVICE_ID_COLUMN_INDEX),
                            )
                        if (key in wanted) out[key] = rows.getLong(0)
                    }
                }
                out
            }

        override fun insertChannel(values: ContentValues): Result<Long?> =
            runCatching {
                val uri: Uri? = context.contentResolver.insert(TvContract.Channels.CONTENT_URI, values)
                uri?.let { ContentUris.parseId(it) }
            }

        // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
        @Suppress("MaxLineLength")
        override fun updateChannel(
            channelId: Long,
            values: ContentValues,
        ): Result<Int> =
            runCatching {
                context.contentResolver.update(ContentUris.withAppendedId(TvContract.Channels.CONTENT_URI, channelId), values, null, null)
            }

        // この処理の規格値・ビット幅・単位換算・固定上限をリテラルのまま照合できる形に保つ。
        @Suppress("MagicNumber")
        override fun listExistingChannels(): Result<List<ChannelRecord>> =
            runCatching {
                val projection =
                    arrayOf(
                        TvContract.Channels._ID,
                        TvContract.Channels.COLUMN_ORIGINAL_NETWORK_ID,
                        TvContract.Channels.COLUMN_TRANSPORT_STREAM_ID,
                        TvContract.Channels.COLUMN_SERVICE_ID,
                        TvContract.Channels.COLUMN_DISPLAY_NUMBER,
                        TvContract.Channels.COLUMN_DISPLAY_NAME,
                        TvContract.Channels.COLUMN_SERVICE_TYPE,
                        TvContract.Channels.COLUMN_INTERNAL_PROVIDER_DATA,
                    )
                val out = mutableListOf<ChannelRecord>()
                val uri = TvContract.buildChannelsUriForInput(inputId)
                val cursor =
                    context.contentResolver.query(uri, projection, null, null, null)
                        ?: error("TvProvider channel list query returned null cursor")
                cursor.use { cursor ->
                    while (cursor.moveToNext()) {
                        val stored = ProviderDataBridge.decodeChannelProviderData(providerDataBytes(cursor, 7))
                        val serviceType = cursor.getString(6)?.toIntOrNull()?.takeIf { it in 0..0xff }
                        val rowServiceKey = ServiceKey(cursor.getInt(1), cursor.getInt(2), cursor.getInt(3))
                        if (stored == null || stored.serviceKey != rowServiceKey || serviceType == null) {
                            error("既存 channel の物理選局情報を復元できません id=${cursor.getLong(0)}")
                        } else {
                            out +=
                                ChannelRecord(
                                    serviceKey = rowServiceKey,
                                    serviceType = serviceType,
                                    displayNumber = cursor.getString(4).orEmpty(),
                                    displayName =
                                        cursor.getString(5).orEmpty().ifBlank {
                                            "service-${rowServiceKey.originalNetworkId}-" +
                                                "${rowServiceKey.transportStreamId}-${rowServiceKey.serviceId}"
                                        },
                                    frequencyHz = stored.tune.frequencyHz,
                                    tvProviderChannelId = cursor.getLong(0),
                                    deliverySystem = stored.tune.deliverySystem,
                                    streamSelector = stored.tune.streamSelector,
                                    physicalChannel = stored.tune.physicalChannel,
                                    satelliteBand = stored.tune.satelliteBand,
                                    remoteControlKeyId = stored.tune.remoteControlKeyId,
                                    requiresCas = stored.requiresCas,
                                )
                        }
                    }
                }
                out
            }

        override fun indexExistingProgramsForWindow(
            channelId: Long,
            windowStartMs: Long,
            windowEndMs: Long,
        ): Result<Map<String, Long>> =
            runCatching {
                val projection = arrayOf(TvContract.Programs._ID, TvContract.Programs.COLUMN_INTERNAL_PROVIDER_DATA)
                val uri = TvContract.buildProgramsUriForChannel(channelId, windowStartMs, windowEndMs)
                val cursor =
                    context.contentResolver.query(
                        uri,
                        projection,
                        null,
                        null,
                        "${TvContract.Programs._ID} DESC",
                    ) ?: error("TvProvider program index queryがnull cursorを返しました")
                val out = linkedMapOf<String, Long>()
                cursor.use { c ->
                    while (c.moveToNext()) {
                        val data = providerDataBytes(c, 1)
                        val key = parseProgramKey(data)
                        if (key != null && key !in out) out[key] = c.getLong(0)
                    }
                }
                out
            }

        override fun indexExistingProgramEntriesForWindow(
            channelId: Long,
            windowStartMs: Long,
            windowEndMs: Long,
        ): Result<Map<String, List<ExistingProgramIndexEntry>>> =
            runCatching {
                val projection =
                    arrayOf(
                        TvContract.Programs._ID,
                        TvContract.Programs.COLUMN_INTERNAL_PROVIDER_DATA,
                        TvContract.Programs.COLUMN_START_TIME_UTC_MILLIS,
                        TvContract.Programs.COLUMN_END_TIME_UTC_MILLIS,
                    )
                val uri = TvContract.buildProgramsUriForChannel(channelId, windowStartMs, windowEndMs)
                val cursor =
                    context.contentResolver.query(
                        uri,
                        projection,
                        null,
                        null,
                        "${TvContract.Programs._ID} DESC",
                    ) ?: error("TvProvider program index queryがnull cursorを返しました")
                val out = linkedMapOf<String, MutableList<ExistingProgramIndexEntry>>()
                cursor.use { rows ->
                    while (rows.moveToNext()) {
                        val key =
                            parseProgramKey(
                                providerDataBytes(rows, PROGRAM_PROVIDER_DATA_COLUMN_INDEX),
                            ) ?: continue
                        val hasDefinedTiming =
                            !rows.isNull(PROGRAM_START_TIME_COLUMN_INDEX) &&
                                !rows.isNull(PROGRAM_END_TIME_COLUMN_INDEX)
                        check(hasDefinedTiming) {
                            "TvProvider program index rowの時刻が未定義です " +
                                "id=${rows.getLong(PROGRAM_ID_COLUMN_INDEX)}"
                        }
                        out.getOrPut(key) { mutableListOf() } +=
                            ExistingProgramIndexEntry(
                                programId = rows.getLong(PROGRAM_ID_COLUMN_INDEX),
                                startTimeMillis = rows.getLong(PROGRAM_START_TIME_COLUMN_INDEX),
                                endTimeMillis = rows.getLong(PROGRAM_END_TIME_COLUMN_INDEX),
                            )
                    }
                }
                out
            }

        override fun indexExistingProgramsForService(channelId: Long): Result<Map<String, Long>> =
            runCatching {
                val projection = arrayOf(TvContract.Programs._ID, TvContract.Programs.COLUMN_INTERNAL_PROVIDER_DATA)
                val uri = TvContract.buildProgramsUriForChannel(channelId)
                val cursor =
                    context.contentResolver.query(
                        uri,
                        projection,
                        null,
                        null,
                        "${TvContract.Programs._ID} DESC",
                    ) ?: error("TvProvider service program index queryがnull cursorを返しました")
                val out = linkedMapOf<String, Long>()
                cursor.use { c ->
                    while (c.moveToNext()) {
                        val data = providerDataBytes(c, 1)
                        val key = parseProgramKey(data)
                        if (key != null && key !in out) out[key] = c.getLong(0)
                    }
                }
                out
            }

        private fun providerDataBytes(
            cursor: android.database.Cursor,
            index: Int,
        ): ByteArray? = cursor.getBlob(index)

        override fun upsertProgramsBatch(requests: List<ProgramUpsertRequest>): Result<List<ProgramUpsertOutcome>> =
            runCatching {
                if (requests.isEmpty()) return@runCatching emptyList()
                val outcomes = mutableListOf<ProgramUpsertOutcome>()
                requests.chunked(PROGRAM_PROVIDER_BATCH_SIZE).forEach { chunk ->
                    val operations =
                        chunk.map { request ->
                            val existingId = request.existingProgramId
                            if (existingId == null) {
                                ContentProviderOperation
                                    .newInsert(TvContract.Programs.CONTENT_URI)
                                    .withValues(request.values)
                                    .build()
                            } else {
                                ContentProviderOperation
                                    .newUpdate(ContentUris.withAppendedId(TvContract.Programs.CONTENT_URI, existingId))
                                    .withValues(request.values)
                                    .build()
                            }
                        }
                    val results =
                        context.contentResolver.applyBatch(
                            TvContract.AUTHORITY,
                            ArrayList(operations),
                        )
                    check(results.size == chunk.size) { "TvProvider program batch結果数が一致しません" }
                    chunk.zip(results).forEach { (request, result) ->
                        outcomes +=
                            if (request.existingProgramId == null) {
                                ProgramUpsertOutcome(
                                    result.uri?.let { uri -> ContentUris.parseId(uri) },
                                    updated = false,
                                )
                            } else {
                                ProgramUpsertOutcome(
                                    request.existingProgramId.takeIf { (result.count ?: 0) > 0 },
                                    updated = (result.count ?: 0) > 0,
                                )
                            }
                    }
                }
                outcomes
            }

        // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
        @Suppress("MaxLineLength")
        override fun readCanonicalGenres(
            channelId: Long,
            programIds: Set<Long>,
        ): Result<Map<Long, String?>> =
            runCatching {
                if (programIds.isEmpty()) return@runCatching emptyMap()
                val wanted = programIds.toHashSet()
                val out = linkedMapOf<Long, String?>()
                val cursor =
                    context.contentResolver.query(
                        TvContract.buildProgramsUriForChannel(channelId),
                        arrayOf(TvContract.Programs._ID, TvContract.Programs.COLUMN_CANONICAL_GENRE),
                        null,
                        null,
                        null,
                    ) ?: error("TvProvider ジャンル一括読戻しが null cursor を返しました")
                cursor.use { rows ->
                    while (rows.moveToNext()) {
                        val id = rows.getLong(0)
                        if (id in wanted) out[id] = rows.getString(1)
                    }
                }
                out
            }

        // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
        @Suppress("MaxLineLength")
        override fun deleteObsoletePrograms(
            channelId: Long,
            validProgramKeys: Set<String>,
            windowStartMs: Long,
            windowEndMs: Long,
        ): Result<Int> =
            runCatching {
                var deleted = 0
                val projection =
                    arrayOf(
                        TvContract.Programs._ID,
                        TvContract.Programs.COLUMN_PACKAGE_NAME,
                        TvContract.Programs.COLUMN_INTERNAL_PROVIDER_DATA,
                    )
                val uri = TvContract.buildProgramsUriForChannel(channelId, windowStartMs, windowEndMs)
                val cursor =
                    context.contentResolver.query(uri, projection, null, null, null)
                        ?: error("TvProvider obsolete program queryがnull cursorを返しました")
                val deleteIds = mutableListOf<Long>()
                cursor.use { cursor ->
                    while (cursor.moveToNext()) {
                        val id = cursor.getLong(0)
                        val ownerPackage = cursor.getString(1)
                        val key = parseProgramKey(providerDataBytes(cursor, 2))
                        if (shouldDeleteOwnedObsoleteProgramRow(
                                ownerPackage,
                                context.packageName,
                                key,
                                validProgramKeys,
                            )
                        ) {
                            deleteIds += id
                        } else if (key == null) {
                            Log.w(
                                LogTags.TIS,
                                "所有元を確認できないProgram provider-data破損行は保持します id=$id owner=$ownerPackage",
                            )
                        }
                    }
                }
                deleteIds.chunked(PROGRAM_PROVIDER_BATCH_SIZE).forEach { ids ->
                    val operations =
                        ids.map { id ->
                            ContentProviderOperation
                                .newDelete(ContentUris.withAppendedId(TvContract.Programs.CONTENT_URI, id))
                                .build()
                        }
                    val results =
                        context.contentResolver.applyBatch(
                            TvContract.AUTHORITY,
                            ArrayList(operations),
                        )
                    deleted += results.sumOf { it.count ?: 0 }
                }
                deleted
            }
    }
}
