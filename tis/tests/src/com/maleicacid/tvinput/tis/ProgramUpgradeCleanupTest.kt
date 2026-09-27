package com.maleicacid.tvinput.tis

import org.junit.Test

class ProgramUpgradeCleanupTest {
    @Test
    fun deletesOnlyOwnPrograms() {
        val rows =
            listOf(
                1L to "com.maleicacid.tvinput",
                2L to "other.package",
                3L to "com.maleicacid.tvinput",
                4L to null,
            )
        check(
            ProgramUpgradeCleanup.ownedProgramIds(rows, "com.maleicacid.tvinput") ==
                listOf(1L, 3L),
        )
    }

    @Test
    fun commitsIdentityOnlyAfterEveryDeleteSucceeds() {
        val deleted = mutableListOf<Long>()
        var committed = false
        val success =
            ProgramUpgradeCleanup.runCleanupTransaction(
                programIds = listOf(10L, 20L, 30L),
                deleteProgram = {
                    deleted += it
                    true
                },
                commitIdentity = {
                    committed = true
                    true
                },
            )
        check(success)
        check(deleted == listOf(10L, 20L, 30L))
        check(committed)
    }

    @Test
    fun failedDeleteStopsBeforeIdentityCommit() {
        val deleted = mutableListOf<Long>()
        var committed = false
        val success =
            ProgramUpgradeCleanup.runCleanupTransaction(
                programIds = listOf(10L, 20L, 30L),
                deleteProgram = {
                    deleted += it
                    it != 20L
                },
                commitIdentity = {
                    committed = true
                    true
                },
            )
        check(!success)
        check(deleted == listOf(10L, 20L))
        check(!committed)
    }
}
