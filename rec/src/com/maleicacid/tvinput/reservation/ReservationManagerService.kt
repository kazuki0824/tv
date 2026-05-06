package com.maleicacid.tvinput.reservation

import android.app.Service
import android.content.Intent
import android.os.Binder
import android.os.IBinder
import android.util.Log
import com.maleicacid.tvinput.common.LogTags
import com.maleicacid.tvinput.ipc.LocalServiceLocator

class ReservationManagerService : Service() {
    private val controller by lazy { ReservationController(this) }

    inner class LocalBinder : Binder() {
        fun controller(): ReservationController = controller
    }

    override fun onCreate() {
        super.onCreate()
        LocalServiceLocator.reservationController = controller
        Log.i(LogTags.RESERVATION, "予約管理 service を作成しました")
    }

    override fun onBind(intent: Intent?): IBinder = LocalBinder()

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        controller.restoreSchedules()
        return START_STICKY
    }

    override fun onDestroy() {
        LocalServiceLocator.reservationController = null
        super.onDestroy()
    }
}
