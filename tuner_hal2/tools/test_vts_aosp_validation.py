from __future__ import annotations

import tempfile
import unittest
from pathlib import Path
from unittest.mock import call, patch

from vts_profile.model import ProfileError
from vts_profile.schema import (
    AOSP_XSDC_TARGET,
    _build_aosp_xsdc,
    _validate_with_selected_aosp_consumer,
    validate_xml_with_aosp_consumer,
)


class AospConsumerValidationTest(unittest.TestCase):
    def test_selected_ref_and_validator_are_bound_to_same_aosp_tree(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            interfaces = root / "hardware/interfaces"
            xsd = interfaces / "tv/tuner/config/tuner_testing_dynamic_configuration.xsd"
            xsd.parent.mkdir(parents=True)
            xsd.write_text("<x/>", encoding="utf-8")

            with (
                patch("vts_profile.schema._git_commit", return_value="same"),
                patch("vts_profile.schema._validate_with_selected_aosp_consumer") as validate,
            ):
                validate_xml_with_aosp_consumer(
                    "<root/>",
                    aosp_root=root,
                    hardware_interfaces_root=interfaces,
                    source_ref="selected-ref",
                )

            validate.assert_called_once_with(
                "<root/>", aosp_root=root.resolve(), xsd=xsd.resolve()
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

    def test_xsdc_is_built_from_selected_aosp_tree(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            soong_ui = root / "build/soong/soong_ui.bash"
            soong_ui.parent.mkdir(parents=True)
            soong_ui.write_text("#!/bin/sh\n", encoding="utf-8")
            host_out = root / "out/host/linux-x86/bin"
            host_out.mkdir(parents=True)
            xsdc = host_out / AOSP_XSDC_TARGET
            xsdc.write_text("binary", encoding="utf-8")

            with patch(
                "vts_profile.schema._run_checked",
                side_effect=["", str(host_out)],
            ) as run:
                self.assertEqual(_build_aosp_xsdc(root), xsdc)

            self.assertEqual(
                run.call_args_list,
                [
                    call(
                        [str(soong_ui), "--make-mode", AOSP_XSDC_TARGET],
                        cwd=root.resolve(),
                        label="failed to build AOSP xsdc",
                    ),
                    call(
                        [str(soong_ui), "--dumpvar-mode", "HOST_OUT_EXECUTABLES"],
                        cwd=root.resolve(),
                        label="failed to resolve AOSP HOST_OUT_EXECUTABLES",
                    ),
                ],
            )

    def test_consumer_rejection_is_fail_closed(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            xsd = root / "hardware/interfaces/tv/tuner/config/tuner_testing_dynamic_configuration.xsd"
            xsd.parent.mkdir(parents=True)
            xsd.write_text("<x/>", encoding="utf-8")
            validator = root / "validator"
            validator.write_text("#!/bin/sh\necho rejected >&2\nexit 1\n", encoding="utf-8")
            validator.chmod(0o755)

            with patch(
                "vts_profile.schema._compile_selected_aosp_consumer", return_value=validator
            ):
                with self.assertRaisesRegex(ProfileError, "rejected"):
                    _validate_with_selected_aosp_consumer(
                        "<root/>", aosp_root=root, xsd=xsd
                    )


if __name__ == "__main__":
    unittest.main()
