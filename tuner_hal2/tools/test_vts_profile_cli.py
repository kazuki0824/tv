from __future__ import annotations

import json
import os
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import call, patch

from vts_profile import cli
from vts_profile.model import ProfileError


class VtsProfileCliTest(unittest.TestCase):
    def test_default_profile_is_repo_root_relative_not_cwd_relative(self) -> None:
        expected = Path(cli.__file__).resolve().parents[3] / (
            "tuner_hal2/config/vts_environment_profile.json"
        )
        self.assertEqual(cli.DEFAULT_PROFILE, expected)
        self.assertTrue(cli.DEFAULT_PROFILE.is_absolute())

    def test_product_uses_android_target_before_project_default(self) -> None:
        args = SimpleNamespace(product=None)
        with patch.dict(
            os.environ,
            {"TARGET_DEVICE": "virtio_x86_64_tv_grub", "TARGET_PRODUCT": "lineage_other"},
            clear=True,
        ):
            self.assertEqual(cli._product(args), "virtio_x86_64_tv_grub")
        with patch.dict(os.environ, {"TARGET_PRODUCT": "lineage_virtio_x86_64_tv_grub"}, clear=True):
            self.assertEqual(cli._product(args), "virtio_x86_64_tv_grub")
        with patch.dict(os.environ, {}, clear=True):
            self.assertEqual(cli._product(args), cli.DEFAULT_PRODUCT)

    def test_vts_source_ref_uses_exact_detected_checkout_commit(self) -> None:
        args = SimpleNamespace(vts_source_ref=None, hardware_interfaces_root=None)
        root = Path("/aosp/hardware/interfaces")
        with (
            patch("vts_profile.cli._hardware_interfaces_root", return_value=root),
            patch("vts_profile.cli.checkout_commit", return_value="deadbeef") as checkout,
        ):
            self.assertEqual(cli._source_ref(args), "deadbeef")
        checkout.assert_called_once_with(root)

    def test_vts_source_ref_fails_closed_when_checkout_cannot_be_detected(self) -> None:
        args = SimpleNamespace(vts_source_ref=None, hardware_interfaces_root=None)
        with patch("vts_profile.cli._hardware_interfaces_root", return_value=None):
            with self.assertRaisesRegex(ProfileError, "cannot locate hardware/interfaces checkout"):
                cli._source_ref(args)

    def test_interactive_choices_are_numbered_and_have_defaults(self) -> None:
        args = SimpleNamespace(non_interactive=False, backend=None)
        with patch("builtins.input", return_value=""):
            self.assertEqual(
                cli._choice(args, "backend", "backend", cli.SUPPORTED_BACKENDS, "px4"),
                "px4",
            )
        with patch("builtins.input", return_value="2"):
            self.assertEqual(
                cli._choice(args, "backend", "backend", cli.SUPPORTED_BACKENDS, "px4"),
                "earth_pt1",
            )

    def test_boolean_cli_override_rejects_typos(self) -> None:
        self.assertTrue(cli._boolean_option(None, "record", default=True))
        self.assertFalse(cli._boolean_option("no", "record", default=True))
        with self.assertRaisesRegex(ProfileError, "record must be yes/no"):
            cli._boolean_option("yse", "record", default=True)

    def test_interactive_init_groups_optional_values_after_choices(self) -> None:
        args = SimpleNamespace(
            non_interactive=False,
            backend=None,
            product=None,
            delivery_system=None,
            vts_source_ref=None,
            hardware_interfaces_root=None,
            region=None,
            frequency_hz=None,
            service_id=None,
            record=None,
            record_pid=None,
            scan=None,
            record_filter_bytes=1048576,
            record_dvr_bytes=4194304,
            playback_dvr_bytes=4194304,
            playback_input_path="/data/local/tmp/segment000000.ts",
            variant="",
        )
        with (
            patch("vts_profile.cli._source_ref", return_value="aosp-commit"),
            patch(
                "builtins.input",
                side_effect=["", "", "神奈川県座間市", "", "", ""],
            ) as user_input,
            patch.dict(os.environ, {}, clear=True),
        ):
            profile = cli._new_profile(args)

        self.assertEqual(profile["target"]["product"], cli.DEFAULT_PRODUCT)
        self.assertEqual(profile["target"]["backend"], "px4")
        self.assertEqual(profile["frontend"]["type"], "ISDBT")
        self.assertEqual(profile["region"]["query"], "神奈川県座間市")
        self.assertTrue(profile["flows"]["scan"])
        self.assertTrue(profile["flows"]["record"]["enabled"])
        self.assertEqual(
            user_input.call_args_list,
            [
                call("select [1]: "),
                call("select [1]: "),
                call("region address/postal/latitude,longitude (optional): "),
                call("frequency Hz (optional): "),
                call("service ID (optional): "),
                call("record PID (optional): "),
            ],
        )

    def test_interactive_init_auto_resolves_isdbt_region_after_checkpoint(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "profile.json"
            profile = {
                "frontend": {"type": "ISDBT", "frequency_hz": None},
                "region": {"query": "神奈川県座間市", "candidates": []},
            }
            args = SimpleNamespace(profile=str(path), non_interactive=False)

            def resolve(profile_to_resolve: dict) -> None:
                checkpoint = json.loads(path.read_text(encoding="utf-8"))
                self.assertEqual(checkpoint["region"]["query"], "神奈川県座間市")
                self.assertEqual(checkpoint["region"]["candidates"], [])
                profile_to_resolve["region"]["candidates"] = [
                    {
                        "frequency_hz": 515142857,
                        "physical_channel": 20,
                        "label": "座間市 ch20",
                    }
                ]

            with (
                patch("vts_profile.cli._new_profile", return_value=profile),
                patch("vts_profile.cli.resolve_region", side_effect=resolve) as resolver,
                patch("vts_profile.cli._print_region_candidates"),
            ):
                self.assertEqual(cli.cmd_init(args), 0)

            resolver.assert_called_once_with(profile)
            saved = json.loads(path.read_text(encoding="utf-8"))
            self.assertEqual(saved["region"]["candidates"][0]["physical_channel"], 20)

    def test_interactive_init_keeps_checkpoint_if_region_resolution_fails(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "profile.json"
            profile = {
                "frontend": {"type": "ISDBT", "frequency_hz": None},
                "region": {"query": "神奈川県座間市", "candidates": []},
            }
            args = SimpleNamespace(profile=str(path), non_interactive=False)
            with (
                patch("vts_profile.cli._new_profile", return_value=profile),
                patch(
                    "vts_profile.cli.resolve_region",
                    side_effect=ProfileError("regional lookup unavailable"),
                ),
            ):
                with self.assertRaisesRegex(ProfileError, "regional lookup unavailable"):
                    cli.cmd_init(args)

            saved = json.loads(path.read_text(encoding="utf-8"))
            self.assertEqual(saved["region"]["query"], "神奈川県座間市")
            self.assertEqual(saved["region"]["candidates"], [])

    def test_noninteractive_init_preserves_explicit_region_resolution_phase(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "profile.json"
            profile = {
                "frontend": {"type": "ISDBT", "frequency_hz": None},
                "region": {"query": "神奈川県座間市", "candidates": []},
            }
            args = SimpleNamespace(profile=str(path), non_interactive=True)
            with (
                patch("vts_profile.cli._new_profile", return_value=profile),
                patch("vts_profile.cli.resolve_region") as resolver,
            ):
                self.assertEqual(cli.cmd_init(args), 0)
            resolver.assert_not_called()

    def test_interactive_isdbs_init_does_not_invoke_terrestrial_region_resolver(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "profile.json"
            profile = {
                "frontend": {"type": "ISDBS", "frequency_hz": None},
                "region": {"query": "神奈川県座間市", "candidates": []},
            }
            args = SimpleNamespace(profile=str(path), non_interactive=False)
            with (
                patch("vts_profile.cli._new_profile", return_value=profile),
                patch("vts_profile.cli.resolve_region") as resolver,
            ):
                self.assertEqual(cli.cmd_init(args), 0)
            resolver.assert_not_called()

    def test_all_profile_commands_default_to_the_single_ssot_path(self) -> None:
        parser = cli.build_parser()
        for argv in (
            ["init", "--non-interactive", "--vts-source-ref", "aosp-commit"],
            ["resolve-region"],
            ["select-candidate", "0"],
            ["validate"],
            ["compile"],
            ["resolve-device"],
        ):
            with self.subTest(argv=argv):
                args = parser.parse_args(argv)
                self.assertEqual(args.profile, str(cli.DEFAULT_PROFILE))


if __name__ == "__main__":
    unittest.main()
