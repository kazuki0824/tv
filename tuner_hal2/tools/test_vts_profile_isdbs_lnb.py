from __future__ import annotations

import unittest

from vts_profile.model import ProfileError, validate_profile
from vts_profile.render import render_xml


def isdbs_profile() -> dict:
    return {
        "schema_version": 1,
        "target": {"hal": "tuner_hal2", "product": "default", "backend": "px4"},
        "vts": {"contract": "android14-aidl-v1", "source_ref": "aosp-commit", "variant": ""},
        "frontend": {
            "type": "ISDBS",
            "is_software_frontend": False,
            "frequency_hz": 1049480000,
            "stream_id": 101,
            "stream_id_type": 0,
            "symbol_rate": 28860000,
            "modulation": 0,
            "coderate": 0,
            "rolloff": 0,
        },
        "flows": {
            "scan": True,
            "record": {"enabled": True, "pid": 272},
            "clear_live": {
                "enabled": True,
                "audio_pid": 273,
                "video_pid": 272,
                "audio_stream_type": 16,
                "video_stream_type": 5,
                "pcr_pid": 272,
                "section_pid": 256,
            },
            "playback": {
                "enabled": True,
                "input_file_path": "/data/local/tmp/segment000000.ts",
                "audio_pid": 257,
                "video_pid": 256,
                "section_pid": 257,
                "audio_stream_type": 2,
                "video_stream_type": 2,
            },
        },
        "lnb": {"voltage": "NONE", "tone": "NONE", "position": "UNDEFINED"},
        "queues": {
            "record_filter_bytes": 1048576,
            "record_dvr_bytes": 4194304,
            "audio_filter_bytes": 1048576,
            "video_filter_bytes": 1048576,
            "pcr_filter_bytes": 1048576,
            "section_filter_bytes": 1048576,
            "playback_dvr_bytes": 4194304,
        },
    }


def record_fmq_probe_with_stale_scan() -> dict:
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


class IsdbsLnbVtsProfileTest(unittest.TestCase):
    def test_isdbs_canonical_requires_lnb(self) -> None:
        profile = isdbs_profile()
        profile.pop("lnb")
        with self.assertRaisesRegex(ProfileError, "lnb must be an object"):
            validate_profile(profile, require_resolved=True)

    def test_isdbt_rejects_lnb_configuration(self) -> None:
        profile = isdbs_profile()
        profile["frontend"] = {
            "type": "ISDBT",
            "is_software_frontend": False,
            "frequency_hz": 557142857,
        }
        with self.assertRaisesRegex(ProfileError, "ISDBT canonical profile must not contain"):
            validate_profile(profile, require_resolved=True)

    def test_isdbs_renderer_connects_lnb_live_and_record(self) -> None:
        xml = render_xml(isdbs_profile())
        self.assertIn('supportBlindScan="false"', xml)
        self.assertIn('<lnb id="LNB_0" voltage="NONE" tone="NONE" position="UNDEFINED"/>', xml)
        self.assertIn('<lnbLive frontendConnection="FE_ISDBS_0"', xml)
        self.assertIn('audioFilterConnection="FILTER_TS_AUDIO_LIVE_0"', xml)
        self.assertIn('videoFilterConnection="FILTER_TS_VIDEO_LIVE_0"', xml)
        self.assertIn('lnbConnection="LNB_0"', xml)
        self.assertIn('<lnbRecord frontendConnection="FE_ISDBS_0"', xml)
        self.assertIn('recordFilterConnection="FILTER_TS_RECORD_0"', xml)
        self.assertIn('dvrRecordConnection="DVR_RECORD_0"', xml)

    def test_record_fmq_probe_renderer_suppresses_stale_scan(self) -> None:
        xml = render_xml(record_fmq_probe_with_stale_scan())
        self.assertIn('supportBlindScan="false"', xml)
        self.assertNotIn("<scan ", xml)
        self.assertNotIn("<lnbs>", xml)
        self.assertIn('subType="RECORD"', xml)
        self.assertIn('useFMQ="true"', xml)


if __name__ == "__main__":
    unittest.main()