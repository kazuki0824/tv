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
    data class UpsertResult(
        val inserted: Int,
        val updated: Int,
        val failures: List<Diagnostic>,
        val deleted: Int = 0,
        val succeededServiceKeys: Set<ServiceKey> = emptySet(),
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
            val values = runCatching { channelValues(channel) }.getOrElse { error ->
                failures += Diagnostic(channel.serviceKey, "provider-data", error.message.orEmpty())
                return@forEach
            }
            val existingIdResult = channelStore.findExistingChannelId(channel.serviceKey)
            if (existingIdResult.isFailure) { failures += Diagnostic(channel.serviceKey, "query", existingIdResult.exceptionOrNull()?.message.orEmpty()); return@forEach }
            val existingId = existingIdResult.getOrNull()
            if (existingId == null) {
                val insertedIdResult = channelStore.insertChannel(values)
                if (insertedIdResult.isFailure) { failures += Diagnostic(channel.serviceKey, "insert", insertedIdResult.exceptionOrNull()?.message.orEmpty()); return@forEach }
                if (insertedIdResult.getOrNull() == null) failures += Diagnostic(channel.serviceKey, "insert", "provider が null URI を返しました") else inserted++
            } else {
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

    fun upsertProgramsForWindows(programs: List<ProgramRecord>, windows: List<ProgramPublishCoordinator.EpgUpdateWindow>): UpsertResult {
        var inserted = 0
        var updated = 0
        var deleted = 0
        val failures = mutableListOf<Diagnostic>()
        val succeededServiceKeys = linkedSetOf<ServiceKey>()
        val programsByChannel = programs.groupBy { it.serviceKey }
        val windowsByChannel = windows.groupBy { it.serviceKey }
        (programsByChannel.keys + windowsByChannel.keys).forEach { serviceKey ->
            val failureCountBeforeService = failures.size
            val channelIdResult = channelStore.findExistingChannelId(serviceKey)
            if (channelIdResult.isFailure) { failures += Diagnostic(serviceKey, "program-channel-query", channelIdResult.exceptionOrNull()?.message.orEmpty()); return@forEach }
            val channelId = channelIdResult.getOrNull()
            if (channelId == null) { failures += Diagnostic(serviceKey, "program-channel-query", "program 登録対象 channel がありません"); return@forEach }
            val servicePrograms = programsByChannel[serviceKey].orEmpty().sortedBy { it.startTimeMillis }
            val serviceWindows = windowsByChannel[serviceKey].orEmpty()
            val existingProgramsByKey = if (servicePrograms.isNotEmpty()) {
                val indexResult = channelStore.indexExistingProgramsForService(channelId)
                if (indexResult.isFailure) {
                    failures += Diagnostic(serviceKey, "program-index-query", indexResult.exceptionOrNull()?.message.orEmpty())
                    return@forEach
                }
                indexResult.getOrThrow()
            } else {
                emptyMap()
            }
            servicePrograms.forEach { program ->
                val validation = validate(program)
                if (validation != null) { failures += validation; return@forEach }
                val key = programIdentity(program)
                val existingId = existingProgramsByKey[key]
                val values = runCatching { programValues(channelId, program) }.getOrElse { error ->
                    failures += Diagnostic(serviceKey, "program-provider-data", error.message.orEmpty())
                    return@forEach
                }
                if (existingId == null) {
                    val insertResult = channelStore.insertProgram(values)
                    if (insertResult.isFailure) { failures += Diagnostic(serviceKey, "program-insert", insertResult.exceptionOrNull()?.message.orEmpty()); return@forEach }
                    val insertedId = insertResult.getOrNull()
                    if (insertedId == null) {
                        failures += Diagnostic(serviceKey, "program-insert", "provider が null URI を返しました")
                    } else {
                        inserted++
                    }
                } else {
                    val updateResult = channelStore.updateProgram(existingId, values)
                    if (updateResult.isFailure) { failures += Diagnostic(serviceKey, "program-update", updateResult.exceptionOrNull()?.message.orEmpty()); return@forEach }
                    if ((updateResult.getOrNull() ?: 0) <= 0) failures += Diagnostic(serviceKey, "program-update", "provider 更新対象行なし id=$existingId") else updated++
                }
            }
            if (failures.size == failureCountBeforeService) {
                serviceWindows.filter { it.deletionAuthoritative }.forEach { window ->
                    val deleteResult = channelStore.deleteObsoletePrograms(channelId, window.validProgramKeys, window.windowStartMs, window.windowEndMs)
                    if (deleteResult.isFailure) {
                        failures += Diagnostic(serviceKey, "program-delete-obsolete", deleteResult.exceptionOrNull()?.message.orEmpty())
                    } else {
                        deleted += deleteResult.getOrNull() ?: 0
                    }
                }
            }
            if (failures.size == failureCountBeforeService) {
                succeededServiceKeys += serviceKey
            }
        }
        Log.i(LogTags.TIS, "program登録結果 inputId=$inputId inserted=$inserted updated=$updated deleted=$deleted failures=${failures.size}")
        return UpsertResult(inserted, updated, failures, deleted = deleted, succeededServiceKeys = succeededServiceKeys)
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
    fun channelValuesForTest(channel: ChannelRecord): ContentValues = channelValues(channel)
    fun programValuesForTest(channelId: Long, program: ProgramRecord): ContentValues =
        programValues(channelId, program)

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
        program.title.isBlank() -> Diagnostic(program.serviceKey, "program-validate", "title が空です")
        else -> null
    }

    private fun channelValues(channel: ChannelRecord): ContentValues = ContentValues().apply {
        put(TvContract.Channels.COLUMN_INPUT_ID, inputId)
        put(TvContract.Channels.COLUMN_TYPE, channelType(channel.deliverySystem))
        put(TvContract.Channels.COLUMN_SERVICE_TYPE, channel.serviceType.toString())
        put(TvContract.Channels.COLUMN_DISPLAY_NUMBER, channel.displayNumber.ifBlank { channel.serviceKey.serviceId.toString() })
        put(TvContract.Channels.COLUMN_DISPLAY_NAME, channel.displayName.ifBlank { fallbackName(channel.serviceKey) })
        put(TvContract.Channels.COLUMN_ORIGINAL_NETWORK_ID, channel.serviceKey.originalNetworkId)
        put(TvContract.Channels.COLUMN_TRANSPORT_STREAM_ID, channel.serviceKey.transportStreamId)
        put(TvContract.Channels.COLUMN_SERVICE_ID, channel.serviceKey.serviceId)
        put(TvContract.Channels.COLUMN_SEARCHABLE, 1)
        put(TvContract.Channels.COLUMN_BROWSABLE, 1)
        put(TvContract.Channels.COLUMN_INTERNAL_PROVIDER_DATA, channelProviderDataBytes(channel))
    }

    private fun programValues(channelId: Long, program: ProgramRecord): ContentValues = ContentValues().apply {
        put(TvContract.Programs.COLUMN_CHANNEL_ID, channelId)
        put(TvContract.Programs.COLUMN_TITLE, program.title)
        put(TvContract.Programs.COLUMN_EVENT_ID, program.eventId)
        put(TvContract.Programs.COLUMN_START_TIME_UTC_MILLIS, program.startTimeMillis)
        put(TvContract.Programs.COLUMN_END_TIME_UTC_MILLIS, Math.addExact(program.startTimeMillis, program.durationMillis))
        put(TvContract.Programs.COLUMN_SHORT_DESCRIPTION, program.shortDescription)
        if (program.description.isBlank()) putNull(TvContract.Programs.COLUMN_LONG_DESCRIPTION) else put(TvContract.Programs.COLUMN_LONG_DESCRIPTION, program.description)
        val audioLanguage = program.descriptors.components.audio
            .asSequence()
            .flatMap { sequenceOf(it.language, it.secondLanguage) }
            .firstOrNull { !it.isNullOrBlank() }
        if (audioLanguage == null) putNull(TvContract.Programs.COLUMN_AUDIO_LANGUAGE) else put(TvContract.Programs.COLUMN_AUDIO_LANGUAGE, audioLanguage)
        if (program.descriptors.broadcastGenre.isNullOrBlank()) putNull(TvContract.Programs.COLUMN_BROADCAST_GENRE) else put(TvContract.Programs.COLUMN_BROADCAST_GENRE, TvContract.Programs.Genres.encode(program.descriptors.broadcastGenre))
        val canonicalGenres = program.canonicalGenres.distinct().sorted()
        if (canonicalGenres.isEmpty()) putNull(TvContract.Programs.COLUMN_CANONICAL_GENRE) else put(TvContract.Programs.COLUMN_CANONICAL_GENRE, TvContract.Programs.Genres.encode(*canonicalGenres.toTypedArray()))
        if (program.contentRatings.isEmpty()) putNull(TvContract.Programs.COLUMN_CONTENT_RATING) else put(TvContract.Programs.COLUMN_CONTENT_RATING, program.contentRatings.distinct().sorted().joinToString(","))
        when (val scrambled = program.descriptors.scrambled) {
            null -> putNull(COLUMN_SCRAMBLED)
            else -> put(COLUMN_SCRAMBLED, if (scrambled) 1 else 0)
        }
        val seriesId = program.descriptors.series?.seriesId
        if (seriesId == null) putNull(COLUMN_SERIES_ID) else put(COLUMN_SERIES_ID, seriesId)
        // ARIB series descriptor はこのモデルでは単一系列なので、複数系列用列には投影しない。
        putNull(COLUMN_MULTI_SERIES_ID)
        val episodeNumber = program.descriptors.series?.episodeNumber
        if (episodeNumber == null || episodeNumber <= 0) putNull(COLUMN_EPISODE_DISPLAY_NUMBER) else put(COLUMN_EPISODE_DISPLAY_NUMBER, episodeNumber.toString())
        put(TvContract.Programs.COLUMN_INTERNAL_PROVIDER_DATA, ProviderDataBridge.buildProgramProviderData(program.copy(tvProviderProgramId = null)).bytes)
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
            ProviderDataBridge.buildProgramProviderData(program).json

        fun parseProgramKey(providerData: ByteArray?): String? = ProviderDataBridge.extractProgramKey(providerData)

        fun providerDataMatchesService(providerData: ByteArray?, serviceKey: ServiceKey?): Boolean {
            if (serviceKey == null) return true
            val key = ProviderDataBridge.extractProgramKeyResult(providerData) ?: return false
            return key.serviceKey == serviceKey
        }

        fun signatureForContentValues(values: ContentValues): String {
            val bytes = buildString {
                SIGNATURE_COLUMNS.forEach { column ->
                    val value = values.get(column)
                    val valueBytes = when (value) {
                        null -> ByteArray(0)
                        is ByteArray -> value
                        is Number, is Boolean -> value.toString().toByteArray(Charsets.UTF_8)
                        else -> value.toString().toByteArray(Charsets.UTF_8)
                    }
                    append(column).append('\u0000').append(valueBytes.size).append('\u0000')
                    append(String(valueBytes, Charsets.ISO_8859_1)).append('\n')
                }
            }.toByteArray(Charsets.ISO_8859_1)
            return MessageDigest.getInstance("SHA-256").digest(bytes).joinToString("") { "%02x".format(it) }
        }

        fun signatureForProgramForTest(channelId: Long, program: ProgramRecord): String {
            val writer = TvProviderWriter("test", object : ChannelStore {
                override fun findExistingChannelId(key: ServiceKey): Result<Long?> = Result.success(channelId)
                override fun insertChannel(values: ContentValues): Result<Long?> = Result.success(channelId)
                override fun updateChannel(channelId: Long, values: ContentValues): Result<Int> = Result.success(1)
            }, testOnly = true)
            return signatureForContentValues(writer.programValues(channelId, program))
        }

    }

    private fun fallbackName(key: ServiceKey): String = "service-${key.originalNetworkId}-${key.transportStreamId}-${key.serviceId}"

    private fun channelType(deliverySystem: String): String = when (deliverySystem) {
        ChannelRecord.DELIVERY_SYSTEM_ISDB_T -> TvContract.Channels.TYPE_ISDB_T
        ChannelRecord.DELIVERY_SYSTEM_ISDB_S -> TvContract.Channels.TYPE_ISDB_S
        else -> TvContract.Channels.TYPE_OTHER
    }

    private fun channelProviderDataBytes(channel: ChannelRecord): ByteArray =
        ProviderDataBridge.buildChannelProviderData(channel.copy(inputId = inputId)).bytes

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
                            requiresCas = false,
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
            val cursor = context.contentResolver.query(TvContract.Programs.CONTENT_URI, projection, selection, args, null)
                ?: throw IllegalStateException("TvProvider program index query returned null cursor")
            val out = linkedMapOf<String, Long>()
            cursor.use { c ->
                while (c.moveToNext()) {
                    val data = providerDataBytes(c, 1)
                    val key = TvProviderWriter.parseProgramKey(data)
                    if (key != null) out[key] = c.getLong(0)
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
            return runCatching { cursor.getBlob(index) }.getOrNull()
                ?: runCatching { cursor.getString(index)?.toByteArray(Charsets.UTF_8) }.getOrNull()
        }

        override fun insertProgram(values: ContentValues): Result<Long?> = runCatching {
            context.contentResolver.insert(TvContract.Programs.CONTENT_URI, values)?.let { ContentUris.parseId(it) }
        }

        override fun updateProgram(programId: Long, values: ContentValues): Result<Int> = runCatching {
            context.contentResolver.update(ContentUris.withAppendedId(TvContract.Programs.CONTENT_URI, programId), values, null, null)
        }

        override fun deleteObsoletePrograms(channelId: Long, validProgramKeys: Set<String>, windowStartMs: Long, windowEndMs: Long): Result<Int> = runCatching {
            var deleted = 0
            val projection = arrayOf(TvContract.Programs._ID, TvContract.Programs.COLUMN_INTERNAL_PROVIDER_DATA)
            val selection = "${TvContract.Programs.COLUMN_CHANNEL_ID}=? AND ${TvContract.Programs.COLUMN_END_TIME_UTC_MILLIS}>? AND ${TvContract.Programs.COLUMN_START_TIME_UTC_MILLIS}<?"
            val args = arrayOf(channelId.toString(), windowStartMs.toString(), windowEndMs.toString())
            val cursor = context.contentResolver.query(TvContract.Programs.CONTENT_URI, projection, selection, args, null)
                ?: throw IllegalStateException("TvProvider obsolete program query returned null cursor")
            cursor.use { cursor ->
                while (cursor.moveToNext()) {
                    val id = cursor.getLong(0)
                    val key = TvProviderWriter.parseProgramKey(providerDataBytes(cursor, 1))
                    if (key == null) {
                        Log.w(LogTags.TIS, "Program provider-data から安定キーを抽出できないため削除を保留します id=$id")
                    } else if (key !in validProgramKeys) {
                        deleted += context.contentResolver.delete(ContentUris.withAppendedId(TvContract.Programs.CONTENT_URI, id), null, null)
                    }
                }
            }
            deleted
        }
    }
}
