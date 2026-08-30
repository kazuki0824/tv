package com.maleicacid.tvinput.tis

import android.media.MediaSync
import android.os.Handler
import android.util.Log
import com.maleicacid.tvinput.common.LogTags
import java.lang.reflect.InvocationTargetException
import java.lang.reflect.Proxy

/**
 * 任意のplatform-private MediaSync first-final-output callbackを実行時に接続するbridge。
 *
 * stock platformでもcompileできるよう、MediaSync.OnFirstVideoFrameQueuedToOutputListenerを
 * 静的参照しない。patch適用済みplatformだけを実行時に発見して接続する。
 */
internal object MediaSyncFirstOutputBridge {
    private const val LISTENER_CLASS_NAME =
        "android.media.MediaSync\$OnFirstVideoFrameQueuedToOutputListener"
    private const val SETTER_NAME = "setOnFirstVideoFrameQueuedToOutputListener"

    private data class Binding(
        val listenerClass: Class<*>,
        val setter: java.lang.reflect.Method,
    )

    private val binding: Binding? by lazy(LazyThreadSafetyMode.PUBLICATION) {
        runCatching {
            val listenerClass = Class.forName(LISTENER_CLASS_NAME)
            val setter = MediaSync::class.java.getMethod(
                SETTER_NAME,
                java.lang.Long.TYPE,
                listenerClass,
                Handler::class.java,
            )
            Binding(listenerClass, setter)
        }.onFailure { error ->
            Log.i(LogTags.TIS, "MediaSync final-output private callback is unavailable; compatibility fallback will be used: ${error.javaClass.simpleName}")
        }.getOrNull()
    }

    fun isAvailable(): Boolean = binding != null

    fun arm(
        sync: MediaSync,
        armSequence: Long,
        handler: Handler,
        callback: (MediaSync, Long) -> Unit,
    ): Boolean {
        val resolved = binding ?: return false
        val listener = Proxy.newProxyInstance(
            resolved.listenerClass.classLoader ?: MediaSync::class.java.classLoader,
            arrayOf(resolved.listenerClass),
        ) { proxy, method, args ->
            when (method.name) {
                "onFirstVideoFrameQueuedToOutput" -> {
                    val callbackSync = args?.getOrNull(0) as? MediaSync ?: return@newProxyInstance null
                    val sequence = (args.getOrNull(1) as? Number)?.toLong() ?: return@newProxyInstance null
                    callback(callbackSync, sequence)
                    null
                }
                "toString" -> "MediaSyncFirstOutputListenerProxy"
                "hashCode" -> System.identityHashCode(proxy)
                "equals" -> proxy === args?.getOrNull(0)
                else -> null
            }
        }
        return try {
            resolved.setter.invoke(sync, armSequence, listener, handler)
            true
        } catch (error: ReflectiveOperationException) {
            val cause = if (error is InvocationTargetException) error.targetException ?: error else error
            Log.w(LogTags.TIS, "MediaSync final-output private callback arm failed", cause)
            false
        } catch (error: RuntimeException) {
            Log.w(LogTags.TIS, "MediaSync final-output private callback arm failed", error)
            false
        }
    }
}
