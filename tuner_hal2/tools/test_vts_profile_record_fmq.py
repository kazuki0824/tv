from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

from vts_profile.model import ProfileError, validate_profile
from vts_profile.render import render_xml
from vts_profile.resource_closure import _program


class RecordFilterFmqProfileTest(unittest.TestCase):
    def profile(self) -> dict:
        return {
            "schema_version": 1,
            "target": {"hal": "tuner_hal2", "product": "default", "backend": "px4"},
            "vts": {
                "contract": "android14-aidl-v1",
                "source_ref": "aosp-commit",
                "variant": "record-filter-fmq",
            },
            "frontend": {
                "type": "ISDBT",
                "is_software_frontend": False,
                "frequency_hz": 557142857,
            },
            "flows": {
                "scan": True,
                "record": {"enabled": True, "pid": 272},
                "clear_live": {"enabled": False},
                "playback": {"enabled": False},
            },
            "queues": {
                "record_filter_bytes": 1048576,
                "record_dvr_bytes": 4194304,
            },
        }

    def test_record_filter_fmq_probe_requires_record_flow(self) -> None:
        profile = self.profile()
        profile["flows"]["record"] = {"enabled": False}
        profile["queues"] = {}

        with self.assertRaisesRegex(
            ProfileError,
            "record-filter-fmq VTS variant requires flows.record.enabled=true",
        ):
            validate_profile(profile, require_resolved=True)

    def test_isdbt_renderer_keeps_aosp_unspecified_settings_path(self) -> None:
        xml = render_xml(self.profile())
        self.assertIn('type="ISDBT"', xml)
        self.assertIn('frequency="557142857"', xml)
        self.assertNotIn("<isdbtFrontendSettings", xml)

    def test_resource_checker_uses_supplied_filter_config_source(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            capability = root / "capability_snapshot.rs"
            capability.write_text("// fixture", encoding="utf-8")
            filter_config = root / "config.rs"
            filter_config.write_text("// production config fixture", encoding="utf-8")
            program = _program(
                self.profile(),
                capability,
                1024 * 1024,
                filter_config,
            )

        self.assertIn("mod production_demux_config", program)
        self.assertIn("// production config fixture", program)
        self.assertNotIn("pub enum FilterOpenType { TsRaw", program)


if __name__ == "__main__":
    unittest.main()
