package com.maleicacid.tvinput.tis

import android.content.ContentUris
import android.content.ContentValues
import android.content.Context
import android.media.tv.TvContract
import android.net.Uri
import android.util.Log
import com.maleicacid.tvinput.common.LogTags
import com.maleicacid.tvinput.common.FrequencyHz
import com.maleicacid.tvinput.common.ServiceKey
import com.maleicacid.tvinput.common.StreamSelector
import com.maleicacid.tvinput.db.ChannelRecord
import com.maleicacid.tvinput.db.ProgramRecord
import com.maleicacid.tvinput.aribsi.ProviderDataBridge
import java.security.MessageDigest

class TvProviderWriter private constructor(
    private val inputId: String,
    private val channelStore: ChannelStore,
) {
    constructor(context: Context, inputId: String) : this(inputId, AndroidTvProviderChannelStore(context, inputId))

    constructor(inputId: String, channelStore: ChannelStore, @Suppress("UNUSED_PARAMETER") testOnly: Boolean) : this(inputId, channelStore)

    data class Diagnostic(val serviceKey: ServiceKey?, val operation: String, val message: String)
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

    interface ChannelStore {
        fun findExistingChannelId(key: ServiceKey): Result<Long?>
        fun insertChannel(values: ContentValues): Result<Long?>
        fun updateChannel(channelId: Long, values: ContentValues): Result<Int>
        fun indexExistingProgramsForWindow(channelId: Long, windowStartMs: Long, windowEndMs: Long): Result<Map<String, Long>> = Result.success(emptyMap())
        /**
         * channel 全体の既存 Program row を stable programKey で引ける形で返す。
         * EPG 更新区間 より意図的に広く取得し、start / end time が現在 window の外へ
         * 移動した event も、duplicate insert ではなく stable ONID / TSID / SID / event identity で更新する。
         */
        fun indexExistingProgramsForService(channelId: Long): Result<Map<String, Long>> =
            indexExistingProgramsForWindow(channelId, Long.MIN_VALUE, Long.MAX_VALUE)
        fun insertProgram(values: ContentValues): Result<Long?> = Result.failure(UnsupportedOperationException("この store は program insert に対応しません"))
        fun updateProgram(programId: Long, values: ContentValues): Result<Int> = Result.failure(UnsupportedOperationException("この store は program update に対応しません"))
        fun readCanonicalGenre(programId: Long): Result<String?> =
            Result.failure(UnsupportedOperationException("この store はジャンル読戻しに対応しません"))
        fun deleteObsoletePrograms(channelId: Long, validProgramKeys: Set<String>, windowStartMs: Long, windowEndMs: Long): Result<Int> = Result.success(0)
        fun listExistingChannels(): Result<List<ChannelRecord>> = Result.success(emptyList())
    }

    fun upsertChannels(channels: List<ChannelRecord>): UpsertResult {
        var inserted = 0
        var updated = 0
        val failures = mutableListOf<Diagnostic>()
        channels.forEach { channel ->
            val validation = validate(channel)
            if (validation != null) { failures += validation; return@forEach }
            val providerData = when (val built = ProviderDataBridge.buildChannelProviderData(channel)) {
                is ProviderDataBridge.Success -> built.bytes
                is ProviderDataBridge.Failure -> {
                    failures += Diagnostic(channel.serviceKey, "provider-data", "${built.errorCode}: ${built.errorMessage}")
                    return@forEach
                }
            }
            val values = channelValues(channel, providerData)
            val existingIdResult = channelStore.findExistingChannelId(channel.serviceKey)
            if (existingIdResult.isFailure) { failures += Diagnostic(channel.serviceKey, "query", existingIdResult.exceptionOrNull()?.message.orEmpty()); return@forEach }
            val existingId = existingIdResult.getOrNull()
            if (existingId == null) {
                val insertedIdResult = channelStore.insertChannel(values)
                if (insertedIdResult.isFailure) { failures += Diagnostic(channel.serviceKey, "insert", insertedIdResult.exceptionOrNull()?.message.orEmpty()); return@forEach }
                if (insertedIdResult.getOrNull() == null) failures += Diagnostic(channel.serviceKey, "insert", "provider が null URI を返しました") else inserted++
            } else {
                values.remove(TvContract.Channels.COLUMN_TYPE)
                val updateResult = channelStore.updateChannel(existingId, values)
                if (updateResult.isFailure) { failures += Diagnostic(channel.serviceKey, "update", updateResult.exceptionOrNull()?.message.orEmpty()); return@forEach }
                if (updateResult.getOrNull() == null || updateResult.getOrNull()!! <= 0) failures += Diagnostic(channel.serviceKey, "update", "provider 更新対象行なし id=$existingId") else updated++
            }
        }
        Log.i(LogTags.TIS, "channel登録結果 inputId=$inputId inserted=$inserted updated=$updated failures=${failures.size}")
        return UpsertResult(inserted, updated, failures)
    }

    fun upsertPrograms(programs: List<ProgramRecord>): UpsertResult = upsertProgramsForWindows(
        programs = programs,
        windows = programs.groupBy { it.serviceKey }.map { (key, values) ->
            ProgramPublishCoordinator.EpgUpdateWindow(
                serviceKey = key,
                windowStartMs = values.minOf { it.startTimeMillis },
                windowEndMs = values.mapNotNull(::checkedProgramEndTimeMillis).maxOrNull()
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

    internal fun prepareProgramPublication(
        programs: List<ProgramRecord>,
        windows: List<ProgramPublishCoordinator.EpgUpdateWindow>,
        verifiedEmptyServiceKeys: Set<ServiceKey> = emptySet(),
    ): PreparedProgramPublication {
        val failures = mutableListOf<Diagnostic>()
        val programsByService = programs.groupBy { it.serviceKey }
        val windowsByService = windows.groupBy { it.serviceKey }
        val services = (programsByService.keys + windowsByService.keys + verifiedEmptyServiceKeys).mapNotNull { key ->
            val channelId = channelStore.findExistingChannelId(key).getOrElse { error ->
                failures += Diagnostic(key, "program-channel-query", error.message.orEmpty())
                return@mapNotNull null
            }
            if (channelId == null) {
                failures += Diagnostic(key, "program-channel-query", "program 登録対象 channel がありません")
                return@mapNotNull null
            }
            val rows = programsByService[key].orEmpty().mapNotNull row@ { program ->
                validate(program)?.let { failures += it; return@row null }
                val providerData = when (val built = ProviderDataBridge.buildProgramProviderData(program)) {
                    is ProviderDataBridge.Success -> built.bytes
                    is ProviderDataBridge.Failure -> {
                        failures += Diagnostic(key, "program-provider-data", "${built.errorCode}: ${built.errorMessage}")
                        return@row null
                    }
                }
                val values = programValues(
                    channelId, program,
                    clearAbsentOptionalColumns = hasAuthoritativeOptionalColumnSnapshot(program, windowsByService[key].orEmpty()),
                    providerData = providerData,
                )
                program to values
            }
            if (failures.any { it.serviceKey == key }) null
            else PreparedServicePrograms(key, channelId, rows, windowsByService[key].orEmpty())
        }
        return PreparedProgramPublication(services, failures)
    }

    fun upsertProgramsForWindows(programs: List<ProgramRecord>, windows: List<ProgramPublishCoordinator.EpgUpdateWindow>): UpsertResult =
        upsertPreparedPrograms(prepareProgramPublication(programs, windows))

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
            service.programs.sortedBy { it.first.startTimeMillis }.forEach { (program, values) ->
                val key = programIdentity(program)
                val programEnd = checkedProgramEndTimeMillis(program)
                if (programEnd == null) {
                    failures += Diagnostic(serviceKey, "program-index-window", "program end time overflow eventId=${program.eventId}")
                    return@forEach
                }
                val guardStart = runCatching { Math.subtractExact(program.startTimeMillis, EVENT_ID_REUSE_GUARD_MS) }
                    .getOrDefault(Long.MIN_VALUE)
                val guardEnd = runCatching { Math.addExact(programEnd, EVENT_ID_REUSE_GUARD_MS) }
                    .getOrDefault(Long.MAX_VALUE)
                val indexResult = channelStore.indexExistingProgramsForWindow(channelId, guardStart, guardEnd)
                if (indexResult.isFailure) {
                    failures += Diagnostic(serviceKey, "program-index-query", indexResult.exceptionOrNull()?.message.orEmpty())
                    return@forEach
                }
                // event_idは終了後24時間を越えて再利用できるため、service全履歴ではなく
                // 対象event近傍のguard window内だけでstable programKeyを照合する。
                val existingId = indexResult.getOrThrow()[key]
                if (existingId == null) {
                    val insertResult = channelStore.insertProgram(values)
                    if (insertResult.isFailure) { failures += Diagnostic(serviceKey, "program-insert", insertResult.exceptionOrNull()?.message.orEmpty()); return@forEach }
                    val insertedId = insertResult.getOrNull()
                    if (insertedId == null) {
                        failures += Diagnostic(serviceKey, "program-insert", "provider が null URI を返しました")
                    } else {
                        inserted++
                        genreDiagnostics += recordGenreReadback(serviceKey, insertedId, values)
                    }
                } else {
                    val updateResult = channelStore.updateProgram(existingId, values)
                    if (updateResult.isFailure) { failures += Diagnostic(serviceKey, "program-update", updateResult.exceptionOrNull()?.message.orEmpty()); return@forEach }
                    if ((updateResult.getOrNull() ?: 0) <= 0) {
                        failures += Diagnostic(serviceKey, "program-update", "provider 更新対象行なし id=$existingId")
                    } else {
                        updated++
                        genreDiagnostics += recordGenreReadback(serviceKey, existingId, values)
                    }
                }
            }
            if (!preparationFailed && failures.size == failureCountBeforeService) {
                serviceWindows.filter { it.deletionAuthoritative }.forEach { window ->
                    val deleteResult = channelStore.deleteObsoletePrograms(channelId, window.validProgramKeys, window.windowStartMs, window.windowEndMs)
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
        return UpsertResult(inserted, updated, failures, deleted = deleted, succeededServiceKeys = succeededServiceKeys, genreDiagnostics = genreDiagnostics)
    }


    private fun recordGenreReadback(serviceKey: ServiceKey, programId: Long, values: ContentValues): GenreReadbackDiagnostic {
        val directlySet = values.getAsString(TvContract.Programs.COLUMN_CANONICAL_GENRE)
        val readBack = channelStore.readCanonicalGenre(programId)
        val diagnostic = GenreReadbackDiagnostic(serviceKey, programId, directlySet, readBack.getOrNull(), readBack.exceptionOrNull()?.message)
        // 読戻し値から補完理由を推測せず、実際に設定した値と観測結果を独立して残す。
        Log.i(LogTags.TIS, "program genre readback id=$programId directlySet=$directlySet readBack=${diagnostic.readBackCanonicalGenre} failure=${diagnostic.readFailure}")
        return diagnostic
    }


    sealed class ExistingServiceKeysResult {
        data class Success(val keys: Set<ServiceKey>) : ExistingServiceKeysResult()
        data class Failure(val diagnostics: List<Diagnostic>) : ExistingServiceKeysResult()
    }

    fun existingServiceKeysResult(keys: Iterable<ServiceKey>): ExistingServiceKeysResult {
        val out = linkedSetOf<ServiceKey>()
        val failures = mutableListOf<Diagnostic>()
        keys.forEach { key ->
            val result = channelStore.findExistingChannelId(key)
            if (result.isFailure) {
                failures += Diagnostic(key, "channel-query", result.exceptionOrNull()?.message.orEmpty())
            } else if (result.getOrNull() != null) {
                out += key
            }
        }
        return if (failures.isEmpty()) ExistingServiceKeysResult.Success(out) else ExistingServiceKeysResult.Failure(failures)
    }

    fun existingServiceKeys(keys: Iterable<ServiceKey>): Set<ServiceKey> = when (val result = existingServiceKeysResult(keys)) {
        is ExistingServiceKeysResult.Success -> result.keys
        is ExistingServiceKeysResult.Failure -> emptySet()
    }

    fun existingChannelsResult(): Result<List<ChannelRecord>> = channelStore.listExistingChannels()
        .onFailure { error -> Log.w(LogTags.TIS, "既存channel復元に失敗しました inputId=$inputId", error) }

    @Deprecated("TvProvider問い合わせ失敗を空のチャンネル一覧へ潰してはなりません", level = DeprecationLevel.ERROR)
    fun existingChannelsForTestOnly(): List<ChannelRecord> = existingChannelsResult().getOrElse { emptyList() }

    fun validateForTest(channel: ChannelRecord): Diagnostic? = validate(channel)
    fun channelValuesForTest(channel: ChannelRecord): ContentValues =
        channelValues(channel, (ProviderDataBridge.buildChannelProviderData(channel) as ProviderDataBridge.Success).bytes)
    fun programValuesForTest(channelId: Long, program: ProgramRecord): ContentValues =
        programValues(channelId, program, clearAbsentOptionalColumns = true, providerData = (ProviderDataBridge.buildProgramProviderData(program) as ProviderDataBridge.Success).bytes)

    private fun validate(channel: ChannelRecord): Diagnostic? {
        val key = channel.serviceKey
        return when {
            key.serviceId !in 1..0xffff -> Diagnostic(key, "validate", "不正な service_id=${key.serviceId}")
            key.transportStreamId !in 0..0xffff -> Diagnostic(key, "validate", "不正な transport_stream_id=${key.transportStreamId}")
            key.originalNetworkId !in 0..0xffff -> Diagnostic(key, "validate", "不正な original_network_id=${key.originalNetworkId}")
                        channel.deliverySystem != ChannelRecord.DELIVERY_SYSTEM_ISDB_T && channel.deliverySystem != ChannelRecord.DELIVERY_SYSTEM_ISDB_S -> Diagnostic(key, "validate", "対象外 deliverySystem=${channel.deliverySystem}")
            inputId.isBlank() -> Diagnostic(key, "validate", "inputId が空です")
            else -> null
        }
    }

    private fun validate(program: ProgramRecord): Diagnostic? = when {
        program.eventId !in 0..0xffff -> Diagnostic(program.serviceKey, "program-validate", "不正な eventId=${program.eventId}")
        program.startTimeMillis <= 0L -> Diagnostic(program.serviceKey, "program-validate", "不正な start=${program.startTimeMillis}")
        program.durationMillis <= 0L -> Diagnostic(program.serviceKey, "program-validate", "不正な duration=${program.durationMillis}")
        checkedProgramEndTimeMillis(program) == null -> Diagnostic(program.serviceKey, "program-validate", "番組時刻が overflow しました")
        else -> null
    }

    private fun channelValues(channel: ChannelRecord, providerData: ByteArray): ContentValues = ContentValues().apply {
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

    private fun programValues(
        channelId: Long,
        program: ProgramRecord,
        clearAbsentOptionalColumns: Boolean,
        providerData: ByteArray,
    ): ContentValues = ContentValues().apply {
        put(TvContract.Programs.COLUMN_CHANNEL_ID, channelId)
        if (program.title.isBlank()) {
            if (clearAbsentOptionalColumns) putNull(TvContract.Programs.COLUMN_TITLE)
        } else put(TvContract.Programs.COLUMN_TITLE, program.title)
        put(TvContract.Programs.COLUMN_EVENT_ID, program.eventId)
        put(TvContract.Programs.COLUMN_START_TIME_UTC_MILLIS, program.startTimeMillis)
        put(TvContract.Programs.COLUMN_END_TIME_UTC_MILLIS, Math.addExact(program.startTimeMillis, program.durationMillis))
        if (program.shortDescription.isBlank()) {
            if (clearAbsentOptionalColumns) putNull(TvContract.Programs.COLUMN_SHORT_DESCRIPTION)
        } else put(TvContract.Programs.COLUMN_SHORT_DESCRIPTION, program.shortDescription)
        if (program.description.isBlank()) {
            if (clearAbsentOptionalColumns) putNull(TvContract.Programs.COLUMN_LONG_DESCRIPTION)
        } else put(TvContract.Programs.COLUMN_LONG_DESCRIPTION, program.description)
        val videoWidth = program.videoWidth?.takeIf { it > 0 }
        val videoHeight = program.videoHeight?.takeIf { it > 0 }
        if (videoWidth != null && videoHeight != null) {
            put(TvContract.Programs.COLUMN_VIDEO_WIDTH, videoWidth)
            put(TvContract.Programs.COLUMN_VIDEO_HEIGHT, videoHeight)
        } else if (clearAbsentOptionalColumns) {
            putNull(TvContract.Programs.COLUMN_VIDEO_WIDTH)
            putNull(TvContract.Programs.COLUMN_VIDEO_HEIGHT)
        }
        val audioLanguages = program.descriptors.components.audio
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
        } else put(TvContract.Programs.COLUMN_BROADCAST_GENRE, TvContract.Programs.Genres.encode(program.descriptors.broadcastGenre))
        val canonicalGenres = program.canonicalGenres.distinct().sorted()
        if (canonicalGenres.isEmpty()) {
            if (clearAbsentOptionalColumns) putNull(TvContract.Programs.COLUMN_CANONICAL_GENRE)
        } else put(TvContract.Programs.COLUMN_CANONICAL_GENRE, TvContract.Programs.Genres.encode(*canonicalGenres.toTypedArray()))
        if (program.contentRatings.isEmpty()) {
            if (clearAbsentOptionalColumns) putNull(TvContract.Programs.COLUMN_CONTENT_RATING)
        } else put(TvContract.Programs.COLUMN_CONTENT_RATING, program.contentRatings.distinct().sorted().joinToString(","))
        when (val scrambled = program.descriptors.scrambled) {
            null -> if (clearAbsentOptionalColumns) putNull(COLUMN_SCRAMBLED)
            else -> put(COLUMN_SCRAMBLED, if (scrambled) 1 else 0)
        }
        val seriesId = program.descriptors.series?.seriesId
        if (seriesId == null) {
            if (clearAbsentOptionalColumns) putNull(COLUMN_SERIES_ID)
        } else put(COLUMN_SERIES_ID, seriesId)
        // 投影契約は一意な単一series。複数記述子は根拠を保存し、ID・話数を選択しない。
        if (clearAbsentOptionalColumns) putNull(COLUMN_MULTI_SERIES_ID)
        val episodeNumber = program.descriptors.series?.episodeNumber
        if (episodeNumber == null || episodeNumber <= 0) {
            if (clearAbsentOptionalColumns) putNull(COLUMN_EPISODE_DISPLAY_NUMBER)
        } else put(COLUMN_EPISODE_DISPLAY_NUMBER, episodeNumber.toString())
        put(TvContract.Programs.COLUMN_INTERNAL_PROVIDER_DATA, providerData)
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

    companion object {
        /**
         * テスト専用 assertion 用に維持する フィールド 名。本番 provider-data は
         * ProviderDataBridge / Rust だけが生成・正規化する。
         */
        const val PROGRAM_KEY_FIELD = "programKey"
        internal const val EVENT_ID_REUSE_GUARD_MS = 24L * 60L * 60L * 1_000L
        const val COLUMN_SCRAMBLED = "scrambled"
        const val COLUMN_SERIES_ID = "series_id"
        const val COLUMN_MULTI_SERIES_ID = "multi_series_id"
        const val COLUMN_EPISODE_DISPLAY_NUMBER = "episode_display_number"
        private val SIGNATURE_COLUMNS = listOf(
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

        fun providerDataMatchesService(providerData: ByteArray?, serviceKey: ServiceKey?): Boolean {
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

        private fun MessageDigest.appendField(column: String, value: Any?) {
            val bytes = when (value) {
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
            val sorted = services.sortedWith(compareBy<PreparedServicePrograms> { it.serviceKey.originalNetworkId }
                .thenBy { it.serviceKey.transportStreamId }.thenBy { it.serviceKey.serviceId })
            digest.appendField("serviceCount", sorted.size)
            sorted.forEach { service ->
                digest.appendField("channelId", service.channelId)
                digest.appendField("rowCount", service.programs.size)
                service.programs.sortedWith(compareBy<Pair<ProgramRecord, ContentValues>> { it.first.eventId }
                    .thenBy { it.first.startTimeMillis }).forEach { (_, values) -> digest.appendRow(values) }
                digest.appendField("windowCount", service.windows.size)
                service.windows.sortedWith(compareBy<ProgramPublishCoordinator.EpgUpdateWindow> { it.windowStartMs }
                    .thenBy { it.windowEndMs }.thenBy { it.deletionAuthoritative }).forEach { window ->
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

        fun signatureForProgramForTest(channelId: Long, program: ProgramRecord): String {
            val writer = TvProviderWriter("test", object : ChannelStore {
                override fun findExistingChannelId(key: ServiceKey): Result<Long?> = Result.success(channelId)
                override fun insertChannel(values: ContentValues): Result<Long?> = Result.success(channelId)
                override fun updateChannel(channelId: Long, values: ContentValues): Result<Int> = Result.success(1)
            }, testOnly = true)
            return signatureForContentValues(
                writer.programValues(channelId, program, clearAbsentOptionalColumns = true, providerData = (ProviderDataBridge.buildProgramProviderData(program) as ProviderDataBridge.Success).bytes),
            )
        }

    }

    private fun fallbackName(key: ServiceKey): String = "service-${key.originalNetworkId}-${key.transportStreamId}-${key.serviceId}"

    private fun channelType(deliverySystem: String): String = when (deliverySystem) {
        ChannelRecord.DELIVERY_SYSTEM_ISDB_T -> TvContract.Channels.TYPE_ISDB_T
        ChannelRecord.DELIVERY_SYSTEM_ISDB_S -> TvContract.Channels.TYPE_ISDB_S
        else -> TvContract.Channels.TYPE_OTHER
    }

    private class AndroidTvProviderChannelStore(private val context: Context, private val inputId: String) : ChannelStore {
        override fun findExistingChannelId(key: ServiceKey): Result<Long?> = runCatching {
            val projection = arrayOf(TvContract.Channels._ID)
            val selection = "${TvContract.Channels.COLUMN_INPUT_ID}=? AND ${TvContract.Channels.COLUMN_ORIGINAL_NETWORK_ID}=? AND ${TvContract.Channels.COLUMN_TRANSPORT_STREAM_ID}=? AND ${TvContract.Channels.COLUMN_SERVICE_ID}=?"
            val args = arrayOf(inputId, key.originalNetworkId.toString(), key.transportStreamId.toString(), key.serviceId.toString())
            val cursor = context.contentResolver.query(TvContract.Channels.CONTENT_URI, projection, selection, args, null)
                ?: throw IllegalStateException("TvProvider channel query returned null cursor")
            cursor.use { if (it.moveToFirst()) it.getLong(0) else null }
        }.onFailure { Log.w(LogTags.TIS, "既存 channel 検索に失敗しました key=$key", it) }

        override fun insertChannel(values: ContentValues): Result<Long?> = runCatching {
            val uri: Uri? = context.contentResolver.insert(TvContract.Channels.CONTENT_URI, values)
            uri?.let { ContentUris.parseId(it) }
        }

        override fun updateChannel(channelId: Long, values: ContentValues): Result<Int> = runCatching {
            context.contentResolver.update(ContentUris.withAppendedId(TvContract.Channels.CONTENT_URI, channelId), values, null, null)
        }

        override fun listExistingChannels(): Result<List<ChannelRecord>> = runCatching {
            val projection = arrayOf(
                TvContract.Channels._ID,
                TvContract.Channels.COLUMN_ORIGINAL_NETWORK_ID,
                TvContract.Channels.COLUMN_TRANSPORT_STREAM_ID,
                TvContract.Channels.COLUMN_SERVICE_ID,
                TvContract.Channels.COLUMN_DISPLAY_NUMBER,
                TvContract.Channels.COLUMN_DISPLAY_NAME,
                TvContract.Channels.COLUMN_SERVICE_TYPE,
                TvContract.Channels.COLUMN_INTERNAL_PROVIDER_DATA,
            )
            val selection = "${TvContract.Channels.COLUMN_INPUT_ID}=?"
            val args = arrayOf(inputId)
            val out = mutableListOf<ChannelRecord>()
            val cursor = context.contentResolver.query(TvContract.Channels.CONTENT_URI, projection, selection, args, null)
                ?: throw IllegalStateException("TvProvider channel list query returned null cursor")
            cursor.use { cursor ->
                while (cursor.moveToNext()) {
                    val stored = ProviderDataBridge.decodeChannelProviderData(providerDataBytes(cursor, 7))
                    val serviceType = cursor.getString(6)?.toIntOrNull()?.takeIf { it in 0..0xff }
                    val rowServiceKey = ServiceKey(cursor.getInt(1), cursor.getInt(2), cursor.getInt(3))
                    if (stored == null || stored.serviceKey != rowServiceKey || serviceType == null) {
                        throw IllegalStateException("既存 channel の物理選局情報を復元できません id=${cursor.getLong(0)}")
                    } else {
                        out += ChannelRecord(
                            serviceKey = rowServiceKey,
                            serviceType = serviceType,
                            displayNumber = cursor.getString(4).orEmpty(),
                            displayName = cursor.getString(5).orEmpty().ifBlank {
                                "service-${rowServiceKey.originalNetworkId}-${rowServiceKey.transportStreamId}-${rowServiceKey.serviceId}"
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

        override fun indexExistingProgramsForWindow(channelId: Long, windowStartMs: Long, windowEndMs: Long): Result<Map<String, Long>> = runCatching {
            val projection = arrayOf(TvContract.Programs._ID, TvContract.Programs.COLUMN_INTERNAL_PROVIDER_DATA)
            val selection = "${TvContract.Programs.COLUMN_CHANNEL_ID}=? AND ${TvContract.Programs.COLUMN_END_TIME_UTC_MILLIS}>? AND ${TvContract.Programs.COLUMN_START_TIME_UTC_MILLIS}<?"
            val args = arrayOf(channelId.toString(), windowStartMs.toString(), windowEndMs.toString())
            val cursor = context.contentResolver.query(
                TvContract.Programs.CONTENT_URI,
                projection,
                selection,
                args,
                "${TvContract.Programs._ID} DESC",
            ) ?: throw IllegalStateException("TvProvider program index query returned null cursor")
            val out = linkedMapOf<String, Long>()
            cursor.use { c ->
                while (c.moveToNext()) {
                    val data = providerDataBytes(c, 1)
                    val key = TvProviderWriter.parseProgramKey(data)
                    if (key != null && key !in out) out[key] = c.getLong(0)
                }
            }
            out
        }

        override fun indexExistingProgramsForService(channelId: Long): Result<Map<String, Long>> = runCatching {
            val projection = arrayOf(TvContract.Programs._ID, TvContract.Programs.COLUMN_INTERNAL_PROVIDER_DATA)
            val selection = "${TvContract.Programs.COLUMN_CHANNEL_ID}=?"
            val args = arrayOf(channelId.toString())
            val cursor = context.contentResolver.query(
                TvContract.Programs.CONTENT_URI,
                projection,
                selection,
                args,
                "${TvContract.Programs._ID} DESC",
            ) ?: throw IllegalStateException("TvProvider service program index query returned null cursor")
            val out = linkedMapOf<String, Long>()
            cursor.use { c ->
                while (c.moveToNext()) {
                    val data = providerDataBytes(c, 1)
                    val key = TvProviderWriter.parseProgramKey(data)
                    if (key != null && key !in out) out[key] = c.getLong(0)
                }
            }
            out
        }

        private fun providerDataBytes(cursor: android.database.Cursor, index: Int): ByteArray? {
            return cursor.getBlob(index)
        }

        override fun insertProgram(values: ContentValues): Result<Long?> = runCatching {
            context.contentResolver.insert(TvContract.Programs.CONTENT_URI, values)?.let { ContentUris.parseId(it) }
        }

        override fun updateProgram(programId: Long, values: ContentValues): Result<Int> = runCatching {
            context.contentResolver.update(ContentUris.withAppendedId(TvContract.Programs.CONTENT_URI, programId), values, null, null)
        }

        override fun readCanonicalGenre(programId: Long): Result<String?> = runCatching {
            val uri = ContentUris.withAppendedId(TvContract.Programs.CONTENT_URI, programId)
            val cursor = context.contentResolver.query(uri, arrayOf(TvContract.Programs.COLUMN_CANONICAL_GENRE), null, null, null)
                ?: throw IllegalStateException("TvProvider ジャンル読戻しが null cursor を返しました")
            cursor.use {
                check(it.moveToFirst()) { "TvProvider ジャンル読戻しの対象行がありません" }
                it.getString(0)
            }
        }

        override fun deleteObsoletePrograms(channelId: Long, validProgramKeys: Set<String>, windowStartMs: Long, windowEndMs: Long): Result<Int> = runCatching {
            var deleted = 0
            val projection = arrayOf(TvContract.Programs._ID, TvContract.Programs.COLUMN_PACKAGE_NAME, TvContract.Programs.COLUMN_INTERNAL_PROVIDER_DATA)
            val selection = "${TvContract.Programs.COLUMN_CHANNEL_ID}=? AND ${TvContract.Programs.COLUMN_END_TIME_UTC_MILLIS}>? AND ${TvContract.Programs.COLUMN_START_TIME_UTC_MILLIS}<?"
            val args = arrayOf(channelId.toString(), windowStartMs.toString(), windowEndMs.toString())
            val cursor = context.contentResolver.query(TvContract.Programs.CONTENT_URI, projection, selection, args, null)
                ?: throw IllegalStateException("TvProvider obsolete program query returned null cursor")
            cursor.use { cursor ->
                while (cursor.moveToNext()) {
                    val id = cursor.getLong(0)
                    val ownerPackage = cursor.getString(1)
                    val key = TvProviderWriter.parseProgramKey(providerDataBytes(cursor, 2))
                    if (TvProviderWriter.shouldDeleteOwnedObsoleteProgramRow(ownerPackage, context.packageName, key, validProgramKeys)) {
                        deleted += context.contentResolver.delete(ContentUris.withAppendedId(TvContract.Programs.CONTENT_URI, id), null, null)
                    } else if (key == null) {
                        Log.w(LogTags.TIS, "所有元を確認できないProgram provider-data破損行は保持します id=$id owner=$ownerPackage")
                    }
                }
            }
            deleted
        }
    }
}
