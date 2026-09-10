package com.maleicacid.tvinput.tis

/** Session executor内で所有する、現在番組の一時解除。番組時刻をstable keyに含めない。 */
internal class TemporaryContentUnblocks {
    private data class Expiry(val utcMillis: Long, val elapsedMillis: Long)
    private var programIdentity: String? = null
    private val grants = linkedMapOf<String, Expiry>()

    fun updateProgram(identity: String?) {
        if (programIdentity != identity) grants.clear()
        programIdentity = identity
    }

    fun grant(key: String, endTimeMillis: Long, nowUtcMillis: Long, nowElapsedMillis: Long): Boolean {
        expire(nowUtcMillis, nowElapsedMillis)
        if (programIdentity == null || nowElapsedMillis < 0 || endTimeMillis <= nowUtcMillis) return false
        val elapsedDeadline = try {
            Math.addExact(nowElapsedMillis, Math.subtractExact(endTimeMillis, nowUtcMillis))
        } catch (_: ArithmeticException) {
            return false
        }
        // 同じ認証通知の再送や番組延長で既存の解除期限を延ばさない。
        grants.putIfAbsent(key, Expiry(endTimeMillis, elapsedDeadline))
        return true
    }

    fun contains(key: String, nowUtcMillis: Long, nowElapsedMillis: Long): Boolean {
        expire(nowUtcMillis, nowElapsedMillis)
        return key in grants
    }

    fun restrictEnd(endTimeMillis: Long, nowUtcMillis: Long, nowElapsedMillis: Long) {
        expire(nowUtcMillis, nowElapsedMillis)
        if (endTimeMillis <= nowUtcMillis) {
            grants.clear()
            return
        }
        val deadline = try {
            Math.addExact(nowElapsedMillis, Math.subtractExact(endTimeMillis, nowUtcMillis))
        } catch (_: ArithmeticException) {
            grants.clear()
            return
        }
        grants.replaceAll { _, expiry ->
            Expiry(minOf(expiry.utcMillis, endTimeMillis), minOf(expiry.elapsedMillis, deadline))
        }
    }

    fun nextDelayMillis(nowUtcMillis: Long, nowElapsedMillis: Long): Long? {
        expire(nowUtcMillis, nowElapsedMillis)
        return grants.values.minOfOrNull { it.elapsedMillis - nowElapsedMillis }
    }

    fun expire(nowUtcMillis: Long, nowElapsedMillis: Long) {
        grants.entries.removeAll {
            nowElapsedMillis < 0 || nowUtcMillis >= it.value.utcMillis || nowElapsedMillis >= it.value.elapsedMillis
        }
    }

    fun clear() {
        grants.clear()
        programIdentity = null
    }
}
