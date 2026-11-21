package android.util

@Suppress("UNUSED_PARAMETER")
object Log {
    const val DEBUG = 3

    // hostではAndroidのlog propertyを再実装せず、追加DEBUG診断を無効にする。
    @Suppress("FunctionOnlyReturningConstant")
    @JvmStatic
    fun isLoggable(
        tag: String?,
        level: Int,
    ): Boolean = false

    // Android Logのメソッド形状と戻り値をホスト用スタブで維持する。
    @Suppress("FunctionOnlyReturningConstant")
    @JvmStatic
    fun d(
        tag: String,
        message: String,
    ): Int = 0

    // Android Logのメソッド形状と戻り値をホスト用スタブで維持する。
    @Suppress("FunctionOnlyReturningConstant")
    @JvmStatic
    fun i(
        tag: String,
        message: String,
    ): Int = 0

    // Android Logのメソッド形状と戻り値をホスト用スタブで維持する。
    @Suppress("FunctionOnlyReturningConstant")
    @JvmStatic
    fun w(
        tag: String,
        message: String,
    ): Int = 0

    // Android Logのメソッド形状と戻り値をホスト用スタブで維持する。
    @Suppress("FunctionOnlyReturningConstant")
    @JvmStatic
    fun w(
        tag: String,
        message: String,
        error: Throwable,
    ): Int = 0
}
