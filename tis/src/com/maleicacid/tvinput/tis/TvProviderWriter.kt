package com.maleicacid.tvinput.tis

import android.content.ContentUris
import android.content.ContentValues
import android.content.Context
import android.media.tv.TvContract
import android.net.Uri
import android.util.Log
import com.maleicacid.tvinput.common.LogTags
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
        fun findExistingProgramId(channelId: Long, programKey: String): Result<Long?> = Result.success(null)
        fun indexExistingProgramsForWindow(channelId: Long, windowStartMs: Long, windowEndMs: Long): Result<Map<String, Long>> = Result.success(emptyMap())
        /**
         * Returns existing Program rows for the whole channel keyed by stable programKey.
         * This is intentionally wider than an EPG update window so that an event whose
         * start/end time moves outside the current window is still updated by stable
         * ONID/TSID/SID/event identity instead of being inserted as a duplicate.
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
            val values = channelValues(channel)
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
        Log.i(LogTags.TIS, "channel 登録結果 inputId=$inputId inserted=$inserted updated=$updated failures=${failures.size}")
        return UpsertResult(inserted, updated, failures)
    }

    fun upsertPrograms(programs: List<ProgramRecord>): UpsertResult = upsertProgramsForWindows(
        programs = programs,
        windows = programs.groupBy { it.serviceKey }.map { (key, values) ->
            ProgramPublishCoordinator.EpgUpdateWindow(
                serviceKey = key,
                windowStartMs = values.minOf { it.startTimeMillis },
                windowEndMs = values.maxOf { it.startTimeMillis + it.durationMillis },
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
            val validKeys = mutableSetOf<String>()
            servicePrograms.forEach { program ->
                val validation = validate(program)
                if (validation != null) { failures += validation; return@forEach }
                val key = programIdentity(program)
                validKeys += key
                val existingId = existingProgramsByKey[key]
                if (existingId == null) {
                    val values = programValues(channelId, program, key)
                    val insertResult = channelStore.insertProgram(values)
                    if (insertResult.isFailure) { failures += Diagnostic(serviceKey, "program-insert", insertResult.exceptionOrNull()?.message.orEmpty()); return@forEach }
                    val insertedId = insertResult.getOrNull()
                    if (insertedId == null) {
                        failures += Diagnostic(serviceKey, "program-insert", "provider が null URI を返しました")
                    } else {
                        inserted++
                    }
                } else {
                    val values = programValues(channelId, program, key)
                    val updateResult = channelStore.updateProgram(existingId, values)
                    if (updateResult.isFailure) { failures += Diagnostic(serviceKey, "program-update", updateResult.exceptionOrNull()?.message.orEmpty()); return@forEach }
                    if ((updateResult.getOrNull() ?: 0) <= 0) failures += Diagnostic(serviceKey, "program-update", "provider 更新対象行なし id=$existingId") else updated++
                }
            }
            if (failures.size == failureCountBeforeService) {
                serviceWindows.filter { it.deletionAuthoritative }.forEach { window ->
                    val keysForWindow = validKeys.filter { it in window.validProgramKeys }.toSet().ifEmpty { window.validProgramKeys }
                    val deleteResult = channelStore.deleteObsoletePrograms(channelId, keysForWindow, window.windowStartMs, window.windowEndMs)
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
        Log.i(LogTags.TIS, "program 登録結果 inputId=$inputId inserted=$inserted updated=$updated deleted=$deleted failures=${failures.size}")
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
        .onFailure { error -> Log.w(LogTags.TIS, "既存 channel 復元に失敗しました inputId=$inputId", error) }

    @Deprecated("production code must not collapse TvProvider query failure to an empty channel list", level = DeprecationLevel.ERROR)
    fun existingChannelsForTestOnly(): List<ChannelRecord> = existingChannelsResult().getOrElse { emptyList() }

    fun validateForTest(channel: ChannelRecord): Diagnostic? = validate(channel)
    fun channelValuesForTest(channel: ChannelRecord): ContentValues = channelValues(channel)
    fun programValuesForTest(channelId: Long, program: ProgramRecord): ContentValues =
        programValues(channelId, program, programIdentity(program), selectedProgramId = program.tvProviderProgramId ?: -1L)

    private fun validate(channel: ChannelRecord): Diagnostic? {
        val key = channel.serviceKey
        return when {
            key.serviceId !in 1..0xffff -> Diagnostic(key, "validate", "不正な service_id=${key.serviceId}")
            key.transportStreamId !in 0..0xffff -> Diagnostic(key, "validate", "不正な transport_stream_id=${key.transportStreamId}")
            key.originalNetworkId !in 0..0xffff -> Diagnostic(key, "validate", "不正な original_network_id=${key.originalNetworkId}")
            channel.frequencyHz <= 0L -> Diagnostic(key, "validate", "不正な frequencyHz=${channel.frequencyHz}")
            channel.deliverySystem != ChannelRecord.DELIVERY_SYSTEM_ISDB_T && channel.deliverySystem != ChannelRecord.DELIVERY_SYSTEM_ISDB_S -> Diagnostic(key, "validate", "対象外 deliverySystem=${channel.deliverySystem}")
            inputId.isBlank() -> Diagnostic(key, "validate", "inputId が空です")
            else -> null
        }
    }

    private fun validate(program: ProgramRecord): Diagnostic? = when {
        program.eventId !in 0..0xffff -> Diagnostic(program.serviceKey, "program-validate", "不正な eventId=${program.eventId}")
        program.startTimeMillis <= 0L -> Diagnostic(program.serviceKey, "program-validate", "不正な start=${program.startTimeMillis}")
        program.durationMillis <= 0L -> Diagnostic(program.serviceKey, "program-validate", "不正な duration=${program.durationMillis}")
        program.title.isBlank() -> Diagnostic(program.serviceKey, "program-validate", "title が空です")
        else -> null
    }

    private fun channelValues(channel: ChannelRecord): ContentValues = ContentValues().apply {
        put(TvContract.Channels.COLUMN_INPUT_ID, inputId)
        put(TvContract.Channels.COLUMN_TYPE, channelType(channel.deliverySystem))
        put(TvContract.Channels.COLUMN_SERVICE_TYPE, TvContract.Channels.SERVICE_TYPE_AUDIO_VIDEO)
        put(TvContract.Channels.COLUMN_DISPLAY_NUMBER, channel.displayNumber.ifBlank { channel.serviceKey.serviceId.toString() })
        put(TvContract.Channels.COLUMN_DISPLAY_NAME, channel.displayName.ifBlank { fallbackName(channel.serviceKey) })
        put(TvContract.Channels.COLUMN_ORIGINAL_NETWORK_ID, channel.serviceKey.originalNetworkId)
        put(TvContract.Channels.COLUMN_TRANSPORT_STREAM_ID, channel.serviceKey.transportStreamId)
        put(TvContract.Channels.COLUMN_SERVICE_ID, channel.serviceKey.serviceId)
        put(TvContract.Channels.COLUMN_SEARCHABLE, 1)
        put(TvContract.Channels.COLUMN_BROWSABLE, 1)
        put(TvContract.Channels.COLUMN_INTERNAL_PROVIDER_DATA, channelProviderDataBytes(channel))
    }

    private fun programValues(channelId: Long, program: ProgramRecord, identity: String, @Suppress("UNUSED_PARAMETER") selectedProgramId: Long = -1L): ContentValues = ContentValues().apply {
        put(TvContract.Programs.COLUMN_CHANNEL_ID, channelId)
        put(TvContract.Programs.COLUMN_TITLE, program.title)
        put(TvContract.Programs.COLUMN_EVENT_ID, program.eventId)
        put(TvContract.Programs.COLUMN_START_TIME_UTC_MILLIS, program.startTimeMillis)
        put(TvContract.Programs.COLUMN_END_TIME_UTC_MILLIS, program.startTimeMillis + program.durationMillis)
        put(TvContract.Programs.COLUMN_SHORT_DESCRIPTION, program.shortDescription)
        put(TvContract.Programs.COLUMN_LONG_DESCRIPTION, program.description)
        if (program.audioLanguage.isNullOrBlank()) putNull(TvContract.Programs.COLUMN_AUDIO_LANGUAGE) else put(TvContract.Programs.COLUMN_AUDIO_LANGUAGE, program.audioLanguage)
        if (program.broadcastGenre.isNullOrBlank()) putNull(TvContract.Programs.COLUMN_BROADCAST_GENRE) else put(TvContract.Programs.COLUMN_BROADCAST_GENRE, TvContract.Programs.Genres.encode(program.broadcastGenre))
        if (program.contentRatings.isEmpty()) putNull(TvContract.Programs.COLUMN_CONTENT_RATING) else put(TvContract.Programs.COLUMN_CONTENT_RATING, program.contentRatings.distinct().sorted().joinToString(","))
        put(TvContract.Programs.COLUMN_INTERNAL_PROVIDER_FLAG1, if (program.requiresCas) 1 else 0)
        put(TvContract.Programs.COLUMN_INTERNAL_PROVIDER_FLAG2, if (program.unsupportedCas) 1 else 0)
        put(TvContract.Programs.COLUMN_INTERNAL_PROVIDER_FLAG3, if (program.clearLivePlaybackSupported) 1 else 0)
        put(TvContract.Programs.COLUMN_INTERNAL_PROVIDER_FLAG4, if (program.epgPublishable) 1 else 0)
        put(TvContract.Programs.COLUMN_INTERNAL_PROVIDER_DATA, ProviderDataBridge.buildProgramProviderData(program.copy(tvProviderProgramId = null)).json.toByteArray(Charsets.UTF_8))
    }

    private fun programIdentity(program: ProgramRecord): String = ProviderDataBridge.buildProgramKey(program)

    companion object {
        /**
         * Test-only field name kept for legacy assertions. Production provider-data
         * is generated and normalized only by ProviderDataBridge / Rust.
         */
        const val PROGRAM_KEY_FIELD = "programKeyB64"
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
            TvContract.Programs.COLUMN_CONTENT_RATING,
            TvContract.Programs.COLUMN_INTERNAL_PROVIDER_DATA,
            TvContract.Programs.COLUMN_INTERNAL_PROVIDER_FLAG1,
            TvContract.Programs.COLUMN_INTERNAL_PROVIDER_FLAG2,
            TvContract.Programs.COLUMN_INTERNAL_PROVIDER_FLAG3,
            TvContract.Programs.COLUMN_INTERNAL_PROVIDER_FLAG4,
        )

        fun programKeyForTest(program: ProgramRecord): String = ProviderDataBridge.buildProgramKey(program)

        fun programProviderDataForTest(program: ProgramRecord): String =
            ProviderDataBridge.buildProgramProviderData(program).json

        fun parseProgramKey(providerData: String?): String? = ProviderDataBridge.extractProgramKey(providerData)

        fun providerDataMatchesService(providerData: String?, serviceKey: ServiceKey?): Boolean {
            if (serviceKey == null) return true
            val programKey = parseProgramKey(providerData) ?: return false
            return programKey.startsWith(
                "onid=${serviceKey.originalNetworkId};tsid=${serviceKey.transportStreamId};sid=${serviceKey.serviceId};"
            )
        }

        fun providerDataWithCurrentProgramDiagnostics(
            providerData: String?,
            overlapCount: Int,
            selectedProgramId: Long,
            selectionRule: String,
        ): String = ProviderDataBridge.appendCurrentProgramDiagnostics(providerData, overlapCount, selectedProgramId, selectionRule).json

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
            return signatureForContentValues(writer.programValues(channelId, program, ProviderDataBridge.buildProgramKey(program), selectedProgramId = program.tvProviderProgramId ?: -1L))
        }

        fun parseChannelProviderData(providerData: String?): Map<String, String> {
            val extracted = ProviderDataBridge.extractChannelTuneKey(providerData)
            if (extracted != null) {
                return linkedMapOf(
                    "originalNetworkId" to extracted.serviceKey.originalNetworkId.toString(),
                    "transportStreamId" to extracted.serviceKey.transportStreamId.toString(),
                    "serviceId" to extracted.serviceKey.serviceId.toString(),
                    "system" to extracted.system,
                    "frequencyHz" to extracted.frequencyHz.toString(),
                    "streamSelectorType" to extracted.streamSelector.type.name,
                    "streamSelectorValue" to (extracted.streamSelector.value?.toString().orEmpty()),
                    "physicalChannel" to (extracted.physicalChannel?.toString().orEmpty()),
                    "backendHint" to extracted.backendHint.orEmpty(),
                    "satelliteBand" to extracted.satelliteBand.orEmpty(),
                    "remoteControlKeyId" to (extracted.remoteControlKeyId?.toString().orEmpty()),
                    "requiresCas" to extracted.requiresCas.toString(),
                    "unsupportedCas" to extracted.unsupportedCas.toString(),
                    "clearLivePlaybackSupported" to extracted.clearLivePlaybackSupported.toString(),
                    "channelRegistrationReady" to extracted.channelRegistrationReady.toString(),
                    "epgPublishable" to extracted.epgPublishable.toString(),
                )
            }
            val raw = providerData?.takeIf { it.isNotBlank() } ?: return emptyMap()
            return raw.split(';').mapNotNull { part ->
                val i = part.indexOf('=')
                if (i <= 0) null else part.substring(0, i) to part.substring(i + 1)
            }.toMap()
        }


    }

    private fun fallbackName(key: ServiceKey): String = "service-${key.originalNetworkId}-${key.transportStreamId}-${key.serviceId}"

    private fun channelType(deliverySystem: String): String = when (deliverySystem) {
        ChannelRecord.DELIVERY_SYSTEM_ISDB_T -> TvContract.Channels.TYPE_ISDB_T
        ChannelRecord.DELIVERY_SYSTEM_ISDB_S -> TvContract.Channels.TYPE_ISDB_S
        else -> TvContract.Channels.TYPE_OTHER
    }

    private fun channelProviderDataBytes(channel: ChannelRecord): ByteArray =
        ProviderDataBridge.buildChannelProviderData(channel).json.toByteArray(Charsets.UTF_8)

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
                TvContract.Channels.COLUMN_INTERNAL_PROVIDER_DATA,
            )
            val selection = "${TvContract.Channels.COLUMN_INPUT_ID}=?"
            val args = arrayOf(inputId)
            val out = mutableListOf<ChannelRecord>()
            val cursor = context.contentResolver.query(TvContract.Channels.CONTENT_URI, projection, selection, args, null)
                ?: throw IllegalStateException("TvProvider channel list query returned null cursor")
            cursor.use { cursor ->
                while (cursor.moveToNext()) {
                    val providerData = cursor.getBlob(6)?.toString(Charsets.UTF_8)
                    val stored = TvProviderWriter.parseChannelProviderData(providerData)
                    val frequencyHz = stored["frequencyHz"]?.toLongOrNull()
                    val system = stored["system"]
                    if (frequencyHz == null || system == null) {
                        Log.w(LogTags.TIS, "既存 channel の物理選局情報を復元できないため boot/background sync から除外します id=${cursor.getLong(0)}")
                    } else {
                        val onid = cursor.getInt(1)
                        val tsid = cursor.getInt(2)
                        val sid = cursor.getInt(3)
                        val serviceKey = ServiceKey(onid, tsid, sid)
                        val streamSelector = runCatching { StreamSelector.fromStored(stored["streamSelectorType"], stored["streamSelectorValue"]?.takeIf { it.isNotBlank() }) }.getOrDefault(StreamSelector.NONE)
                        out += ChannelRecord(
                            serviceKey = serviceKey,
                            displayNumber = cursor.getString(4).orEmpty(),
                            displayName = cursor.getString(5).orEmpty().ifBlank { fallbackName(serviceKey) },
                            frequencyHz = frequencyHz,
                            tvProviderChannelId = cursor.getLong(0),
                            deliverySystem = system,
                            streamSelector = streamSelector,
                            physicalChannel = stored["physicalChannel"]?.toIntOrNull(),
                            backendHint = stored["backendHint"]?.takeIf { it.isNotBlank() },
                            satelliteBand = stored["satelliteBand"]?.takeIf { it.isNotBlank() },
                            remoteControlKeyId = stored["remoteControlKeyId"]?.toIntOrNull(),
                            requiresCas = stored["requiresCas"] == "true",
                            unsupportedCas = stored["unsupportedCas"] == "true",
                            clearLivePlaybackSupported = stored["clearLivePlaybackSupported"] == "true",
                            channelRegistrationReady = stored["channelRegistrationReady"] == "true",
                            epgPublishable = stored["epgPublishable"] == "true",
                        )
                    }
                }
            }
            out
        }

        override fun findExistingProgramId(channelId: Long, programKey: String): Result<Long?> = runCatching {
            val projection = arrayOf(TvContract.Programs._ID, TvContract.Programs.COLUMN_INTERNAL_PROVIDER_DATA)
            val selection = "${TvContract.Programs.COLUMN_CHANNEL_ID}=?"
            val args = arrayOf(channelId.toString())
            val cursor = context.contentResolver.query(TvContract.Programs.CONTENT_URI, projection, selection, args, null)
                ?: throw IllegalStateException("TvProvider program query returned null cursor")
            cursor.use { cursor ->
                while (cursor.moveToNext()) {
                    val data = cursor.getBlob(1)?.toString(Charsets.UTF_8)
                    if (TvProviderWriter.parseProgramKey(data) == programKey) return@use cursor.getLong(0)
                }
                null
            }
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
                    val data = c.getBlob(1)?.toString(Charsets.UTF_8)
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
                    val data = c.getBlob(1)?.toString(Charsets.UTF_8)
                    val key = TvProviderWriter.parseProgramKey(data)
                    if (key != null && key !in out) out[key] = c.getLong(0)
                }
            }
            out
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
                    val key = TvProviderWriter.parseProgramKey(cursor.getBlob(1)?.toString(Charsets.UTF_8))
                    if (key == null || key !in validProgramKeys) {
                        deleted += context.contentResolver.delete(ContentUris.withAppendedId(TvContract.Programs.CONTENT_URI, id), null, null)
                    }
                }
            }
            deleted
        }
    }
}
