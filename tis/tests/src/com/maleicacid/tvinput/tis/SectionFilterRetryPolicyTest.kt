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
    fun exceptionalOpenRetainsCleanupOwnershipUntilRequestRemoval() {
        val pid = TsPid(EXCEPTIONAL_DYNAMIC_PID)
        val current = linkedSetOf<TsPid>()
        val failed = linkedSetOf<TsPid>()
        var openAttempts = 0
        var cleanupAttempts = 0
        var firstOpen = true

        fun apply(next: Set<TsPid>) {
            SectionFilterPolicy.replaceDynamicPids(
                current = current,
                next = next,
                close = {
                    check(it == pid)
                    cleanupAttempts++
                },
                open = {
                    check(it == pid)
                    openAttempts++
                    if (firstOpen) {
                        firstOpen = false
                        error("open failed after retaining cleanup ownership")
                    }
                    true
                },
                isOpen = { it in current },
                failedWhileRequested = failed,
            )
        }

        check(runCatching { apply(setOf(pid)) }.isFailure)
        check(openAttempts == 1)
        check(failed == setOf(pid))
        check(current.isEmpty())

        apply(setOf(pid))
        check(openAttempts == 1)
        check(cleanupAttempts == 0)

        apply(emptySet())
        check(cleanupAttempts == 1)
        check(failed.isEmpty())

        apply(setOf(pid))
        check(openAttempts == 2)
        check(current == setOf(pid))
        check(failed.isEmpty())
    }

    @Test
    fun removalClearsRetryMarkerEvenWhenCleanupFails() {
        val pid = TsPid(REMOVAL_RETRY_PID)
        val current = linkedSetOf<TsPid>()
        val failed = linkedSetOf(pid)
        var cleanupAttempts = 0

        val result =
            runCatching {
                SectionFilterPolicy.replaceDynamicPids(
                    current = current,
                    next = emptySet(),
                    close = {
                        check(it == pid)
                        cleanupAttempts++
                        error("injected cleanup failure")
                    },
                    open = { error("empty request must not open") },
                    isOpen = { false },
                    failedWhileRequested = failed,
                )
            }

        check(result.isFailure)
        check(cleanupAttempts == 1)
        check(failed.isEmpty())
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

    @Test
    fun scanRefreshesPmtOnlyOnPatAndPreservesDynamicRetryRemoval() {
        val pat = com.maleicacid.tvinput.aribsi.WellKnownSectionPid.PAT
        val eit = com.maleicacid.tvinput.aribsi.WellKnownSectionPid.EIT
        val pmt = TsPid(FIRST_DYNAMIC_PID)
        val nextPmt = TsPid(SECOND_DYNAMIC_PID)
        val current = linkedSetOf<TsPid>()
        val failed = linkedSetOf<TsPid>()
        var attempted = 0
        var recalculated = 0
        var firstAttemptFails = true

        fun received(
            receivedPid: TsPid?,
            required: Set<TsPid>,
        ) {
            if (!ChannelScanController.shouldRefreshPmtFiltersForSection(receivedPid)) return
            recalculated++
            SectionFilterPolicy.replaceDynamicPids(
                current = current,
                next = required,
                close = { current.remove(it) },
                open = {
                    attempted++
                    if (firstAttemptFails) {
                        firstAttemptFails = false
                        false
                    } else {
                        true
                    }
                },
                isOpen = { it in current },
                failedWhileRequested = failed,
            )
        }

        received(eit, setOf(pmt))
        received(null, setOf(pmt))
        check(recalculated == 0 && attempted == 0)
        received(pat, setOf(pmt))
        check(recalculated == 1 && attempted == 1 && failed == setOf(pmt))
        received(pat, setOf(pmt))
        check(attempted == 1 && failed == setOf(pmt))
        received(pat, emptySet())
        check(failed.isEmpty() && current.isEmpty())
        received(pat, setOf(pmt))
        check(current == setOf(pmt) && attempted == 2)
        received(pat, setOf(nextPmt))
        check(current == setOf(nextPmt) && attempted == 3)
        received(eit, emptySet())
        check(current == setOf(nextPmt) && attempted == 3)
        received(pat, emptySet())
        check(current.isEmpty())
    }

    private companion object {
        const val FIRST_DYNAMIC_PID = 0x1001
        const val SECOND_DYNAMIC_PID = 0x1002
        const val EXCEPTIONAL_DYNAMIC_PID = 0x1003
        const val REMOVAL_RETRY_PID = 0x1004
    }
}
