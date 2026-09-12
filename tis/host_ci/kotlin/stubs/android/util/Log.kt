package android.util

@Suppress("UNUSED_PARAMETER")
object Log {
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
