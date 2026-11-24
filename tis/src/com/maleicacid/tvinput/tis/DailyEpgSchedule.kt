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
import com.maleicacid.tvinput.R
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

    fun arm(
        context: Context,
        onlyIfAbsent: Boolean = false,
    ) {
        val alarm = context.getSystemService(AlarmManager::class.java) ?: return
        val intent = Intent(context, EpgBootSyncReceiver::class.java).setAction(ACTION)
        val flags = PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_ONE_SHOT
        val lookupFlags = flags or PendingIntent.FLAG_NO_CREATE
        if (onlyIfAbsent && PendingIntent.getBroadcast(context, 0, intent, lookupFlags) != null) return
        val pending = PendingIntent.getBroadcast(context, 0, intent, flags)
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
            val displayTime = time(activity).format(DateTimeFormatter.ofPattern("HH:mm"))
            clock.text = activity.getString(R.string.daily_epg_time, displayTime)
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
                setText(R.string.daily_epg_targets)
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
                    Toast.makeText(activity, R.string.daily_epg_channels_unavailable, Toast.LENGTH_SHORT).show()
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
            .setTitle(R.string.daily_epg_targets)
            .setMultiChoiceItems(names, checked) { _, i, value ->
                checked[i] = value
            }.setPositiveButton(android.R.string.ok) { _, _ ->
                val keys = channels.filterIndexed { i, _ -> checked[i] }.map(::identity).toSet()
                prefs.edit().putStringSet("services", keys).apply()
            }.setNeutralButton(R.string.daily_epg_all_services) { _, _ -> prefs.edit().remove("services").apply() }
            .setNegativeButton(android.R.string.cancel, null)
            .show()
    }
}
