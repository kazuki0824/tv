from __future__ import annotations

import json
import subprocess
import tempfile
import unittest
from copy import deepcopy
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import MagicMock, call, patch

from vts_profile.cli import _new_profile
from vts_profile.device import resolve_device
from vts_profile.integration import write_product_artifacts
from vts_profile.model import ProfileError, save_profile, validate_profile
from vts_profile.region import resolve_region, select_candidate
from vts_profile.render import render_xml
from vts_profile.resource_closure import _program, validate_resource_closure
from vts_profile.schema import selected_xsd, validate_xml


class VtsProfileTest(unittest.TestCase):
    def profile(self, *, resolved: bool = True) -> dict:
        live = {
            "enabled": True,
            "audio_pid": 273 if resolved else None,
            "video_pid": 272 if resolved else None,
            "audio_stream_type": 16 if resolved else None,
            "video_stream_type": 5 if resolved else None,
            "pcr_pid": 272 if resolved else None,
            "section_pid": 256 if resolved else None,
        }
        return {
            "schema_version": 1,
            "target": {"hal": "tuner_hal2", "product": "default", "backend": "px4"},
            "vts": {"contract": "android14-aidl-v1", "source_ref": "aosp-commit", "variant": ""},
            "frontend": {
                "type": "ISDBT",
                "is_software_frontend": False,
                "frequency_hz": 557142857 if resolved else None,
            },
            "flows": {
                "scan": True,
                "record": {"enabled": True, "pid": 272 if resolved else None},
                "clear_live": live,
                "playback": {
                    "enabled": True,
                    "input_file_path": "/data/local/tmp/segment000000.ts",
                    "audio_pid": 257,
                    "video_pid": 256,
                    "section_pid": 257,
                    "audio_stream_type": 2,
                    "video_stream_type": 2,
                },
            },
            "queues": {
                "record_filter_bytes": 1048576,
                "record_dvr_bytes": 4194304,
                "audio_filter_bytes": 1048576,
                "video_filter_bytes": 1048576,
                "pcr_filter_bytes": 1048576,
                "section_filter_bytes": 1048576,
                "playback_dvr_bytes": 4194304,
            },
        }

    def probe_profile(self, *, resolved: bool = True) -> dict:
        profile = self.profile(resolved=resolved)
        profile["vts"]["variant"] = "record-filter-fmq"
        profile["flows"]["clear_live"] = {"enabled": False}
        profile["flows"]["playback"] = {"enabled": False}
        profile["queues"] = {
            "record_filter_bytes": 1048576,
            "record_dvr_bytes": 4194304,
        }
        return profile

    def test_canonical_xml_has_full_capability_reachability(self) -> None:
        xml = render_xml(self.profile())
        self.assertIn('frequency="557142857"', xml)
        self.assertIn('subType="RECORD"', xml)
        self.assertIn('useFMQ="false"', xml)
        self.assertIn('id="FILTER_TS_AUDIO_LIVE_0"', xml)
        self.assertIn('id="FILTER_TS_VIDEO_LIVE_0"', xml)
        self.assertIn('id="FILTER_TS_PCR_LIVE_0"', xml)
        self.assertIn('id="FILTER_TS_SECTION_LIVE_0"', xml)
        self.assertIn('bitWidthOfLengthField="12"', xml)
        self.assertIn('timeDelayInMs="100"', xml)
        self.assertIn('pcrFilterConnection="FILTER_TS_PCR_LIVE_0"', xml)
        self.assertIn('sectionFilterConnection="FILTER_TS_SECTION_LIVE_0"', xml)
        self.assertIn('<dvrPlayback dvrConnection="DVR_PLAYBACK_0"', xml)
        self.assertIn('inputFilePath="/data/local/tmp/segment000000.ts"', xml)
        self.assertIn('id="FILTER_TS_VIDEO_PLAYBACK_0"', xml)
        self.assertIn('id="FILTER_TS_AUDIO_PLAYBACK_0"', xml)
        self.assertIn('id="FILTER_TS_SECTION_PLAYBACK_0"', xml)
        self.assertIn('videoFilterConnection="FILTER_TS_VIDEO_PLAYBACK_0"', xml)
        self.assertIn('audioFilterConnection="FILTER_TS_AUDIO_PLAYBACK_0"', xml)
        self.assertIn('sectionFilterConnection="FILTER_TS_SECTION_PLAYBACK_0"', xml)
        self.assertIn('pid="256" useFMQ="false"><avFilterSettings', xml)
        self.assertIn('pid="257" useFMQ="false"><avFilterSettings', xml)

    def test_playback_asset_filter_contract_is_independent_from_live_pids(self) -> None:
        profile = self.profile()
        profile["flows"]["clear_live"]["audio_pid"] = 300
        profile["flows"]["clear_live"]["video_pid"] = 301
        xml = render_xml(profile)
        self.assertIn('id="FILTER_TS_AUDIO_LIVE_0" mainType="TS" subType="AUDIO" bufferSize="1048576" pid="300"', xml)
        self.assertIn('id="FILTER_TS_VIDEO_LIVE_0" mainType="TS" subType="VIDEO" bufferSize="1048576" pid="301"', xml)
        self.assertIn('id="FILTER_TS_AUDIO_PLAYBACK_0" mainType="TS" subType="AUDIO" bufferSize="1048576" pid="257"', xml)
        self.assertIn('id="FILTER_TS_VIDEO_PLAYBACK_0" mainType="TS" subType="VIDEO" bufferSize="1048576" pid="256"', xml)

    def test_canonical_missing_public_capability_flow_fails_closed(self) -> None:
        for flow in ("record", "clear_live", "playback"):
            with self.subTest(flow=flow):
                profile = self.profile()
                profile["flows"][flow] = {"enabled": False}
                if flow == "record":
                    profile["queues"].pop("record_filter_bytes")
                    profile["queues"].pop("record_dvr_bytes")
                elif flow == "clear_live":
                    profile["queues"].pop("pcr_filter_bytes")
                else:
                    profile["queues"].pop("playback_dvr_bytes")
                with self.assertRaisesRegex(ProfileError, "canonical VTS capability coverage is unreachable"):
                    validate_profile(profile, require_resolved=True)
        profile = self.profile()
        profile["flows"]["scan"] = False
        with self.assertRaisesRegex(ProfileError, "canonical VTS capability coverage is unreachable"):
            validate_profile(profile, require_resolved=True)

    def test_record_filter_fmq_probe_variant_requests_filter_descriptor(self) -> None:
        xml = render_xml(self.probe_profile())
        self.assertIn('subType="RECORD"', xml)
        self.assertIn('useFMQ="true"', xml)
        self.assertNotIn('subType="AUDIO"', xml)
        self.assertNotIn('<dvrPlayback ', xml)

    def test_region_resolution_and_selection_update_same_profile(self) -> None:
        profile = self.profile(resolved=False)
        profile["region"] = {"query": "大阪府", "candidates": []}
        dataset = {
            "schema_version": 2,
            "source": {"index_url": "fixture", "source_notice": "fixture"},
            "prefectures": {
                "大阪府": {
                    "source_url": "fixture",
                    "default_channels": [22, 27],
                    "prefecture_channels": [22, 27],
                    "areas": {"大阪市": [22, 27]},
                }
            },
        }
        resolve_region(profile, dataset)
        self.assertEqual(len(profile["region"]["candidates"]), 2)
        select_candidate(profile, 1)
        self.assertEqual(profile["frontend"]["frequency_hz"], 557142857)

    def test_unknown_profile_field_is_rejected(self) -> None:
        profile = self.profile()
        profile["unused"] = True
        with self.assertRaises(ProfileError):
            validate_profile(profile)

    def test_resource_closure_uses_production_rust_ssot(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "capability_snapshot.rs"
            source.write_text("// fixture")
            program = _program(self.profile(), source, 1024 * 1024)
        self.assertIn("CapabilitySnapshot::product_default()", program)
        self.assertIn("validate_dependency_closures()", program)
        self.assertIn("CapacityLedger::default()", program)
        self.assertIn("reserve_filter(snapshot, 1, FilterOpenType::TsRecord, 1048576)", program)
        self.assertIn("FilterOpenType::TsAudio", program)
        self.assertIn("FilterOpenType::TsVideo", program)
        self.assertIn("FilterOpenType::TsPcr", program)
        self.assertIn("FilterOpenType::TsSection", program)
        self.assertIn("reserve_dvr(snapshot, 2, 4194304)", program)
        self.assertIn("reserve_playback_processing(snapshot, 2, DvrKind::Playback, 4194304)", program)
        self.assertIn("require_published_coverage(snapshot.num_playback, true", program)
        self.assertIn("PLAYBACK_CONSUME_CHUNK_PACKETS: usize = 256", program)
        self.assertNotIn("let chunk = 188usize * 256usize", program)

    def test_resource_closure_uses_profile_booleans_for_reverse_coverage(self) -> None:
        profile = self.profile()
        profile["flows"]["playback"] = {"enabled": False}
        profile["queues"].pop("playback_dvr_bytes")
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "capability_snapshot.rs"
            source.write_text("// fixture")
            program = _program(profile, source, 1024 * 1024)
        self.assertIn("require_published_coverage(snapshot.num_playback, false", program)
        self.assertIn("require_published_coverage(snapshot.num_audio_filter, true", program)

    def test_resource_closure_is_fail_closed(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            capability = root / "capability_snapshot.rs"
            capability.write_text("// fixture")
            pes = root / "ts_core.rs"
            pes.write_text("pub const MAX_PES_BUFFER_BYTES: usize = 1024 * 1024;\n")
            failed = SimpleNamespace(returncode=1, stdout="", stderr="compile failed")
            with patch("vts_profile.resource_closure.subprocess.run", return_value=failed):
                with self.assertRaises(ProfileError):
                    validate_resource_closure(self.profile(), capability_source=capability, pes_source=pes)

    def test_noninteractive_init_builds_canonical_full_coverage(self) -> None:
        args = SimpleNamespace(
            non_interactive=True,
            backend="px4",
            product="default",
            delivery_system="ISDBT",
            vts_source_ref="aosp-commit",
            region=None,
            frequency_hz="557142857",
            service_id=None,
            record="yes",
            record_pid="272",
            scan="yes",
            record_filter_bytes="1048576",
            record_dvr_bytes="4194304",
            playback_dvr_bytes=4194304,
            playback_input_path="/data/local/tmp/segment000000.ts",
            variant="",
        )
        profile = _new_profile(args)
        self.assertEqual(profile["vts"]["source_ref"], "aosp-commit")
        self.assertTrue(profile["flows"]["clear_live"]["enabled"])
        self.assertTrue(profile["flows"]["playback"]["enabled"])
        self.assertEqual(profile["flows"]["playback"]["video_pid"], 256)
        self.assertEqual(profile["flows"]["playback"]["audio_pid"], 257)
        self.assertEqual(profile["flows"]["playback"]["video_stream_type"], 2)
        self.assertEqual(profile["flows"]["playback"]["audio_stream_type"], 2)

    def test_selected_xsd_requires_exact_checkout(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            subprocess.run(["git", "init", "-q", str(root)], check=True)
            subprocess.run(["git", "-C", str(root), "config", "user.email", "test@example.com"], check=True)
            subprocess.run(["git", "-C", str(root), "config", "user.name", "test"], check=True)
            xsd = root / "tv/tuner/config/tuner_testing_dynamic_configuration.xsd"
            xsd.parent.mkdir(parents=True)
            xsd.write_text("<x/>")
            subprocess.run(["git", "-C", str(root), "add", "."], check=True)
            subprocess.run(["git", "-C", str(root), "commit", "-qm", "xsd"], check=True)
            commit = subprocess.run(["git", "-C", str(root), "rev-parse", "HEAD"], check=True, capture_output=True, text=True).stdout.strip()
            self.assertEqual(selected_xsd(root, commit), xsd)
            with self.assertRaises(ProfileError):
                selected_xsd(root, "HEAD~1")

    def test_xsd_validator_and_product_variant_are_fail_closed(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            xsd = root / "schema.xsd"
            xsd.write_text("<x/>")
            ok = root / "ok"
            ok.write_text("#!/bin/sh\nexit 0\n")
            ok.chmod(0o755)
            bad = root / "bad"
            bad.write_text("#!/bin/sh\nexit 3\n")
            bad.chmod(0o755)
            validate_xml("<root/>", xsd, xmllint=str(ok))
            with self.assertRaises(ProfileError):
                validate_xml("<root/>", xsd, xmllint=str(bad))
            profile = self.profile()
            profile["vts"]["variant"] = "lab"
            output = write_product_artifacts(profile, "<validated/>", root)
            self.assertEqual(output.name, "tuner_vts_config_aidl_V1.lab.xml")
            self.assertIn(
                "ro.vendor.vts_tuner_configuration_variant=lab",
                (root / "vts_product_generated.mk").read_text(),
            )

    def test_device_resolution_keeps_all_si_reads_in_one_tune_session(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "profile.json"
            profile = self.profile(resolved=False)
            profile["frontend"]["frequency_hz"] = 557142857
            save_profile(path, profile)
            pat_semantics = {"pmt_pids": [256], "services": []}
            full_semantics = {
                "pmt_pids": [256],
                "services": [{
                    "original_network_id": 4,
                    "transport_stream_id": 1,
                    "service_id": 100,
                    "pmt_pid": 256,
                    "pcr_pid": 272,
                    "streams": [
                        {"pid": 272, "stream_type": 0x1B},
                        {"pid": 273, "stream_type": 0x0F},
                    ],
                }],
            }
            session = MagicMock()
            session.__enter__.return_value = session
            session.section.side_effect = [(0x0000, b"pat"), (0x0100, b"pmt"), (0x0011, b"sdt")]
            with (
                patch("vts_profile.device._prepare_agent", return_value=("/data/local/tmp/agent", False)),
                patch("vts_profile.device._AgentSession", return_value=session) as session_type,
                patch("vts_profile.device._si_query", side_effect=[pat_semantics, full_semantics]),
                patch("vts_profile.device._cleanup_agent"),
            ):
                updated = resolve_device(path)
            session_type.assert_called_once()
            self.assertEqual(
                session.section.call_args_list,
                [call(0x0000, 0x00), call(0x0100, 0x02), call(0x0011, 0x42)],
            )
            self.assertEqual(updated["service"]["service_id"], 100)
            self.assertEqual(updated["flows"]["record"]["pid"], 272)
            self.assertEqual(updated["flows"]["clear_live"]["video_pid"], 272)
            self.assertEqual(updated["flows"]["clear_live"]["audio_pid"], 273)
            self.assertEqual(updated["flows"]["clear_live"]["video_stream_type"], 5)
            self.assertEqual(updated["flows"]["clear_live"]["audio_stream_type"], 16)
            self.assertEqual(updated["flows"]["clear_live"]["pcr_pid"], 272)
            self.assertEqual(updated["flows"]["clear_live"]["section_pid"], 256)
            self.assertEqual(updated["flows"]["playback"]["video_pid"], 256)
            self.assertEqual(updated["flows"]["playback"]["audio_pid"], 257)
            self.assertEqual(updated["flows"]["playback"]["video_stream_type"], 2)
            self.assertEqual(updated["flows"]["playback"]["audio_stream_type"], 2)
            self.assertEqual(json.loads(path.read_text())["flows"]["record"]["pid"], 272)

    def test_device_resolution_rejects_unsupported_live_codec_mapping(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "profile.json"
            profile = self.profile(resolved=False)
            profile["frontend"]["frequency_hz"] = 557142857
            save_profile(path, profile)
            pat_semantics = {"pmt_pids": [256], "services": []}
            full_semantics = {
                "pmt_pids": [256],
                "services": [{
                    "service_id": 100,
                    "pmt_pid": 256,
                    "pcr_pid": 272,
                    "streams": [
                        {"pid": 272, "stream_type": 0x24},
                        {"pid": 273, "stream_type": 0x11},
                    ],
                }],
            }
            session = MagicMock()
            session.__enter__.return_value = session
            session.section.side_effect = [(0x0000, b"pat"), (0x0100, b"pmt"), (0x0011, b"sdt")]
            with (
                patch("vts_profile.device._prepare_agent", return_value=("/data/local/tmp/agent", False)),
                patch("vts_profile.device._AgentSession", return_value=session),
                patch("vts_profile.device._si_query", side_effect=[pat_semantics, full_semantics]),
                patch("vts_profile.device._cleanup_agent"),
            ):
                with self.assertRaisesRegex(
                    ProfileError,
                    "resolved PMT has no supported audio elementary stream",
                ):
                    resolve_device(path)

    def test_device_resolution_failure_does_not_modify_profile(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "profile.json"
            original = self.profile(resolved=False)
            original["frontend"]["frequency_hz"] = 557142857
            save_profile(path, original)
            session = MagicMock()
            session.__enter__.return_value = session
            session.section.side_effect = ProfileError("no lock")
            with (
                patch("vts_profile.device._prepare_agent", return_value=("/data/local/tmp/agent", False)),
                patch("vts_profile.device._AgentSession", return_value=session),
                patch("vts_profile.device._cleanup_agent"),
            ):
                with self.assertRaises(ProfileError):
                    resolve_device(path)
            self.assertEqual(json.loads(path.read_text()), original)

    def test_architecture_matches_approved_resolver_design(self) -> None:
        tuner_hal2 = Path(__file__).resolve().parents[1]
        repo = tuner_hal2.parent
        agent = (tuner_hal2 / "vts_agent/main.rs").read_text()
        device = (tuner_hal2 / "tools/vts_profile/device.py").read_text()
        host = (repo / "arib_si_engine_rs/src/vts_profile_host.rs").read_text()
        arib_bp = (repo / "arib_si_engine_rs/Android.bp").read_text()
        service_wrapper = (repo / "arib_si_engine_rs/src/service_discovery.rs").read_text()
        product = (tuner_hal2 / "config/product_integration.mk").read_text()
        test_product = (tuner_hal2 / "config/vts_test_agent_integration.mk").read_text()
        self.assertIn("DemuxTsFilterType::SECTION", agent)
        self.assertIn('.get("op")', agent)
        self.assertNotIn("parse_pat", agent)
        self.assertNotIn("parse_pmt", agent)
        self.assertNotIn("SectionAssembler", agent)
        self.assertIn("class _AgentSession", device)
        self.assertNotIn("_run_agent_payload", device)
        self.assertIn("with _AgentSession(", device)
        self.assertIn("session.section(0x0011, 0x42)", device)
        self.assertNotIn("include!", host)
        self.assertIn("maleicacid_arib_si_engine_core", host)
        self.assertIn("ServiceDiscoveryCollector", host)
        self.assertIn("pmt_pids_for_section_filters", host)
        self.assertEqual(service_wrapper.strip(), "pub use maleicacid_arib_si_engine_core::service_discovery::*;")
        self.assertIn('name: "libmaleicacid_arib_si_engine_core"', arib_bp)
        self.assertGreaterEqual(arib_bp.count('"libmaleicacid_arib_si_engine_core"'), 3)
        self.assertNotIn("maleicacid_tuner_hal2_vts_agent", product)
        self.assertIn("maleicacid_tuner_hal2_vts_agent", test_product)
        self.assertTrue((tuner_hal2 / "tools/pyproject.toml").is_file())
        self.assertTrue((tuner_hal2 / "tools/uv.lock").is_file())


if __name__ == "__main__":
    unittest.main()
