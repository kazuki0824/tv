from __future__ import annotations

import json
import subprocess
import tempfile
import unittest
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
                "clear_live": {"enabled": False},
            },
            "queues": {"record_filter_bytes": 1048576, "record_dvr_bytes": 4194304},
        }

    def test_record_only_xml_is_generated_without_device(self) -> None:
        xml = render_xml(self.profile())
        self.assertIn('frequency="557142857"', xml)
        self.assertIn('pid="272"', xml)

    def test_region_resolution_and_selection_update_same_profile(self) -> None:
        profile = self.profile(resolved=False)
        profile["region"] = {"query": "test", "candidates": []}
        dataset = {
            "schema_version": 1,
            "dataset_version": "v1",
            "entries": [
                {"region": "test", "delivery_system": "ISDBT", "physical_channel": 22,
                 "frequency_hz": 527142857, "label": "A"},
                {"region": "test", "delivery_system": "ISDBT", "physical_channel": 27,
                 "frequency_hz": 557142857, "label": "B"},
            ],
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
        self.assertIn("reserve_dvr(snapshot, 1, 4194304)", program)

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
                    validate_resource_closure(
                        self.profile(), capability_source=capability, pes_source=pes
                    )

    def test_noninteractive_init_requires_explicit_inputs(self) -> None:
        args = SimpleNamespace(
            non_interactive=True, backend="px4", product="default", delivery_system="ISDBT",
            vts_source_ref="aosp-commit", region=None, frequency_hz="557142857", service_id=None,
            record="yes", record_pid="272", scan="yes", record_filter_bytes="1048576",
            record_dvr_bytes="4194304", variant="",
        )
        self.assertEqual(_new_profile(args)["vts"]["source_ref"], "aosp-commit")

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
            commit = subprocess.run(
                ["git", "-C", str(root), "rev-parse", "HEAD"], check=True,
                capture_output=True, text=True,
            ).stdout.strip()
            self.assertEqual(selected_xsd(root, commit), xsd)
            with self.assertRaises(ProfileError):
                selected_xsd(root, "HEAD~1")

    def test_xsd_validator_and_product_variant_are_fail_closed(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            xsd = root / "schema.xsd"; xsd.write_text("<x/>")
            ok = root / "ok"; ok.write_text("#!/bin/sh\nexit 0\n"); ok.chmod(0o755)
            bad = root / "bad"; bad.write_text("#!/bin/sh\nexit 3\n"); bad.chmod(0o755)
            validate_xml("<root/>", xsd, xmllint=str(ok))
            with self.assertRaises(ProfileError):
                validate_xml("<root/>", xsd, xmllint=str(bad))
            profile = self.profile(); profile["vts"]["variant"] = "lab"
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
            session.section.side_effect = [
                (0x0000, b"pat"),
                (0x0100, b"pmt"),
                (0x0011, b"sdt"),
            ]
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
            self.assertEqual(json.loads(path.read_text())["flows"]["record"]["pid"], 272)

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
        self.assertIn('request.get("op")', agent)
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
        self.assertEqual(
            service_wrapper.strip(),
            "pub use maleicacid_arib_si_engine_core::service_discovery::*;",
        )
        self.assertIn('name: "libmaleicacid_arib_si_engine_core"', arib_bp)
        self.assertGreaterEqual(arib_bp.count('"libmaleicacid_arib_si_engine_core"'), 3)

        self.assertNotIn("maleicacid_tuner_hal2_vts_agent", product)
        self.assertIn("maleicacid_tuner_hal2_vts_agent", test_product)
        self.assertTrue((tuner_hal2 / "tools/pyproject.toml").is_file())
        self.assertTrue((tuner_hal2 / "tools/uv.lock").is_file())


if __name__ == "__main__":
    unittest.main()
