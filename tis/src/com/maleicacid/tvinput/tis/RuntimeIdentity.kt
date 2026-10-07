package com.maleicacid.tvinput.tis

/** Runtime世代・tokenの枯渇をsilent wrap/reuseへ落とさない共通allocation規則。 */
internal object RuntimeIdentity {
    fun nextLong(
        current: Long,
        label: String,
    ): Long {
        require(current >= 0L) { "$label の現在identityは0以上でなければなりません: $current" }
        return try {
            Math.addExact(current, 1L)
        } catch (error: ArithmeticException) {
            throw IllegalStateException("$label のidentityが${current}で枯渇しました", error)
        }
    }

    fun nextInt(
        current: Int,
        label: String,
    ): Int {
        require(current >= 0) { "$label の現在identityは0以上でなければなりません: $current" }
        return try {
            Math.addExact(current, 1)
        } catch (error: ArithmeticException) {
            throw IllegalStateException("$label のidentityが${current}で枯渇しました", error)
        }
    }

    fun nextReusablePositiveInt(
        current: Int,
        live: Set<Int>,
        label: String,
    ): Int {
        require(current >= 0) { "$label の現在tokenは0以上でなければなりません: $current" }
        var candidate = current
        repeat(live.size + 1) {
            candidate = if (candidate == Int.MAX_VALUE) 1 else candidate + 1
            if (candidate !in live) return candidate
        }
        error("$label のtoken空間が枯渇しました live数=${live.size}")
    }

    fun nextReusablePositiveLong(
        current: Long,
        live: Set<Long>,
        label: String,
    ): Long {
        require(current >= 0L) { "$label の現在tokenは0以上でなければなりません: $current" }
        var candidate = current
        repeat(live.size + 1) {
            candidate = if (candidate == Long.MAX_VALUE) 1L else candidate + 1L
            if (candidate !in live) return candidate
        }
        error("$label のtoken空間が枯渇しました live数=${live.size}")
    }
}
