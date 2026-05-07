#!/usr/bin/env python3
from pathlib import Path
import sys

STREAM_ID_TYPE_XML = {
    'TS_ID': '0',
    'ABSOLUTE_STREAM_ID': '0',
    'RELATIVE_STREAM_NUMBER': '1',
}


def parse_simple_yaml(path: Path):
    profile = {}
    current = None
    for raw in path.read_text(encoding='utf-8').splitlines():
        line = raw.rstrip()
        if not line or line.lstrip().startswith('#'):
            continue
        if not raw.startswith(' '):
            key, value = [part.strip() for part in line.split(':', 1)]
            if value:
                profile[key] = value.strip('"')
                current = None
            else:
                current = key
                profile[current] = {}
        else:
            key, value = [part.strip() for part in line.split(':', 1)]
            profile[current][key] = value.strip('"')
    return profile


def bool_xml(value):
    return str(value).lower()


def is_cs110(fe):
    return fe.get('satellite_band', '').upper() == 'CS110' or 'CS110' in fe.get('id', '').upper()


def require_explicit_tune_point(fe):
    end = fe.get('end_frequency')
    if end is not None and end != fe['frequency']:
        raise ValueError(
            f"r51 VTS/lab profile must be an explicit tune point: "
            f"{fe.get('id')} has end_frequency={end}"
        )


def frontend_xml(fe):
    fe_type = fe['type']
    require_explicit_tune_point(fe)
    if fe_type == 'ISDBT':
        return f'''      <frontend id="{fe['id']}" type="ISDBT" isSoftwareFrontend="{bool_xml(fe['is_software_frontend'])}" frequency="{fe['frequency']}"><isdbtFrontendSettings serviceAreaId="0" inversion="0" bandwidth="8" mode="1" guardInterval="1" partialReceptionFlag="0"><FrontendIsdbtLayerSettings modulation="1" coderate="1" timeInterleave="1" numOfSegment="0"/></isdbtFrontendSettings></frontend>'''
    if fe_type == 'ISDBS':
        symbol_rate = fe.get('symbol_rate', '0')
        if is_cs110(fe):
            stream_attrs = ''
        else:
            if 'stream_id' not in fe:
                raise ValueError(f"BS profile {fe.get('id')} must declare stream_id")
            stream_type = STREAM_ID_TYPE_XML.get(fe.get('stream_id_type', 'TS_ID'))
            if stream_type is None:
                raise ValueError(f"unknown stream_id_type {fe.get('stream_id_type')}")
            stream_attrs = f' streamId="{fe["stream_id"]}" streamIdType="{stream_type}"'
        return f'''      <frontend id="{fe['id']}" type="ISDBS" isSoftwareFrontend="{bool_xml(fe['is_software_frontend'])}" frequency="{fe['frequency']}"><isdbsFrontendSettings{stream_attrs} modulation="1" coderate="1" symbolRate="{symbol_rate}"/></frontend>'''
    raise ValueError(f"対象外 frontend type {fe_type}")


def filters_xml(profile):
    live = profile.get('live', {})
    rec = profile.get('record', {})
    audio_id = live.get('audio_filter_id', 'FILTER_AUDIO_DEFAULT')
    video_id = live.get('video_filter_id', 'FILTER_VIDEO_DEFAULT')
    audio_pid = live.get('audio_pid', '273')
    video_pid = live.get('video_pid', '272')
    values = [
        f'''      <filter id="{audio_id}" mainType="TS" subType="AUDIO" bufferSize="16777216" pid="{audio_pid}" useFMQ="false" monitorEventTypes="0"><avFilterSettings isPassthrough="false" isSecureMemory="false"><audioStreamType>2</audioStreamType></avFilterSettings></filter>''',
        f'''      <filter id="{video_id}" mainType="TS" subType="VIDEO" bufferSize="16777216" pid="{video_pid}" useFMQ="false" monitorEventTypes="0"><avFilterSettings isPassthrough="false" isSecureMemory="false"><videoStreamType>2</videoStreamType></avFilterSettings></filter>''',
    ]
    if rec:
        values.append(f'''      <filter id="{rec['filter_id']}" mainType="TS" subType="RECORD" bufferSize="16777216" pid="{rec['pid']}" useFMQ="false"><recordFilterSettings tsIndexMask="1" scIndexType="NONE"/></filter>''')
    return values


