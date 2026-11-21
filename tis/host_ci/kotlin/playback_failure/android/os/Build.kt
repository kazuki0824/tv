package android.os

/** ホスト試験だけでSystemPropertiesのnative初期化を置き換える。 */
object Build {
    const val FINGERPRINT = "host-test"
    const val IS_DEBUGGABLE = false
    const val HW_TIMEOUT_MULTIPLIER = 1
}
