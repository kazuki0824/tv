from pathlib import Path


def replace_once(path: str, old: str, new: str) -> None:
    p = Path(path)
    text = p.read_text()
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{path}: expected exactly one match, found {count}")
    p.write_text(text.replace(old, new, 1))


# HAL: Linux DVB has no authoritative BS TMCC TSID enumeration. Reject selector-less
# BS scan before backend worker start so framework/TIS receives RESULT_UNAVAILABLE.
path = "tuner_hal2/service_runtime/src/frontend_request_txn.rs"
marker = "fn validate_frontend_begin_contract(\n"
insert = '''fn validate_dynamic_isdbs_stream_id_scan_availability(\n    entry: &FrontendRegistryEntry,\n    request: &FrontendTuneRequest,\n    scan_mode: Option<FrontendScanMode>,\n) -> Result<(), HalError> {\n    if scan_mode.is_none()\n        || entry.backend != FrontendBackendKind::LinuxDvb\n        || request.system != FrontendSystem::IsdbS\n        || request.stream_id.is_some()\n        || maleicacid_tuner_hal2_device::px4::normalize_japan_bs_if_frequency_hz(\n            request.frequency,\n        )\n        .is_none()\n    {\n        return Ok(());\n    }\n    Err(HalError::unsupported_detail(\n        "frontend.scan.inputStreamIds",\n        "Linux DVB does not expose authoritative BS TMCC TSID enumeration; use explicit absolute STREAM_ID tune candidates",\n    ))\n}\n\n'''
replace_once(path, marker, insert + marker)
replace_once(
    path,
    "    validate_frontend_requested_settings_against_product_profile(requested_settings)?;\n    validate_frontend_request_availability_against_entry(entry, request)?;\n",
    "    validate_frontend_requested_settings_against_product_profile(requested_settings)?;\n    validate_dynamic_isdbs_stream_id_scan_availability(entry, request, scan_mode)?;\n    validate_frontend_request_availability_against_entry(entry, request)?;\n",
)
# Add focused unit coverage inside the existing test module.
test_marker = "    #[test]\n    fn isdbt_physical_layer_cardinality_is_validated_after_aidl_conversion() {\n"
test = '''    #[test]\n    fn selectorless_bs_dynamic_stream_id_scan_is_px4_only() {\n        let linux = isdbs_entry(FrontendBackendKind::LinuxDvb, 1, 100_000_000);\n        let px4 = isdbs_entry(FrontendBackendKind::Px4CharDevice, 1, 100_000_000);\n        let seed = isdbs_request(None);\n\n        assert!(validate_dynamic_isdbs_stream_id_scan_availability(\n            &linux,\n            &seed,\n            Some(FrontendScanMode::Auto),\n        )\n        .is_err());\n        assert!(validate_dynamic_isdbs_stream_id_scan_availability(\n            &px4,\n            &seed,\n            Some(FrontendScanMode::Auto),\n        )\n        .is_ok());\n\n        let mut explicit = isdbs_request(None);\n        explicit.stream_id = Some(16_400);\n        explicit.stream_id_kind = Some(FrontendStreamIdKind::AbsoluteStreamId);\n        assert!(validate_dynamic_isdbs_stream_id_scan_availability(\n            &linux,\n            &explicit,\n            Some(FrontendScanMode::Auto),\n        )\n        .is_ok());\n    }\n\n'''
replace_once(path, test_marker, test + test_marker)

# TIS: RuntimeException is not evidence that the frontend lacks dynamic enumeration.
replace_once(
    "tis/src/com/maleicacid/tvinput/tis/TunerController.kt",
    "                return StreamIdDiscoveryResult(false, emptySet(), Tuner.RESULT_UNAVAILABLE, error.message.orEmpty())\n",
    "                return StreamIdDiscoveryResult(false, emptySet(), Tuner.RESULT_UNKNOWN_ERROR, error.message.orEmpty())\n",
)

