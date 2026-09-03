from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

from vts_profile.model import ProfileError, validate_profile
from vts_profile.resource_closure import _program

from test_vts_profile import VtsProfileTest


class RecordFilterFmqProfileTest(unittest.TestCase):
    def profile(self) -> dict:
        return VtsProfileTest().profile()

    def test_record_filter_fmq_probe_requires_record_flow(self) -> None:
        profile = self.profile()
        profile["vts"]["variant"] = "record-filter-fmq"
        profile["flows"]["record"] = {"enabled": False}
        profile["queues"] = {}

        with self.assertRaisesRegex(
            ProfileError,
            "record-filter-fmq VTS variant requires flows.record.enabled=true",
        ):
            validate_profile(profile, require_resolved=True)

    def test_resource_checker_includes_production_filter_config_ssot(self) -> None:
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
        self.assertIn(str(filter_config.resolve()), program)
        self.assertNotIn("pub enum FilterOpenType { TsRaw", program)
        self.assertNotIn("matches!(self, Self::TsRaw | Self::TsSection", program)


if __name__ == "__main__":
    unittest.main()
