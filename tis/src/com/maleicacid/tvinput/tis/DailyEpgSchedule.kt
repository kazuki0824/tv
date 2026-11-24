package com.maleicacid.tvinput.tis

import android.app.Activity
import android.app.AlarmManager
import android.app.AlertDialog
import android.app.PendingIntent
import android.app.TimePickerDialog
import android.content.Context
import android.content.Intent
import android.widget.Button
import android.widget.LinearLayout
import android.widget.Toast
import com.maleicacid.tvinput.db.ChannelRecord
import java.time.LocalTime
import java.time.ZonedDateTime
import java.time.format.DateTimeFormatter

internal object DailyEpgSchedule {
    const val ACTION = "com.maleicacid.tvinput.DAILY_EPG"
    private val defaultTime = LocalTime.parse("23:00")

    private fun preferences(context: Context) =
        context.createDeviceProtectedStorageContext().getSharedPreferences("daily_epg", Context.MODE_PRIVATE)

    private fun time(context: Context): LocalTime {
        val value = preferences(context).getString("time", null)
        return value?.let(LocalTime::parse) ?: defaultTime
    }

    internal fun next(
        now: ZonedDateTime,
        time: LocalTime,
    ): ZonedDateTime {
        val today = now.toLocalDate().atTime(time).atZone(now.zone)
        return if (today.isAfter(now)) {
            today
        } else {
            now
                .toLocalDate()
                .plusDays(1)
                .atTime(time)
                .atZone(now.zone)
        }
    }

    fun arm(context: Context) {
        val alarm = context.getSystemService(AlarmManager::class.java) ?: return
        val intent = Intent(context, EpgBootSyncReceiver::class.java).setAction(ACTION)
        val pending = PendingIntent.getBroadcast(context, 0, intent, PendingIntent.FLAG_IMMUTABLE)
        val nextRun = next(ZonedDateTime.now(), time(context)).toInstant()
        alarm.setAndAllowWhileIdle(AlarmManager.RTC_WAKEUP, nextRun.toEpochMilli(), pending)
    }

    private fun identity(channel: ChannelRecord): String =
        with(channel.serviceKey) {
            "$originalNetworkId/$transportStreamId/$serviceId"
        }

    fun targets(
        context: Context,
        channels: List<ChannelRecord>,
    ): List<ChannelRecord> {
        val selected = preferences(context).getStringSet("services", null) ?: return channels
        return channels.filter { identity(it) in selected }
    }

    fun addSettings(
        activity: Activity,
        layout: LinearLayout,
        inputId: String,
    ) {
        val clock = Button(activity)

        fun updateLabel() {
            clock.text = "番組表の取得時刻 " + time(activity).format(DateTimeFormatter.ofPattern("HH:mm"))
        }
        updateLabel()
        clock.setOnClickListener {
            val current = time(activity)
            TimePickerDialog(activity, { _, hour, minute ->
                preferences(activity).edit().putString("time", LocalTime.of(hour, minute).toString()).apply()
                arm(activity)
                updateLabel()
            }, current.hour, current.minute, true).show()
        }
        layout.addView(clock)
        layout.addView(
            Button(activity).apply {
                text = "指定時刻に取得する局"
                setOnClickListener { chooseChannels(activity, inputId) }
            },
        )
    }

    private fun chooseChannels(
        activity: Activity,
        inputId: String,
    ) {
        Thread {
            val result = TvProviderWriter(activity.applicationContext, inputId).existingChannelsResult()
            activity.runOnUiThread {
                if (activity.isFinishing || activity.isDestroyed) return@runOnUiThread
                result.onSuccess { showSelection(activity, it) }.onFailure {
                    Toast.makeText(activity, "登録局を読み込めませんでした", Toast.LENGTH_SHORT).show()
                }
            }
        }.start()
    }

    private fun showSelection(
        activity: Activity,
        channels: List<ChannelRecord>,
    ) {
        val prefs = preferences(activity)
        val selected = prefs.getStringSet("services", null)
        val checked = BooleanArray(channels.size) { selected == null || identity(channels[it]) in selected }
        val names = channels.map { "${it.displayNumber} ${it.displayName}" }.toTypedArray()
        AlertDialog
            .Builder(activity)
            .setTitle("指定時刻に取得する局")
            .setMultiChoiceItems(names, checked) { _, i, value ->
                checked[i] = value
            }.setPositiveButton(android.R.string.ok) { _, _ ->
                val keys = channels.filterIndexed { i, _ -> checked[i] }.map(::identity).toSet()
                prefs.edit().putStringSet("services", keys).apply()
            }.setNeutralButton("全登録局（追加局を含む）") { _, _ -> prefs.edit().remove("services").apply() }
            .setNegativeButton(android.R.string.cancel, null)
            .show()
    }
}