# Static table is a candidate source only for an explicit unsupported result, never a
# substitute for px4 discovery failure/timeout/zero IDs.
path = "tis/src/com/maleicacid/tvinput/tis/ScanPlan.kt"
marker = "    fun explicitBsCandidatesFromScan(seed: ScanCandidate, inputStreamIds: Collection<Int>): List<ScanCandidate> {\n"
insert = '''    fun versionedBsCandidatesForUnsupportedDynamicDiscovery(seed: ScanCandidate): List<ScanCandidate> {\n        require(seed.kind == ScanCandidateKind.ISDB_S_BS && seed.streamSelector.type == StreamSelectorType.NONE)\n        return bsTsidEntries\n            .asSequence()\n            .filter { entry ->\n                entry.frequencyHz == seed.frequencyHz && entry.physical == seed.physicalChannel\n            }\n            .map { entry ->\n                ScanCandidate(\n                    deliverySystem = ChannelRecord.DELIVERY_SYSTEM_ISDB_S,\n                    frequencyHz = entry.frequencyHz,\n                    streamSelector = StreamSelector.tsid(entry.tsid.value),\n                    displayChannel = entry.label,\n                    physicalChannel = entry.physical,\n                    backendHint = "jp-bs-versioned-tsid",\n                    satelliteBand = "BS",\n                    kind = ScanCandidateKind.ISDB_S_BS,\n                )\n            }\n            .toList()\n    }\n\n'''
replace_once(path, marker, insert + marker)

# The decision is based only on the public Tuner result code; TIS never inspects backend name.
path = "tis/src/com/maleicacid/tvinput/tis/ChannelScanController.kt"
replace_once(
    path,
    "import android.media.tv.TvInputService\n",
    "import android.media.tv.TvInputService\nimport android.media.tv.tuner.Tuner\n",
)
old = '''                if (discovered.isNotEmpty()) {\n                    discovered\n                } else {\n                    diagnostics += ScanDiagnostic(\n                        candidate,\n                        "BS dynamic stream-ID discoveryをfail-closedにします result=${discovery.resultCode} message=${discovery.message}",\n                    )\n                    emptyList()\n                }\n'''
new = '''                if (discovered.isNotEmpty()) {\n                    discovered\n                } else if (discovery.resultCode == Tuner.RESULT_UNAVAILABLE) {\n                    val versioned = JapanIsdbScanPlan.versionedBsCandidatesForUnsupportedDynamicDiscovery(candidate)\n                    diagnostics += ScanDiagnostic(\n                        candidate,\n                        "このfrontendはBS dynamic stream-ID discovery非対応のためversioned TSID tune候補を使用します candidates=${versioned.size}",\n                    )\n                    versioned\n                } else {\n                    diagnostics += ScanDiagnostic(\n                        candidate,\n                        "BS dynamic stream-ID discovery失敗をfail-closedにします result=${discovery.resultCode} message=${discovery.message}",\n                    )\n                    emptyList()\n                }\n'''
replace_once(path, old, new)

# Regression: static table is an explicit-TSID candidate set scoped to one RF seed.
path = "tis/tests/src/com/maleicacid/tvinput/tis/ScanPlanPolicyTest.kt"
marker = "    @Test\n    fun bsDynamicDiscoveryUsesOnlyReportedStreamIds() {\n"
test = '''    @Test\n    fun versionedBsCandidatesAreExplicitTsidsForOneUnsupportedRfSeed() {\n        val seed = JapanIsdbScanPlan.isdbsBsBands().first()\n        val candidates = JapanIsdbScanPlan.versionedBsCandidatesForUnsupportedDynamicDiscovery(seed)\n        assertTrue(candidates.isNotEmpty())\n        assertTrue(candidates.all { it.frequencyHz == seed.frequencyHz })\n        assertTrue(candidates.all { it.physicalChannel == seed.physicalChannel })\n        assertTrue(candidates.all { it.streamSelector.type == StreamSelectorType.TSID })\n        assertEquals(setOf(16400, 16401, 16402), candidates.mapNotNull { it.streamSelector.value }.toSet())\n    }\n\n'''
replace_once(path, marker, test + marker)
