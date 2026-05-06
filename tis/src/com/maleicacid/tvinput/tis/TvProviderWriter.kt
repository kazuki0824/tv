package com.maleicacid.tvinput.tis

import android.content.ContentUris
import android.content.ContentValues
import android.content.Context
import android.media.tv.TvContract
import android.net.Uri
import android.util.Log
import com.maleicacid.tvinput.common.LogTags
import com.maleicacid.tvinput.common.ServiceKey
import com.maleicacid.tvinput.db.ChannelRecord
import com.maleicacid.tvinput.db.ProgramRecord
import java.util.Base64

class TvProviderWriter private constructor(
    private val inputId: String,
    private val channelStore: ChannelStore,
) {
    constructor(context: Context, inputId: String) : this(inputId, AndroidTvProviderChannelStore(context, inputId))

    constructor(inputId: String, channelStore: ChannelStore, @Suppress("UNUSED_PARAMETER") testOnly: Boolean) : this(inputId, channelStore)

    data class Diagnostic(val serviceKey: ServiceKey?, val operation: String, val message: String)
    data class UpsertResult(val inserted: Int, val updated: Int, val failures: List<Diagnostic>)

    interface ChannelStore {
        fun findExistingChannelId(key: ServiceKey): Result<Long?>
        fun insertChannel(values: ContentValues): Result<Long?>
        fun updateChannel(channelId: Long, values: ContentValues): Result<Int>
        fun findExistingProgramId(channelId: Long, programKey: String): Result<Long?> = Result.success(null)
        fun insertProgram(values: ContentValues): Result<Long?> = Result.failure(UnsupportedOperationException("この store は program insert に対応しません"))
        fun updateProgram(programId: Long, values: ContentValues): Result<Int> = Result.failure(UnsupportedOperationException("この store は program update に対応しません"))
        fun deleteObsoletePrograms(channelId: Long, validProgramKeys: Set<String>, windowStartMs: Long, windowEndMs: Long): Result<Int> = Result.success(0)
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

    fun upsertPrograms(programs: List<ProgramRecord>): UpsertResult {
        var inserted = 0
        var updated = 0
        val failures = mutableListOf<Diagnostic>()
        val byChannel = programs.groupBy { it.serviceKey }
        byChannel.forEach { (serviceKey, channelPrograms) ->
            val channelIdResult = channelStore.findExistingChannelId(serviceKey)
            if (channelIdResult.isFailure) { failures += Diagnostic(serviceKey, "program-channel-query", channelIdResult.exceptionOrNull()?.message.orEmpty()); return@forEach }
            val channelId = channelIdResult.getOrNull()
            if (channelId == null) { failures += Diagnostic(serviceKey, "program-channel-query", "program 登録対象 channel がありません"); return@forEach }
            val validKeys = mutableSetOf<String>()
            channelPrograms.sortedBy { it.startTimeMillis }.forEach { program ->
                val validation = validate(program)
                if (validation != null) { failures += validation; return@forEach }
                val key = programIdentity(program)
                validKeys += key
                val values = programValues(channelId, program, key)
                val existingResult = channelStore.findExistingProgramId(channelId, key)
                if (existingResult.isFailure) { failures += Diagnostic(serviceKey, "program-query", existingResult.exceptionOrNull()?.message.orEmpty()); return@forEach }
                val existingId = existingResult.getOrNull()
                if (existingId == null) {
                    val insertResult = channelStore.insertProgram(values)
                    if (insertResult.isFailure) { failures += Diagnostic(serviceKey, "program-insert", insertResult.exceptionOrNull()?.message.orEmpty()); return@forEach }
                    if (insertResult.getOrNull() == null) failures += Diagnostic(serviceKey, "program-insert", "provider が null URI を返しました") else inserted++
                } else {
                    val updateResult = channelStore.updateProgram(existingId, values)
                    if (updateResult.isFailure) { failures += Diagnostic(serviceKey, "program-update", updateResult.exceptionOrNull()?.message.orEmpty()); return@forEach }
                    if ((updateResult.getOrNull() ?: 0) <= 0) failures += Diagnostic(serviceKey, "program-update", "provider 更新対象行なし id=$existingId") else updated++
                }
            }
            val start = channelPrograms.minOfOrNull { it.startTimeMillis } ?: return@forEach
            val end = channelPrograms.maxOfOrNull { it.startTimeMillis + it.durationMillis } ?: return@forEach
            channelStore.deleteObsoletePrograms(channelId, validKeys, start, end).onFailure { failures += Diagnostic(serviceKey, "program-cleanup", it.message.orEmpty()) }
        }
        Log.i(LogTags.TIS, "program 登録結果 inputId=$inputId inserted=$inserted updated=$updated failures=${failures.size}")
        return UpsertResult(inserted, updated, failures)
    }

    internal fun validateForTest(channel: ChannelRecord): Diagnostic? = validate(channel)
    internal fun channelValuesForTest(channel: ChannelRecord): ContentValues = channelValues(channel)
    internal fun programValuesForTest(channelId: Long, program: ProgramRecord): ContentValues = programValues(channelId, program, programIdentity(program))

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
        put(TvContract.Channels.COLUMN_INTERNAL_PROVIDER_DATA, identityBlob(channel))
    }

    private fun programValues(channelId: Long, program: ProgramRecord, identity: String): ContentValues = ContentValues().apply {
        put(TvContract.Programs.COLUMN_CHANNEL_ID, channelId)
        put(TvContract.Programs.COLUMN_TITLE, program.title)
        put(TvContract.Programs.COLUMN_EVENT_ID, program.eventId)
        put(TvContract.Programs.COLUMN_START_TIME_UTC_MILLIS, program.startTimeMillis)
        put(TvContract.Programs.COLUMN_END_TIME_UTC_MILLIS, program.startTimeMillis + program.durationMillis)
        put(TvContract.Programs.COLUMN_SHORT_DESCRIPTION, program.shortDescription)
        put(TvContract.Programs.COLUMN_LONG_DESCRIPTION, program.description)
        program.audioLanguage?.takeIf { it.isNotBlank() }?.let { put(TvContract.Programs.COLUMN_AUDIO_LANGUAGE, it) }
        program.canonicalGenre?.takeIf { it.isNotBlank() }?.let { put(TvContract.Programs.COLUMN_CANONICAL_GENRE, it) }
        put(TvContract.Programs.COLUMN_INTERNAL_PROVIDER_DATA, programProviderData(identity, program).toByteArray(Charsets.UTF_8))
    }

    private fun programIdentity(program: ProgramRecord): String {
        val key = program.serviceKey
        return program.stableIdentity.ifBlank { "onid=${key.originalNetworkId};tsid=${key.transportStreamId};sid=${key.serviceId};event=${program.eventId}" }
    }

    internal fun programProviderDataForTest(programKey: String): String = programProviderData(programKey)

    companion object {
        const val PROGRAM_KEY_FIELD = "programKeyB64"
        fun programProviderData(programKey: String): String {
            val encoded = Base64.getUrlEncoder().withoutPadding().encodeToString(programKey.toByteArray(Charsets.UTF_8))
            return "$PROGRAM_KEY_FIELD=$encoded"
        }
        fun programProviderData(programKey: String, program: ProgramRecord): String {
            val base = programProviderData(programKey)
            val details = listOf(
                "extendedItemsB64" to program.extendedItemsJson,
                "componentTextB64" to program.componentText.orEmpty(),
                "audioComponentTextB64" to program.audioComponentText.orEmpty(),
                "audioLanguageB64" to program.audioLanguage.orEmpty(),
                "canonicalGenreB64" to program.canonicalGenre.orEmpty(),
                "genreSupplementTextB64" to program.genreSupplementText.orEmpty(),
                "eventGroupTextB64" to program.eventGroupText.orEmpty(),
                "freeCaTextB64" to program.freeCaText.orEmpty(),
                "seriesNameB64" to program.seriesName.orEmpty(),
                "diagnosticTextB64" to program.diagnosticText,
                "descriptorJsonB64" to program.diagnosticDescriptorJson,
            ).filter { it.second.isNotBlank() && it.second != "[]" && it.second != "{}" }
                .joinToString(";") { (key, value) -> "$key=${Base64.getUrlEncoder().withoutPadding().encodeToString(value.toByteArray(Charsets.UTF_8))}" }
            return if (details.isBlank()) base else "$base;$details"
        }
        fun parseProgramKey(providerData: String?): String? {
            val raw = providerData?.takeIf { it.isNotBlank() } ?: return null
            if (!raw.contains("=")) return raw
            val encoded = raw.split(';')
                .mapNotNull { part ->
                    val i = part.indexOf('=')
                    if (i <= 0) null else part.substring(0, i) to part.substring(i + 1)
                }
                .toMap()[PROGRAM_KEY_FIELD] ?: return null
            return runCatching { String(Base64.getUrlDecoder().decode(encoded), Charsets.UTF_8) }.getOrNull()
        }
    }

    private fun fallbackName(key: ServiceKey): String = "service-${key.originalNetworkId}-${key.transportStreamId}-${key.serviceId}"

    private fun channelType(deliverySystem: String): String = when (deliverySystem) {
        ChannelRecord.DELIVERY_SYSTEM_ISDB_T -> TvContract.Channels.TYPE_ISDB_T
        ChannelRecord.DELIVERY_SYSTEM_ISDB_S -> TvContract.Channels.TYPE_ISDB_S
        else -> TvContract.Channels.TYPE_OTHER
    }

    private fun identityBlob(channel: ChannelRecord): ByteArray {
        val key = channel.serviceKey
        return listOf(
            "onid=${key.originalNetworkId}", "tsid=${key.transportStreamId}", "sid=${key.serviceId}", "input=$inputId",
            "system=${channel.deliverySystem}", "frequencyHz=${channel.frequencyHz}", "streamSelectorType=${channel.streamSelector.type.name}", "streamSelectorValue=${channel.streamSelector.value ?: ""}",
            "physicalChannel=${channel.physicalChannel ?: ""}", "backendHint=${channel.backendHint ?: ""}", "satelliteBand=${channel.satelliteBand ?: ""}", "remoteControlKeyId=${channel.remoteControlKeyId ?: ""}",
        ).joinToString(";").toByteArray(Charsets.UTF_8)
    }

    private class AndroidTvProviderChannelStore(private val context: Context, private val inputId: String) : ChannelStore {
        override fun findExistingChannelId(key: ServiceKey): Result<Long?> = runCatching {
            val projection = arrayOf(TvContract.Channels._ID)
            val selection = "${TvContract.Channels.COLUMN_INPUT_ID}=? AND ${TvContract.Channels.COLUMN_ORIGINAL_NETWORK_ID}=? AND ${TvContract.Channels.COLUMN_TRANSPORT_STREAM_ID}=? AND ${TvContract.Channels.COLUMN_SERVICE_ID}=?"
            val args = arrayOf(inputId, key.originalNetworkId.toString(), key.transportStreamId.toString(), key.serviceId.toString())
            context.contentResolver.query(TvContract.Channels.CONTENT_URI, projection, selection, args, null)?.use { cursor -> if (cursor.moveToFirst()) cursor.getLong(0) else null }
        }.onFailure { Log.w(LogTags.TIS, "既存 channel 検索に失敗しました key=$key", it) }

        override fun insertChannel(values: ContentValues): Result<Long?> = runCatching {
            val uri: Uri? = context.contentResolver.insert(TvContract.Channels.CONTENT_URI, values)
            uri?.let { ContentUris.parseId(it) }
        }

        override fun updateChannel(channelId: Long, values: ContentValues): Result<Int> = runCatching {
            context.contentResolver.update(ContentUris.withAppendedId(TvContract.Channels.CONTENT_URI, channelId), values, null, null)
        }

        override fun findExistingProgramId(channelId: Long, programKey: String): Result<Long?> = runCatching {
            val projection = arrayOf(TvContract.Programs._ID, TvContract.Programs.COLUMN_INTERNAL_PROVIDER_DATA)
            val selection = "${TvContract.Programs.COLUMN_CHANNEL_ID}=?"
            val args = arrayOf(channelId.toString())
            context.contentResolver.query(TvContract.Programs.CONTENT_URI, projection, selection, args, null)?.use { cursor ->
                while (cursor.moveToNext()) {
                    val data = cursor.getBlob(1)?.toString(Charsets.UTF_8)
                    if (TvProviderWriter.parseProgramKey(data) == programKey) return@use cursor.getLong(0)
                }
                null
            }
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
            val selection = "${TvContract.Programs.COLUMN_CHANNEL_ID}=? AND ${TvContract.Programs.COLUMN_START_TIME_UTC_MILLIS}>=? AND ${TvContract.Programs.COLUMN_START_TIME_UTC_MILLIS}<=?"
            val args = arrayOf(channelId.toString(), windowStartMs.toString(), windowEndMs.toString())
            context.contentResolver.query(TvContract.Programs.CONTENT_URI, projection, selection, args, null)?.use { cursor ->
                while (cursor.moveToNext()) {
                    val id = cursor.getLong(0)
                    val key = TvProviderWriter.parseProgramKey(cursor.getBlob(1)?.toString(Charsets.UTF_8))
                    if (key != null && key !in validProgramKeys) {
                        deleted += context.contentResolver.delete(ContentUris.withAppendedId(TvContract.Programs.CONTENT_URI, id), null, null)
                    }
                }
            }
            deleted
        }
    }
}
