package com.maleicacid.tvinput.tis

import android.media.MediaCas
import com.maleicacid.tvinput.aribsi.CaMetadata
import com.maleicacid.tvinput.aribsi.CaMetadataSource
import com.maleicacid.tvinput.common.ServiceKey
import com.maleicacid.tvinput.common.TsPid
import org.junit.Test

class FrameworkCasCloseTest {
    private val metadata = listOf(CaMetadata(ServiceKey(4, 16625, 101), 5,
        ecmPid = TsPid(0x123), elementaryPid = TsPid(0x101), source = CaMetadataSource.ELEMENTARY_STREAM))

    @Test fun underlyingSessionAndPluginCloseFailuresReachRealAdaptersAndOwner() {
        val f = MediaCas.Faults
        f.reset()
        val controller = CasController()
        try {
            controller.updateFromCaMetadata(metadata)
            f.sessionFailure = true
            f.pluginFailure = true
            check(runCatching { controller.close() }.isFailure)
            check(f.sessionCloses == 1 && f.pluginCloses == 1)
            f.sessionFailure = false
            check(runCatching { controller.close() }.isFailure)
            check(f.sessionCloses == 2 && f.pluginCloses == 2)
            f.pluginFailure = false
            controller.close()
            check(f.sessionCloses == 2 && f.pluginCloses == 3)
            controller.close()
            check(f.sessionCloses == 2 && f.pluginCloses == 3)
        } finally {
            f.sessionFailure = false; f.pluginFailure = false
            controller.close()
        }
    }

    @Test fun failedOpenAndPluginRollbackRetainOwnershipBeforeReturningDiagnostic() {
        val f = MediaCas.Faults
        f.reset()
        val controller = CasController()
        try {
            f.openFailure = true
            f.pluginFailure = true
            val result = controller.updateFromCaMetadata(metadata)
            check(result.diagnostics.any { it.errorCode == CasController.ErrorCode.SESSION_OPEN_FAILED })
            check(result.ecmPids.isEmpty() && f.creates == 1 && f.pluginCloses == 1)
            check(runCatching { controller.updateFromCaMetadata(metadata) }.isFailure)
            check(f.creates == 1 && f.pluginCloses == 2 && f.sessionCloses == 0)
            f.pluginFailure = false
            f.openFailure = false
            controller.updateFromCaMetadata(metadata)
            check(f.pluginCloses == 3 && f.creates == 2)
            controller.close()
            check(f.pluginCloses == 4 && f.sessionCloses == 1)
        } finally {
            f.pluginFailure = false; f.openFailure = false
            controller.close()
        }
    }
}