DVR_BUFFER_SIZE = 4_194_304
DVR_LOW_THRESHOLD = DVR_BUFFER_SIZE // 4
DVR_HIGH_THRESHOLD = (DVR_BUFFER_SIZE * 3) // 4


def playback_dvr_id_for_record(record_dvr_id: str) -> str:
    if record_dvr_id.startswith('DVR_RECORD_'):
        return 'DVR_PLAYBACK_' + record_dvr_id[len('DVR_RECORD_'):]
    return record_dvr_id + '_PLAYBACK'


def dvr_xml(profile):
    rec = profile.get('record', {})
    if not rec:
        return []
    record_id = rec["dvr_id"]
    playback_id = rec.get("playback_dvr_id", playback_dvr_id_for_record(record_id))
    common = (
        f'bufferSize="{DVR_BUFFER_SIZE}" statusMask="15" '
        f'lowThreshold="{DVR_LOW_THRESHOLD}" highThreshold="{DVR_HIGH_THRESHOLD}" '
        f'dataFormat="TS" packetSize="188"'
    )
    return [
        f'<dvr id="{record_id}" type="RECORD" {common}/>',
        f'<dvr id="{playback_id}" type="PLAYBACK" {common}/>',
    ]


def data_flow_xml(profile):
    fe = profile['frontend']
    live = profile.get('live', {})
    rec = profile.get('record', {})
    audio_id = live.get('audio_filter_id', 'FILTER_AUDIO_DEFAULT')
    video_id = live.get('video_filter_id', 'FILTER_VIDEO_DEFAULT')
    values = [
        f'''    <clearLiveBroadcast frontendConnection="{fe['id']}" audioFilterConnection="{audio_id}" videoFilterConnection="{video_id}"/>''',
    ]
    if rec:
        playback_id = rec.get('playback_dvr_id', playback_dvr_id_for_record(rec['dvr_id']))
        values.append(f'''    <dvrRecord hasFrontendConnection="true" frontendConnection="{fe['id']}" recordFilterConnection="{rec['filter_id']}" dvrRecordConnection="{rec['dvr_id']}"/>''')
        values.append(f'''    <dvrPlayback dvrConnection="{playback_id}" audioFilterConnection="{audio_id}" videoFilterConnection="{video_id}"/>''')
    return values


def render(profiles):
    frontends = []
    filters = []
    dvrs = []
    flows = []
    for profile in profiles:
        if 'descramble' in profile:
            raise ValueError('VTS lab profile では production descrambling を claim しない')
        frontends.append(frontend_xml(profile['frontend']))
        filters.extend(filters_xml(profile))
        dvrs.extend(dvr_xml(profile))
        flows.extend(data_flow_xml(profile))
    dvr_block = ''
    if dvrs:
        dvr_block = '\n    <dvrs>' + ''.join(dvrs) + '</dvrs>'
    return f'''<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<TunerConfiguration version="1.0" xmlns:xi="http://www.w3.org/2001/XInclude">
  <hardwareConfiguration>
    <frontends>
{chr(10).join(frontends)}
    </frontends>
    <filters>
{chr(10).join(filters)}
    </filters>{dvr_block}
  </hardwareConfiguration>
  <dataFlowConfiguration>
{chr(10).join(flows)}
  </dataFlowConfiguration>
</TunerConfiguration>
'''


def expand_sources(args):
    sources = []
    for arg in args:
        path = Path(arg)
        if path.is_dir():
            sources.extend(sorted(path.glob('*.yaml')))
        else:
            sources.append(path)
    return sources


def main():
    if len(sys.argv) < 3:
        raise SystemExit('usage: render_vts_config.py <profile.yaml|profile_dir>... <output.xml>')
    sources = expand_sources(sys.argv[1:-1])
    dst = Path(sys.argv[-1])
    profiles = [parse_simple_yaml(src) for src in sources]
    profiles.sort(key=lambda p: p['frontend']['id'])
    dst.write_text(render(profiles), encoding='utf-8')


if __name__ == '__main__':
    main()
