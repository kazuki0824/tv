from pathlib import Path

p = Path('tis/tests/src/com/maleicacid/tvinput/tis/CasControllerStateTest.kt')
s = p.read_text(encoding='utf-8')
old = '''    @Test fun clearServiceRetriesOnlyPidsWhoseRemovalFailed() {
        val removed = mutableListOf<TsPid>()
        val p1 = TsPid(0x101)
        val p2 = TsPid(0x102)
        var reject = true
        val bridge =
            object : CasController.TunerDescramblerBridge {
                override fun setKeyToken(keyToken: TunerKeyToken) = Result.success(Unit)

                override fun addPid(elementaryPid: TsPid) = Result.success(Unit)

                override fun removePid(elementaryPid: TsPid): Result<Unit> {
                    removed += elementaryPid
                    return if (reject &&
                        elementaryPid == p1
                    ) {
                        Result.failure(IllegalStateException("remove failed"))
                    } else {
                        Result.success(Unit)
                    }
                }

                override fun close() = Unit
            }
        CasController(mediaCasFactory = FakeMediaCasBridgeFactory()).use { controller ->
            controller.updateFromCaMetadata(
                b25Metadata(p1, TsPid(0x123), TsPid(0x010)) +
                    b25Metadata(p2, TsPid(0x123), TsPid(0x010)),
            ) { bridge }
            controller.onEcmSection(TsPid(0x123), byteArrayOf(1))
            check(runCatching { controller.clearForClearService() }.isFailure)
            check(removed == listOf(p1, p2))
            check(controller.lastDiagnostic().errorCode == CasController.ErrorCode.DESCRAMBLER_FAILED)
            check(controller.onEcmSection(TsPid(0x123), byteArrayOf(1)).isEmpty())
            reject = false
            controller.clearForClearService()
            check(removed == listOf(p1, p2, p1))
        }
    }
'''
new = '''    @Test fun metadataUpdateRetriesOnlyPidsWhoseRemovalFailed() {
        val removed = mutableListOf<TsPid>()
        val p1 = TsPid(0x101)
        val p2 = TsPid(0x102)
        var reject = true
        val bridge =
            object : CasController.TunerDescramblerBridge {
                override fun setKeyToken(keyToken: TunerKeyToken) = Result.success(Unit)

                override fun addPid(elementaryPid: TsPid) = Result.success(Unit)

                override fun removePid(elementaryPid: TsPid): Result<Unit> {
                    removed += elementaryPid
                    return if (reject && elementaryPid == p1) {
                        Result.failure(IllegalStateException("remove failed"))
                    } else {
                        Result.success(Unit)
                    }
                }

                override fun close() = Unit
            }
        CasController(mediaCasFactory = FakeMediaCasBridgeFactory()).use { controller ->
            controller.updateFromCaMetadata(
                b25Metadata(p1, TsPid(0x123), TsPid(0x010)) +
                    b25Metadata(p2, TsPid(0x123), TsPid(0x010)),
            ) { bridge }
            controller.onEcmSection(TsPid(0x123), byteArrayOf(1))

            val failed = controller.updateFromCaMetadata(b25Metadata(p2, TsPid(0x123), TsPid(0x010))) { bridge }
            check(failed.diagnostics.any { it.errorCode == CasController.ErrorCode.DESCRAMBLER_FAILED })
            check(removed == listOf(p1))
            check(controller.onEcmSection(TsPid(0x123), byteArrayOf(1)).isEmpty())

            reject = false
            val retried = controller.updateFromCaMetadata(b25Metadata(p2, TsPid(0x123), TsPid(0x010))) { bridge }
            check(retried.diagnostics.isEmpty())
            check(removed == listOf(p1, p1))
            controller.updateFromCaMetadata(b25Metadata(p2, TsPid(0x123), TsPid(0x010))) { bridge }
            check(removed == listOf(p1, p1))
        }
    }
'''
if s.count(old) != 1:
    raise SystemExit(f'pid retry test match count={s.count(old)}')
s = s.replace(old, new, 1)
old2 = '''            elementaryPid = TsPid(0x101),
            privateData = byteArrayOf(value.toByte()),
            source = CaMetadataSource.ELEMENTARY_STREAM,
'''
new2 = '''            elementaryPid = TsPid(0x100 + system),
            privateData = byteArrayOf(value.toByte()),
            source = CaMetadataSource.ELEMENTARY_STREAM,
'''
if s.count(old2) != 1:
    raise SystemExit(f'metadata fixture match count={s.count(old2)}')
s = s.replace(old2, new2, 1)
p.write_text(s, encoding='utf-8')
