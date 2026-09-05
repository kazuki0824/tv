from __future__ import annotations

import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest.mock import call, patch

from vts_profile.cli import build_parser
from vts_profile.install import (
    VARIANT_PROPERTY,
    _run_adb_text,
    install_device,
)
from vts_profile.model import ProfileError


_HARDWARE_INTERFACES_ROOT = Path("hardware/interfaces")
_VALIDATED_XML = "<validated/>\n"


def _profile(variant: str = "") -> dict:
    return {"vts": {"variant": variant}}


class VtsInstallDeviceTest(unittest.TestCase):
    def test_success_installs_and_verifies_compiled_xml(self) -> None:
        profile = _profile()
        payload = _VALIDATED_XML.encode("utf-8")
        with tempfile.TemporaryDirectory() as directory:
            artifact = Path(directory) / "tuner_vts_config_aidl_V1.xml"
            artifact.write_bytes(payload)
            with (
                patch("vts_profile.install.load_profile", return_value=profile),
                patch("vts_profile.install.validated_xml", return_value=_VALIDATED_XML) as validate,
                patch(
                    "vts_profile.install._run_adb_text",
                    side_effect=["", "", "", "0", "", ""],
                ) as adb_text,
                patch(
                    "vts_profile.install._run_adb_bytes",
                    return_value=payload,
                ) as adb_bytes,
            ):
                remote = install_device(
                    Path("profile.json"),
                    hardware_interfaces_root=_HARDWARE_INTERFACES_ROOT,
                    adb="adb-custom",
                    serial="SERIAL",
                    artifact=artifact,
                )
        self.assertEqual(remote, "/vendor/etc/tuner_vts_config_aidl_V1.xml")
        validate.assert_called_once_with(
            profile,
            hardware_interfaces_root=_HARDWARE_INTERFACES_ROOT,
            capability_source=validate.call_args.kwargs["capability_source"],
            pes_source=validate.call_args.kwargs["pes_source"],
            playback_source=validate.call_args.kwargs["playback_source"],
            rustc="rustc",
            xmllint="xmllint",
        )
        self.assertEqual(
            adb_text.call_args_list,
            [
                call("adb-custom", "SERIAL", "shell", "getprop", VARIANT_PROPERTY),
                call("adb-custom", "SERIAL", "root"),
                call("adb-custom", "SERIAL", "wait-for-device"),
                call("adb-custom", "SERIAL", "shell", "id", "-u"),
                call("adb-custom", "SERIAL", "remount"),
                call(
                    "adb-custom",
                    "SERIAL",
                    "push",
                    str(artifact),
                    "/vendor/etc/tuner_vts_config_aidl_V1.xml",
                ),
            ],
        )
        adb_bytes.assert_called_once_with(
            "adb-custom",
            "SERIAL",
            "exec-out",
            "cat",
            "/vendor/etc/tuner_vts_config_aidl_V1.xml",
        )

    def test_stale_artifact_is_rejected_before_adb(self) -> None:
        profile = _profile()
        with tempfile.TemporaryDirectory() as directory:
            artifact = Path(directory) / "tuner_vts_config_aidl_V1.xml"
            artifact.write_text("<stale/>\n", encoding="utf-8")
            with (
                patch("vts_profile.install.load_profile", return_value=profile),
                patch("vts_profile.install.validated_xml", return_value=_VALIDATED_XML),
                patch("vts_profile.install._run_adb_text") as adb_text,
            ):
                with self.assertRaisesRegex(ProfileError, "does not match the current fully validated profile"):
                    install_device(
                        Path("profile.json"),
                        hardware_interfaces_root=_HARDWARE_INTERFACES_ROOT,
                        artifact=artifact,
                    )
        adb_text.assert_not_called()

    def test_variant_mismatch_is_rejected_before_root(self) -> None:
        profile = _profile("lab")
        payload = _VALIDATED_XML.encode("utf-8")
        with tempfile.TemporaryDirectory() as directory:
            artifact = Path(directory) / "tuner_vts_config_aidl_V1.lab.xml"
            artifact.write_bytes(payload)
            with (
                patch("vts_profile.install.load_profile", return_value=profile),
                patch("vts_profile.install.validated_xml", return_value=_VALIDATED_XML),
                patch("vts_profile.install._run_adb_text", return_value="") as adb_text,
            ):
                with self.assertRaisesRegex(ProfileError, "does not match profile variant"):
                    install_device(
                        Path("profile.json"),
                        hardware_interfaces_root=_HARDWARE_INTERFACES_ROOT,
                        artifact=artifact,
                    )
        adb_text.assert_called_once_with(
            "adb", None, "shell", "getprop", VARIANT_PROPERTY
        )

    def test_non_root_adbd_is_rejected_before_remount(self) -> None:
        profile = _profile()
        with tempfile.TemporaryDirectory() as directory:
            artifact = Path(directory) / "tuner_vts_config_aidl_V1.xml"
            artifact.write_text(_VALIDATED_XML, encoding="utf-8")
            with (
                patch("vts_profile.install.load_profile", return_value=profile),
                patch("vts_profile.install.validated_xml", return_value=_VALIDATED_XML),
                patch(
                    "vts_profile.install._run_adb_text",
                    side_effect=["", "", "", "2000"],
                ) as adb_text,
            ):
                with self.assertRaisesRegex(ProfileError, "did not produce uid 0"):
                    install_device(
                        Path("profile.json"),
                        hardware_interfaces_root=_HARDWARE_INTERFACES_ROOT,
                        artifact=artifact,
                    )
        self.assertEqual(adb_text.call_count, 4)

    def test_wrong_artifact_filename_is_rejected_without_adb(self) -> None:
        profile = _profile()
        with tempfile.TemporaryDirectory() as directory:
            artifact = Path(directory) / "wrong.xml"
            artifact.write_text(_VALIDATED_XML, encoding="utf-8")
            with (
                patch("vts_profile.install.load_profile", return_value=profile),
                patch("vts_profile.install.validated_xml", return_value=_VALIDATED_XML),
                patch("vts_profile.install._run_adb_text") as adb_text,
            ):
                with self.assertRaisesRegex(ProfileError, "compiled artifact filename"):
                    install_device(
                        Path("profile.json"),
                        hardware_interfaces_root=_HARDWARE_INTERFACES_ROOT,
                        artifact=artifact,
                    )
        adb_text.assert_not_called()

    def test_readback_mismatch_is_fail_closed(self) -> None:
        profile = _profile()
        with tempfile.TemporaryDirectory() as directory:
            artifact = Path(directory) / "tuner_vts_config_aidl_V1.xml"
            artifact.write_text(_VALIDATED_XML, encoding="utf-8")
            with (
                patch("vts_profile.install.load_profile", return_value=profile),
                patch("vts_profile.install.validated_xml", return_value=_VALIDATED_XML),
                patch(
                    "vts_profile.install._run_adb_text",
                    side_effect=["", "", "", "0", "", ""],
                ),
                patch(
                    "vts_profile.install._run_adb_bytes",
                    return_value=b"different\n",
                ),
            ):
                with self.assertRaisesRegex(ProfileError, "readback does not match"):
                    install_device(
                        Path("profile.json"),
                        hardware_interfaces_root=_HARDWARE_INTERFACES_ROOT,
                        artifact=artifact,
                    )

    def test_adb_execution_failure_is_reported(self) -> None:
        failed = subprocess.CompletedProcess(
            ["adb", "remount"],
            1,
            stdout="",
            stderr="remount failed",
        )
        with patch("vts_profile.install.subprocess.run", return_value=failed):
            with self.assertRaisesRegex(ProfileError, "adb remount failed: remount failed"):
                _run_adb_text("adb", None, "remount")

    def test_parser_exposes_install_device_inputs(self) -> None:
        args = build_parser().parse_args(
            [
                "install-device",
                "profile.json",
                "--adb",
                "adb-custom",
                "--serial",
                "SERIAL",
                "--artifact",
                "compiled.xml",
            ]
        )
        self.assertEqual(args.profile, "profile.json")
        self.assertEqual(args.adb, "adb-custom")
        self.assertEqual(args.serial, "SERIAL")
        self.assertEqual(args.artifact, "compiled.xml")


if __name__ == "__main__":
    unittest.main()
