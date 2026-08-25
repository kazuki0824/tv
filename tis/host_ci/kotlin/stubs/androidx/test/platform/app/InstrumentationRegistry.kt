package androidx.test.platform.app

import android.app.Instrumentation

object InstrumentationRegistry {
    @JvmStatic
    fun getInstrumentation(): Instrumentation =
        throw UnsupportedOperationException("instrumentation is unavailable in host CI")
}
