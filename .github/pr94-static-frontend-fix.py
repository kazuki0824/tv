from pathlib import Path

path = Path("tis/src/com/maleicacid/tvinput/tis/ChannelScanController.kt")
text = path.read_text(encoding="utf-8")
start = text.index("    fun startInitialScan(candidates: List<ScanCandidate> = JapanIsdbScanPlan.defaultInitialScan()): ScanResult {\n")
end = text.index("\n    fun startBootEpgSync", start)
old = text[start:end]
new = '''    fun startInitialScan(candidates: List<ScanCandidate> = JapanIsdbScanPlan.defaultInitialScan()): ScanResult {
        if (!cancelled.get()) cancelled.set(false)
        terminalCancelObserved = cancelled.get()
        resetResourceLostState()
        skippedUnresolvedTransportCount = 0
        val diagnostics = mutableListOf<ScanDiagnostic>()
        var published = 0
        var successfulCandidates = 0
        var scannedCandidates = 0
        var bsCandidateSource: BsCandidateSource? = null

        fun scanExecutionCandidate(candidate: ScanCandidate): Boolean {
            if (cancelled.get() || terminalResourceLostObserved) return false
            engine.reset(discoveryProfile(candidate.kind))
            currentCandidate = candidate
            val tune = tunerController.tuneForScan(candidate)
            if (!tune.success) {
                diagnostics += ScanDiagnostic(candidate, "選局に失敗しました result=${tune.resultCode} ${tune.message}")
                return true
            }
            activateScanGeneration(tune.generation)
            try {
                val collection = collectSiForCandidate(
                    candidate,
                    SiCollectionRequirements(PublishMode.SETUP_SCAN, discoveryProfile(candidate.kind)),
                    tune.generation,
                )
                collection.diagnostic?.let { diagnostics += it }
                if (!collection.mayPublishChannels) {
                    Log.w(
                        LogTags.TIS,
                        "SI discovery 未完了のため TvProvider channel 登録を省略します candidate=$candidate " +
                            "outcome=${collection.outcome} registrationReady=${collection.registrationReadyServices} " +
                            "clearLivePlaybackStaticallyEligible=${collection.clearLivePlaybackStaticallyEligibleServices} " +
                            "diagnostic=${collection.diagnostic?.message}",
                    )
                    return collection.outcome != SiCollectionOutcome.RESOURCE_LOST
                }
                val publishResult = publishScanSnapshotIfCurrent(tune.generation, PublishMode.SETUP_SCAN)
                if (publishResult == null) {
                    diagnostics += resourceLostDiagnostic(candidate, tune.generation)
                    return false
                }
                if (
                    collection.outcome == SiCollectionOutcome.COMPLETE &&
                    collection.registrationReadyServices > 0 &&
                    publishResult.success
                ) {
                    successfulCandidates++
                }
                published += publishResult.changed
                return true
            } finally {
                clearActiveScanGeneration(tune.generation)
            }
        }

        scanLoop@ for (candidate in candidates) {
            if (cancelled.get() || terminalResourceLostObserved) break
            val executionCandidates =
                if (
                    candidate.kind == ScanCandidateKind.ISDB_S_BS &&
                    candidate.streamSelector == com.maleicacid.tvinput.common.StreamSelector.NONE
                ) {
                    if (bsCandidateSource == null) {
                        val selection = tunerController.prepareBsCandidateSource()
                        if (!selection.success) {
                            diagnostics += ScanDiagnostic(
                                candidate,
                                "BS frontend選択に失敗しました result=${selection.resultCode} message=${selection.message}",
                            )
                            break@scanLoop
                        }
                        bsCandidateSource = requireNotNull(selection.source)
                    }
                    when (bsCandidateSource) {
                        BsCandidateSource.DYNAMIC_STREAM_ID_LIST -> {
                            val discovery = tunerController.discoverIsdbsStreamIds(candidate)
                            discovery.generation?.let { activateScanGeneration(it) }
                            if (discovery.resourceLost || terminalResourceLostObserved) {
                                discovery.generation?.let { resourceLossFence.onLost(it) }
                                diagnostics += ScanDiagnostic(
                                    candidate,
                                    "BS探索中のTUNER_RESOURCE_LOSTにより後続選局を停止します",
                                )
                                break@scanLoop
                            }
                            discovery.generation?.let { clearActiveScanGeneration(it) }
                            val discovered = discovery.candidatesFor(candidate)
                            if (discovery.success && discovered.isNotEmpty()) {
                                discovered
                            } else {
                                diagnostics += ScanDiagnostic(
                                    candidate,
                                    "BS dynamic stream-ID discovery失敗 result=${discovery.resultCode} " +
                                        "message=${discovery.message}",
                                )
                                emptyList()
                            }
                        }
                        BsCandidateSource.STATIC_TSID_TABLE -> JapanIsdbScanPlan.staticBsCandidatesFor(candidate)
                        null -> error("BS候補sourceが確定していません")
                    }
                } else {
                    listOf(candidate)
                }
            scannedCandidates += executionCandidates.size
            for (executionCandidate in executionCandidates) {
                if (!scanExecutionCandidate(executionCandidate)) break@scanLoop
            }
        }
        currentCandidate = null
        return ScanResult(
            scannedCandidates,
            published,
            diagnostics,
            successfulCandidates = successfulCandidates,
            terminalCancelObserved = terminalCancelObserved,
            terminalResourceLostObserved = terminalResourceLostObserved,
        )
    }
'''
if "val executionCandidates = mutableListOf<ScanCandidate>()" not in old:
    raise SystemExit("expected precomputed execution-candidate loop not found")
path.write_text(text[:start] + new + text[end:], encoding="utf-8")
