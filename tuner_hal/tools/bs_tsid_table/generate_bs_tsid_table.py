#!/usr/bin/env python3
import re
from pathlib import Path

src = Path(__file__).with_name('dvbv5_channels_isdbs.conf')
current = None
entries = []
for raw in src.read_text(encoding='utf-8').splitlines():
    line = raw.strip()
    if not line or line.startswith('#'):
        continue
    m = re.match(r'\[(BS\d+)_([0-9]+)\]', line)
    if m:
        current = {'name': m.group(1), 'relative': int(m.group(2))}
        continue
    if current is None:
        continue
    if line.startswith('FREQUENCY'):
        current['freq_khz'] = int(line.split('=', 1)[1])
    elif line.startswith('STREAM_ID'):
        current['tsid'] = int(line.split('=', 1)[1])
        entries.append(current)
        current = None

print('pub const JAPAN_BS_ISDBS_TSID_TABLE: &[JapanIsdbsTsidEntry] = &[')
for e in entries:
    print(f'    JapanIsdbsTsidEntry {{ band: JapanIsdbsBand::Bs, if_frequency_hz: {e["freq_khz"] * 1000}, relative_stream_number: {e["relative"]}, tsid: 0x{e["tsid"]:04x} }},')
print('];')
