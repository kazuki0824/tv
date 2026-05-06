package com.maleicacid.tvinput.aribsi

import android.app.Service
import android.content.Intent
import android.os.Binder
import android.os.IBinder
import android.util.Log
import com.maleicacid.tvinput.common.LogTags
import com.maleicacid.tvinput.ipc.LocalServiceLocator

class AribSiEngineService : Service() {
    private val engine by lazy { AribSiEngine(this) }

    inner class LocalBinder : Binder() {
        fun engine(): AribSiEngine = engine
    }

    override fun onCreate() {
        super.onCreate()
        LocalServiceLocator.aribSiEngine = engine
        Log.i(LogTags.ARIBSI, "ARIB SI engine service を作成しました")
    }

    override fun onBind(intent: Intent?): IBinder = LocalBinder()

    override fun onDestroy() {
        LocalServiceLocator.aribSiEngine = null
        engine.close()
        super.onDestroy()
    }
}
