from __future__ import annotations

import unittest
from pathlib import Path
from unittest.mock import patch

from vts_profile.compiler import validated_xml


class VtsProfileCompilerTest(unittest.TestCase):
    def test_validated_xml_runs_profile_closure_schema_path(self) -> None:
        profile = {"vts": {"source_ref": "commit"}}
        root = Path("hardware/interfaces")
        xsd = root / "tv/tuner/config/tuner_testing_dynamic_configuration.xsd"
        with (
            patch("vts_profile.compiler.validate_profile") as validate_profile,
            patch("vts_profile.compiler.validate_resource_closure") as validate_closure,
            patch("vts_profile.compiler.render_xml", return_value="<validated/>\n") as render,
            patch("vts_profile.compiler.selected_xsd", return_value=xsd) as select_xsd,
            patch("vts_profile.compiler.validate_xml") as validate_schema,
        ):
            xml = validated_xml(
                profile,
                hardware_interfaces_root=root,
                capability_source=Path("capability.rs"),
                pes_source=Path("pes.rs"),
                playback_source=Path("playback.rs"),
                rustc="rustc-custom",
                xmllint="xmllint-custom",
            )
        self.assertEqual(xml, "<validated/>\n")
        validate_profile.assert_called_once_with(profile, require_resolved=True)
        validate_closure.assert_called_once_with(
            profile,
            capability_source=Path("capability.rs"),
            pes_source=Path("pes.rs"),
            playback_source=Path("playback.rs"),
            rustc="rustc-custom",
        )
        render.assert_called_once_with(profile)
        select_xsd.assert_called_once_with(root, "commit")
        validate_schema.assert_called_once_with(
            "<validated/>\n",
            xsd,
            xmllint="xmllint-custom",
        )


if __name__ == "__main__":
    unittest.main()
