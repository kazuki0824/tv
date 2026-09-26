package com.maleicacid.tvinput.tis

import com.maleicacid.tvinput.common.TsPid
import org.junit.Test

class SectionFilterRetryPolicyTest {
    @Test
    fun failedDynamicOpenIsAttemptedOnceWhilePidRemainsRequested() {
        val pid = TsPid(FIRST_DYNAMIC_PID)
        val current = linkedSetOf<TsPid>()
        val failed = linkedSetOf<TsPid>()
        var attempts = 0

        fun apply(next: Set<TsPid>) {
            SectionFilterPolicy.replaceDynamicPids(
                current = current,
                next = next,
                close = { current.remove(it) },
                open = {
                    attempts++
                    false
                },
                isOpen = { it in current },
                failedWhileRequested = failed,
            )
            private companion object {
        const val FIRST_DYNAMIC_PID = 0x1001
        const val SECOND_DYNAMIC_PID = 0x1002
    }
}

        apply(setOf(pid))
        apply(setOf(pid))
        apply(setOf(pid))
        check(attempts == 1)
        check(current.isEmpty())
        check(failed == setOf(pid))

        apply(emptySet())
        check(failed.isEmpty())

        apply(setOf(pid))
        check(attempts == 2)
        check(failed == setOf(pid))
    }

    @Test
    fun successfulDynamicOpenClearsFailedMarkerAndPublishesPid() {
        val pid = TsPid(SECOND_DYNAMIC_PID)
        val current = linkedSetOf<TsPid>()
        val failed = linkedSetOf(pid)
        var attempts = 0

        SectionFilterPolicy.replaceDynamicPids(
            current = current,
            next = emptySet(),
            close = { current.remove(it) },
            open = { error("empty request must not open") },
            isOpen = { false },
            failedWhileRequested = failed,
        )
        check(failed.isEmpty())

        SectionFilterPolicy.replaceDynamicPids(
            current = current,
            next = setOf(pid),
            close = { current.remove(it) },
            open = {
                attempts++
                true
            },
            isOpen = { it in current },
            failedWhileRequested = failed,
        )

        check(attempts == 1)
        check(current == setOf(pid))
        check(failed.isEmpty())
    }
}
