from __future__ import annotations

import re
import unittest
from pathlib import Path


TUNER_HAL2 = Path(__file__).resolve().parent.parent
ANDROID_BP = TUNER_HAL2 / "Android.bp"
LIFECYCLE_RC = TUNER_HAL2 / "init" / "mediatuner-trm-lifecycle.rc"


class MediatunerTrmLifecycleIntegrationTest(unittest.TestCase):
    def test_init_rc_installed_by_existing_hal_binary(self) -> None:
        bp = ANDROID_BP.read_text(encoding="utf-8")
        match = re.search(
            r'rust_binary\s*\{\s*name:\s*"android\.hardware\.tv\.tuner-service\.maleicacid2".*?'
            r'init_rc:\s*\[([^]]+)\]',
            bp,
            flags=re.DOTALL,
        )
        self.assertIsNotNone(match)
        self.assertEqual(
            match.group(1).count('"init/mediatuner-trm-lifecycle.rc"'),
            1,
        )

    def test_framework_stop_then_gated_start_recovers_trm(self) -> None:
        rc = LIFECYCLE_RC.read_text(encoding="utf-8")
        actions: dict[str, list[str]] = {}
        active = None
        for line in rc.splitlines():
            line = line.partition("#")[0].rstrip()
            if not line:
                continue
            if line.startswith("on "):
                active = line.removeprefix("on ")
                self.assertNotIn(active, actions)
                actions[active] = []
                continue
            self.assertIsNotNone(active)
            self.assertTrue(line.startswith("    "))
            actions[active].append(line.strip())

        self.assertEqual(actions["property:init.svc.zygote=stopped"], ["stop media.tuner"])
        self.assertEqual(actions["property:init.svc.zygote=restarting"], ["stop media.tuner"])
        self.assertEqual(
            actions["property:init.svc.zygote=running && property:tuner.server.enable=true"],
            ["start media.tuner"],
        )
        # ro.tuner.lazyhal構成ではtuner.server.enableによる強制起動を行わない。
        self.assertNotIn("property:init.svc.zygote=running", actions)
        self.assertFalse(any("start vendor.maleicacid-tuner2" in v for vs in actions.values() for v in vs))


if __name__ == "__main__":
    unittest.main()
