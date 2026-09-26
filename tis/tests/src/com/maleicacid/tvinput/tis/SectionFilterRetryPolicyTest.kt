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

    @Suppress("TooGenericExceptionCaught")
    @Test
    fun casRejectCleanupKeepsFailureMarkerForContinuouslyRequestedPid() {
        val pid = TsPid(CAS_DYNAMIC_PID)
        val current = linkedSetOf<TsPid>()
        val failed = linkedSetOf<TsPid>()
        var openAttempts = 0

        fun apply(next: Set<TsPid>) {
            SectionFilterPolicy.replaceDynamicPids(
                current = current,
                next = next,
                close = { current.remove(it) },
                open = {
                    openAttempts++
                    false
                },
                isOpen = { it in current },
                failedWhileRequested = failed,
            )
        }

        fun refreshSameMetadata() {
            val requested = setOf(pid)
            val retainAfterRollback = linkedSetOf<TsPid>()
            runCatching {
                SectionFilterPolicy.commitCasAndFilters(
                    updateCas = { CasController.UpdateResult(emptyList(), requested, emptySet()) },
                    commitFilters = { result ->
                        try {
                            apply(result.ecmPids)
                            check(result.ecmPids.all { it in current }) { "CAS filter open failed" }
                        } catch (failure: RuntimeException) {
                            retainAfterRollback += failed.intersect(result.ecmPids)
                            throw failure
                        }
                    },
                    reject = {
                        apply(emptySet())
                        failed += retainAfterRollback
                    },
                )
            }
        }

        refreshSameMetadata()
        check(openAttempts == 1)
        check(failed == setOf(pid))

        refreshSameMetadata()
        check(openAttempts == 1)
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

    private companion object {
        const val FIRST_DYNAMIC_PID = 0x1001
        const val SECOND_DYNAMIC_PID = 0x1002
        const val EXCEPTIONAL_DYNAMIC_PID = 0x1003
        const val CAS_DYNAMIC_PID = 0x1004
    }
}
