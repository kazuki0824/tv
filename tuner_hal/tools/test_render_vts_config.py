#!/usr/bin/env python3
from __future__ import annotations

import importlib.util
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name('render_vts_config.py')
spec = importlib.util.spec_from_file_location('render_vts_config', SCRIPT)
render_vts_config = importlib.util.module_from_spec(spec)
assert spec.loader is not None
spec.loader.exec_module(render_vts_config)


def base_isdbs(name: str = 'bs') -> str:
    return f'''name: {name}
frontend:
  id: FE_ISDBS_0
  type: ISDBS
  is_software_frontend: false
  frequency: 1049480000
  stream_id_type: TS_ID
  stream_id: 16400
  symbol_rate: 0
  modulation: 1
  coderate: 1
  rolloff: 0
live:
  audio_filter_id: FILTER_TS_AUDIO_0
  audio_pid: 273
  video_filter_id: FILTER_TS_VIDEO_0
  video_pid: 272
record:
  filter_id: FILTER_TS_RECORD_0
  pid: 272
  dvr_id: DVR_RECORD_0
scan:
  support_blind_scan: false
'''


def base_isdbt(name: str = 'uhf') -> str:
    return f'''name: {name}
frontend:
  id: FE_ISDBT_0
  type: ISDBT
  is_software_frontend: false
  frequency: 557142857
live:
  audio_filter_id: FILTER_TS_AUDIO_0
  audio_pid: 273
  video_filter_id: FILTER_TS_VIDEO_0
  video_pid: 272
record:
  filter_id: FILTER_TS_RECORD_0
  pid: 272
  dvr_id: DVR_RECORD_0
scan:
  support_blind_scan: false
'''


def parse_text(text: str) -> dict:
    with tempfile.TemporaryDirectory() as tmp:
        path = Path(tmp) / 'profile.yaml'
        path.write_text(text, encoding='utf-8')
        return render_vts_config.parse_simple_yaml(path)


class RenderVtsConfigTest(unittest.TestCase):
    def render_one(self, text: str, selected: str | None = None) -> str:
        return render_vts_config.render([parse_text(text)], selected)

    def test_single_yaml_without_select_renders(self):
        xml = self.render_one(base_isdbs())
        self.assertIn('id="FE_ISDBS_0"', xml)
        self.assertIn('supportBlindScan="false"', xml)

    def test_multiple_yaml_requires_select(self):
        profiles = [parse_text(base_isdbs('bs')), parse_text(base_isdbt('uhf'))]
        with self.assertRaises(ValueError):
            render_vts_config.render(profiles)

    def test_multiple_yaml_with_select_outputs_only_selected_profile(self):
        profiles = [parse_text(base_isdbs('bs')), parse_text(base_isdbt('uhf'))]
        xml = render_vts_config.render(profiles, 'uhf')
        self.assertIn('id="FE_ISDBT_0"', xml)
        self.assertNotIn('FE_ISDBS_0', xml)

    def test_unknown_select_fails(self):
        with self.assertRaises(ValueError):
            render_vts_config.render([parse_text(base_isdbs('bs'))], 'missing')

    def test_duplicate_name_fails(self):
        profiles = [parse_text(base_isdbs('same')), parse_text(base_isdbt('same'))]
        with self.assertRaises(ValueError):
            render_vts_config.render(profiles, 'same')

    def test_support_blind_scan_missing_fails(self):
        text = base_isdbs().replace('  support_blind_scan: false\n', '')
        with self.assertRaises(ValueError):
            self.render_one(text)

    def test_support_blind_scan_true_fails(self):
        with self.assertRaises(ValueError):
            self.render_one(base_isdbs().replace('support_blind_scan: false', 'support_blind_scan: true'))

    def test_invalid_frontend_id_fails(self):
        with self.assertRaises(ValueError):
            self.render_one(base_isdbs().replace('id: FE_ISDBS_0', 'id: FE_ISDBS_BS'))

    def test_invalid_filter_id_fails(self):
        with self.assertRaises(ValueError):
            self.render_one(base_isdbs().replace('audio_filter_id: FILTER_TS_AUDIO_0', 'audio_filter_id: FILTER_AUDIO_BS'))

    def test_invalid_dvr_id_fails(self):
        with self.assertRaises(ValueError):
            self.render_one(base_isdbs().replace('dvr_id: DVR_RECORD_0', 'dvr_id: DVR_RECORD_BS'))

    def test_isdbs_required_attribute_missing_fails(self):
        with self.assertRaises(ValueError):
            self.render_one(base_isdbs().replace('  rolloff: 0\n', ''))

    def test_data_flow_has_one_live_and_one_record(self):
        xml = self.render_one(base_isdbs())
        self.assertEqual(xml.count('<clearLiveBroadcast '), 1)
        self.assertEqual(xml.count('<dvrRecord '), 1)

    def test_dvr_playback_is_not_output(self):
        xml = self.render_one(base_isdbs())
        self.assertNotIn('dvrPlayback', xml)
        self.assertNotIn('type="PLAYBACK"', xml)

    def test_playback_input_is_rejected(self):
        text = base_isdbs().replace('  dvr_id: DVR_RECORD_0\n', '  dvr_id: DVR_RECORD_0\n  playback_dvr_id: DVR_PLAYBACK_0\n')
        with self.assertRaises(ValueError):
            self.render_one(text)

    def test_nonzero_monitor_event_types_fails(self):
        text = base_isdbs().replace('  video_pid: 272\n', '  video_pid: 272\n  video_monitor_event_types: 1\n')
        with self.assertRaises(ValueError):
            self.render_one(text)


if __name__ == '__main__':
    unittest.main()
