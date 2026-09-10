from __future__ import annotations

import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from vts_profile.device import AgentCleanupError, _cleanup_agent, _prepare_agent, resolve_device
from vts_profile.model import ProfileError


class VtsDeviceCleanupTest(unittest.TestCase):
    def test_remove_failure_is_reported(self) -> None:
        failed = subprocess.CompletedProcess([], 1, "", "削除拒否")
        with patch("vts_profile.device._run", return_value=failed):
            with self.assertRaisesRegex(ProfileError, "削除拒否"):
                _cleanup_agent(adb="adb", serial=None, pushed=True)

    def test_installed_agent_is_not_removed(self) -> None:
        with patch("vts_profile.device._run") as run:
            _cleanup_agent(adb="adb", serial=None, pushed=False)
        run.assert_not_called()

    def test_partial_push_and_cleanup_failures_are_both_retained(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            binary = Path(directory) / "agent"
            binary.write_bytes(b"agent")
            with patch("vts_profile.device._run", side_effect=[
                subprocess.CompletedProcess([], 1, "", "転送失敗"),
                subprocess.CompletedProcess([], 1, "", "削除失敗"),
            ]):
                with self.assertRaises(AgentCleanupError) as caught:
                    _prepare_agent(adb="adb", serial=None, agent_binary=binary,
                                   remote_agent="/vendor/bin/agent")
        self.assertEqual(str(caught.exception.operation_error), "転送失敗")
        self.assertEqual(str(caught.exception.cleanup_error), "削除失敗")

    def test_cleanup_failure_preserves_profile_after_successful_resolution(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            profile_path = Path(directory) / "profile.json"
            profile_path.write_bytes(b"original")
            with (
                patch("vts_profile.model.load_profile", return_value={}),
                patch("vts_profile.device.validate_profile"),
                patch("vts_profile.device._requested_service_id", return_value=None),
                patch("vts_profile.device._prepare_agent", return_value=("agent", True)),
                patch("vts_profile.device._frequencies", return_value=[100]),
                patch("vts_profile.device._resolve_frequency", return_value={}),
                patch("vts_profile.device._apply", return_value={"resolved": True}),
                patch("vts_profile.device._cleanup_agent", side_effect=ProfileError("削除失敗")),
            ):
                with self.assertRaisesRegex(ProfileError, "削除失敗"):
                    resolve_device(profile_path)
            self.assertEqual(profile_path.read_bytes(), b"original")


if __name__ == "__main__":
    unittest.main()
