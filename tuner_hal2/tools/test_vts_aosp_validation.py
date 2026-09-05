from __future__ import annotations

import tempfile
import unittest
from pathlib import Path
from unittest.mock import call, patch

from vts_profile.model import ProfileError
from vts_profile.schema import (
    AOSP_VALIDATOR_TARGET,
    _build_aosp_consumer,
    validate_xml_with_aosp_consumer,
)


class AospConsumerValidationTest(unittest.TestCase):
    def test_validator_is_built_from_same_aosp_tree(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            interfaces = root / "hardware/interfaces"
            xsd = interfaces / "tv/tuner/config/tuner_testing_dynamic_configuration.xsd"
            xsd.parent.mkdir(parents=True)
            xsd.write_text("<x/>", encoding="utf-8")
            validator = root / "validator"
            validator.write_text("#!/bin/sh\nexit 0\n", encoding="utf-8")
            validator.chmod(0o755)

            with (
                patch("vts_profile.schema._git_commit", return_value="same"),
                patch("vts_profile.schema._build_aosp_consumer", return_value=validator),
            ):
                validate_xml_with_aosp_consumer(
                    "<root/>",
                    aosp_root=root,
                    hardware_interfaces_root=interfaces,
                    source_ref="selected-ref",
                )

    def test_validator_rejects_interfaces_from_another_tree(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / "aosp"
            other = Path(directory) / "other/hardware/interfaces"
            root.mkdir(parents=True)
            other.mkdir(parents=True)
            with self.assertRaisesRegex(ProfileError, "same AOSP tree"):
                validate_xml_with_aosp_consumer(
                    "<root/>",
                    aosp_root=root,
                    hardware_interfaces_root=other,
                    source_ref="selected-ref",
                )

    def test_build_uses_repository_soong_target_and_host_out(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            host_out = root / "out/host/linux-x86/bin"
            host_out.mkdir(parents=True)
            validator = host_out / AOSP_VALIDATOR_TARGET
            validator.write_text("binary", encoding="utf-8")

            with patch(
                "vts_profile.schema._run_aosp_build_command",
                side_effect=["", str(host_out)],
            ) as run:
                self.assertEqual(_build_aosp_consumer(root), validator)

            self.assertEqual(
                run.call_args_list,
                [
                    call(
                        root.resolve(),
                        ["--make-mode", AOSP_VALIDATOR_TARGET],
                        label="failed to build AOSP xsdc Tuner config validator",
                    ),
                    call(
                        root.resolve(),
                        ["--dumpvar-mode", "HOST_OUT_EXECUTABLES"],
                        label="failed to resolve AOSP HOST_OUT_EXECUTABLES",
                    ),
                ],
            )

    def test_consumer_rejection_is_fail_closed(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            interfaces = root / "hardware/interfaces"
            xsd = interfaces / "tv/tuner/config/tuner_testing_dynamic_configuration.xsd"
            xsd.parent.mkdir(parents=True)
            xsd.write_text("<x/>", encoding="utf-8")
            validator = root / "validator"
            validator.write_text("#!/bin/sh\necho rejected >&2\nexit 1\n", encoding="utf-8")
            validator.chmod(0o755)

            with (
                patch("vts_profile.schema._git_commit", return_value="same"),
                patch("vts_profile.schema._build_aosp_consumer", return_value=validator),
            ):
                with self.assertRaisesRegex(ProfileError, "rejected"):
                    validate_xml_with_aosp_consumer(
                        "<root/>",
                        aosp_root=root,
                        hardware_interfaces_root=interfaces,
                        source_ref="selected-ref",
                    )


if __name__ == "__main__":
    unittest.main()
