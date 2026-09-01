from __future__ import annotations

import json
import subprocess
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

from vts_profile.cli import _new_profile
from vts_profile.device import resolve_device
from vts_profile.integration import write_product_artifacts
from vts_profile.model import ProfileError, parse_capability_source, save_profile, validate_against_capability, validate_profile
from vts_profile.region import resolve_region, select_candidate
from vts_profile.render import render_xml
from vts_profile.schema import selected_xsd, validate_xml

CAPABILITY_SOURCE = """
impl CapabilitySnapshot {
    pub const fn product_default() -> Self {
        Self {
            num_record: 8,
            num_playback: 8,
            num_ts_filter: 32,
            num_section_filter: 8,
            num_audio_filter: 0,
            num_video_filter: 0,
            num_pes_filter: 4,
            num_pcr_filter: 4,
            fmq_runtime_budget_bytes: 256 * MIB,
        }
    }
    pub const fn filter_capacity(self) {}
}
"""


class VtsProfileTest(unittest.TestCase):
    def profile(self, *, resolved: bool = True) -> dict:
        return {
            "schema_version": 1,
            "target": {"hal": "tuner_hal2", "product": "default", "backend": "px4"},
            "vts": {"contract": "android14-aidl-v1", "source_ref": "aosp-commit", "variant": ""},
            "frontend": {"type": "ISDBT", "is_software_frontend": False, "frequency_hz": 557142857 if resolved else None},
            "flows": {
                "scan": True,
                "record": {"enabled": True, "pid": 272 if resolved else None},
                "clear_live": {"enabled": False},
            },
            "queues": {"record_filter_bytes": 1048576, "record_dvr_bytes": 4194304},
        }

    def capability(self) -> dict[str, int]:
        return {
            "num_record": 8, "num_playback": 8, "num_ts_filter": 32, "num_section_filter": 8,
            "num_audio_filter": 0, "num_video_filter": 0, "num_pes_filter": 4, "num_pcr_filter": 4,
            "fmq_runtime_budget_bytes": 256 * 1024 * 1024,
        }

    def test_record_only_xml_is_generated_without_device(self) -> None:
        xml = render_xml(self.profile(), self.capability())
        self.assertIn('frequency="557142857"', xml)
        self.assertIn('pid="272"', xml)
        self.assertNotIn("isdbtFrontendSettings", xml)
        self.assertNotIn("supportBlindScan", xml)

    def test_region_resolution_updates_same_profile(self) -> None:
        profile = self.profile(resolved=False)
        profile["region"] = {"query": "test", "candidates": []}
        dataset = {"schema_version": 1, "dataset_version": "v1", "entries": [
            {"region": "test", "delivery_system": "ISDBT", "physical_channel": 27, "frequency_hz": 557142857, "label": "A"},
        ]}
        resolve_region(profile, dataset)
        self.assertEqual(profile["frontend"]["frequency_hz"], 557142857)

    def test_select_candidate_uses_saved_candidates(self) -> None:
        profile = self.profile(resolved=False)
        profile["region"] = {"query": "test", "candidates": [
            {"delivery_system": "ISDBT", "physical_channel": 22, "frequency_hz": 527142857, "label": "A"},
            {"delivery_system": "ISDBT", "physical_channel": 27, "frequency_hz": 557142857, "label": "B"},
        ]}
        select_candidate(profile, 1)
        self.assertEqual(profile["frontend"]["frequency_hz"], 557142857)

    def test_clear_live_is_rejected_by_current_capability(self) -> None:
        profile = self.profile()
        profile["flows"]["clear_live"] = {
            "enabled": True, "audio_pid": 273, "video_pid": 272,
            "audio_stream_type": 2, "video_stream_type": 2,
        }
        profile["queues"].update({"audio_filter_bytes": 1048576, "video_filter_bytes": 1048576})
        with self.assertRaises(ProfileError):
            validate_against_capability(profile, self.capability())

    def test_unknown_profile_field_is_rejected(self) -> None:
        profile = self.profile()
        profile["unused"] = True
        with self.assertRaises(ProfileError):
            validate_profile(profile)

    def test_capability_is_read_from_rust_ssot(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "capability_snapshot.rs"
            path.write_text(CAPABILITY_SOURCE, encoding="utf-8")
            capability = parse_capability_source(path)
        self.assertEqual(capability["num_audio_filter"], 0)
        self.assertEqual(capability["fmq_runtime_budget_bytes"], 256 * 1024 * 1024)

    def test_noninteractive_init_requires_explicit_inputs(self) -> None:
        args = SimpleNamespace(
            non_interactive=True, backend="px4", product="default", delivery_system="ISDBT",
            vts_source_ref="aosp-commit", region=None, frequency_hz="557142857", service_id=None,
            record="yes", record_pid="272", scan="yes", record_filter_bytes="1048576",
            record_dvr_bytes="4194304", variant="",
        )
        profile = _new_profile(args)
        self.assertEqual(profile["vts"]["source_ref"], "aosp-commit")

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

    def test_xsd_validator_is_fail_closed(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            xsd = root / "schema.xsd"
            xsd.write_text("<x/>")
            ok = root / "ok"
            ok.write_text("#!/bin/sh\nexit 0\n")
            ok.chmod(0o755)
            bad = root / "bad"
            bad.write_text("#!/bin/sh\necho invalid >&2\nexit 3\n")
            bad.chmod(0o755)
            validate_xml("<root/>", xsd, xmllint=str(ok))
            with self.assertRaises(ProfileError):
                validate_xml("<root/>", xsd, xmllint=str(bad))

    def test_product_integration_uses_same_variant(self) -> None:
        profile = self.profile()
        profile["vts"]["variant"] = "lab"
        with tempfile.TemporaryDirectory() as directory:
            path = write_product_artifacts(profile, "<validated/>", Path(directory))
            self.assertEqual(path.name, "tuner_vts_config_aidl_V1.lab.xml")
            mk = (Path(directory) / "vts_product_generated.mk").read_text()
            self.assertIn("tuner_vts_config_aidl_V1.lab.xml:$(TARGET_COPY_OUT_VENDOR)/etc/tuner_vts_config_aidl_V1.lab.xml", mk)
            self.assertIn("ro.vendor.vts_tuner_configuration_variant=lab", mk)

    def test_device_resolver_updates_same_profile(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "profile.json"
            profile = self.profile(resolved=False)
            profile["frontend"]["frequency_hz"] = 557142857
            save_profile(path, profile)
            payload = {
                "frequency_hz": 557142857, "service_id": 100, "pmt_pid": 256,
                "video_pid": 272, "audio_pid": 273, "elementary_pids": [272, 273],
            }
            with patch("vts_profile.device.subprocess.run", return_value=SimpleNamespace(returncode=0, stdout=json.dumps(payload) + "\n", stderr="")):
                updated = resolve_device(path)
            self.assertEqual(updated["flows"]["record"]["pid"], 272)
            self.assertEqual(updated["service"]["service_id"], 100)
            self.assertEqual(json.loads(path.read_text())["flows"]["record"]["pid"], 272)

    def test_device_resolver_failure_does_not_modify_profile(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "profile.json"
            original = self.profile(resolved=False)
            original["frontend"]["frequency_hz"] = 557142857
            save_profile(path, original)
            with patch("vts_profile.device.subprocess.run", return_value=SimpleNamespace(returncode=2, stdout="", stderr="no lock")):
                with self.assertRaises(ProfileError):
                    resolve_device(path)
            self.assertEqual(json.loads(path.read_text()), original)


if __name__ == "__main__":
    unittest.main()
