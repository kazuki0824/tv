package androidx.test.platform.app

import android.app.Instrumentation

object InstrumentationRegistry {
    // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
    @Suppress("MaxLineLength")
    @JvmStatic
    fun getInstrumentation(): Instrumentation = throw UnsupportedOperationException("instrumentation is unavailable in host CI")
}
