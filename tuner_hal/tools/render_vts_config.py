#!/usr/bin/env python3
from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path
from xml.sax.saxutils import escape

STREAM_ID_TYPE_XML = {
    'TS_ID': '0',
    'ABSOLUTE_STREAM_ID': '0',
    'RELATIVE_STREAM_NUMBER': '1',
}

FRONTEND_ID_RE = re.compile(r'^(FE_DEFAULT|FE_[A-Z]+_[0-9]+)$')
FILTER_ID_RE = re.compile(r'^(FILTER_AUDIO_DEFAULT|FILTER_VIDEO_DEFAULT|FILTER_[A-Z]+_[A-Z]+_[0-9]+)$')
DVR_RECORD_ID_RE = re.compile(r'^DVR_RECORD_[0-9]+$')

DVR_BUFFER_SIZE = 4_194_304
DVR_LOW_THRESHOLD = DVR_BUFFER_SIZE // 4
DVR_HIGH_THRESHOLD = (DVR_BUFFER_SIZE * 3) // 4


def parse_simple_yaml(path: Path) -> dict:
    profile: dict[str, object] = {}
    current: str | None = None
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
            if current is None:
                raise ValueError(f'{path}: 親項目のない行です: {line}')
            key, value = [part.strip() for part in line.split(':', 1)]
            section = profile[current]
            if not isinstance(section, dict):
                raise ValueError(f'{path}: {current} は入れ子項目ではありません')
            section[key] = value.strip('"')
    profile['_source_path'] = str(path)
    return profile


def bool_xml(value: object) -> str:
    if isinstance(value, bool):
        return 'true' if value else 'false'
    text = str(value).strip().lower()
    if text in ('true', '1', 'yes'):
        return 'true'
    if text in ('false', '0', 'no'):
        return 'false'
    raise ValueError(f'真偽値ではありません: {value}')


def require_section(profile: dict, key: str) -> dict:
    value = profile.get(key)
    if not isinstance(value, dict):
        raise ValueError(f'{profile_name_for_error(profile)}: {key} がありません')
    return value


def require_key(section: dict, key: str, context: str) -> str:
    if key not in section or str(section[key]) == '':
        raise ValueError(f'{context}: {key} がありません')
    return str(section[key])


def profile_name(profile: dict) -> str:
    value = profile.get('name')
    if not value:
        raise ValueError(f'{profile.get("_source_path", "profile")}: name がありません')
    return str(value)


def profile_name_for_error(profile: dict) -> str:
    return str(profile.get('name') or profile.get('_source_path') or 'profile')


def validate_id(pattern: re.Pattern[str], value: str, kind: str, profile: dict) -> str:
    if not pattern.match(value):
        raise ValueError(f'{profile_name_for_error(profile)}: {kind} id が AOSP VTS XML の形式に合いません: {value}')
    return value


def expected_frontend_id(fe_type: str) -> str:
    if fe_type == 'ISDBS':
        return 'FE_ISDBS_0'
    if fe_type == 'ISDBT':
        return 'FE_ISDBT_0'
    raise ValueError(f'対象外 frontend type {fe_type}')


def normalized_ids(profile: dict) -> dict[str, str]:
    fe = require_section(profile, 'frontend')
    fe_type = require_key(fe, 'type', f'{profile_name_for_error(profile)}.frontend')
    ids = {
        'frontend': expected_frontend_id(fe_type),
        'audio_filter': 'FILTER_TS_AUDIO_0',
        'video_filter': 'FILTER_TS_VIDEO_0',
        'record_filter': 'FILTER_TS_RECORD_0',
        'record_dvr': 'DVR_RECORD_0',
    }
    validate_id(FRONTEND_ID_RE, ids['frontend'], 'frontend', profile)
    validate_id(FILTER_ID_RE, ids['audio_filter'], 'audio filter', profile)
    validate_id(FILTER_ID_RE, ids['video_filter'], 'video filter', profile)
    validate_id(FILTER_ID_RE, ids['record_filter'], 'record filter', profile)
    validate_id(DVR_RECORD_ID_RE, ids['record_dvr'], 'record DVR', profile)
    return ids


def ensure_profile_id_if_present(profile: dict) -> None:
    ids = normalized_ids(profile)
    fe = require_section(profile, 'frontend')
    if fe.get('id') and fe['id'] != ids['frontend']:
        raise ValueError(f'{profile_name_for_error(profile)}: frontend.id は {ids["frontend"]} にしてください')
    live = profile.get('live')
    if isinstance(live, dict):
        expected = {
            'audio_filter_id': ids['audio_filter'],
            'video_filter_id': ids['video_filter'],
        }
        for key, value in expected.items():
            if live.get(key) and live[key] != value:
                raise ValueError(f'{profile_name_for_error(profile)}: live.{key} は {value} にしてください')
    rec = profile.get('record')
    if isinstance(rec, dict):
        expected = {
            'filter_id': ids['record_filter'],
            'dvr_id': ids['record_dvr'],
        }
        for key, value in expected.items():
            if rec.get(key) and rec[key] != value:
                raise ValueError(f'{profile_name_for_error(profile)}: record.{key} は {value} にしてください')
        if 'playback_dvr_id' in rec:
            raise ValueError(f'{profile_name_for_error(profile)}: r51 では playback_dvr_id を指定しません')


def require_explicit_tune_point(fe: dict, profile: dict) -> None:
    frequency = require_key(fe, 'frequency', f'{profile_name_for_error(profile)}.frontend')
    end = fe.get('end_frequency')
    if end is not None and str(end) != frequency:
        raise ValueError(
            f"{profile_name_for_error(profile)}: r51 VTS用プロファイルは明示選局点でなければなりません: "
            f"end_frequency={end}"
        )


def support_blind_scan_xml(profile: dict) -> str:
    scan = require_section(profile, 'scan')
    if 'support_blind_scan' not in scan:
        raise ValueError(f'{profile_name_for_error(profile)}.scan: support_blind_scan がありません')
    value = bool_xml(scan['support_blind_scan'])
    if value != 'false':
        raise ValueError(f'{profile_name_for_error(profile)}: r51 では support_blind_scan は false 固定です')
    return 'false'


def frontend_xml(profile: dict) -> str:
    ensure_profile_id_if_present(profile)
    ids = normalized_ids(profile)
    fe = require_section(profile, 'frontend')
    fe_type = require_key(fe, 'type', f'{profile_name_for_error(profile)}.frontend')
    require_explicit_tune_point(fe, profile)
    common = (
        f'id="{ids["frontend"]}" type="{fe_type}" '
        f'isSoftwareFrontend="{bool_xml(require_key(fe, "is_software_frontend", f"{profile_name_for_error(profile)}.frontend"))}" '
        f'frequency="{escape(require_key(fe, "frequency", f"{profile_name_for_error(profile)}.frontend"))}" '
        f'supportBlindScan="{support_blind_scan_xml(profile)}"'
    )
    if fe_type == 'ISDBT':
        return (
            f'      <frontend {common}><isdbtFrontendSettings serviceAreaId="0" inversion="0" '
            f'bandwidth="8" mode="1" guardInterval="1" partialReceptionFlag="0">'
            f'<FrontendIsdbtLayerSettings modulation="1" coderate="1" timeInterleave="1" numOfSegment="0"/>'
            f'</isdbtFrontendSettings></frontend>'
        )
    if fe_type == 'ISDBS':
        context = f'{profile_name_for_error(profile)}.frontend'
        stream_type_name = require_key(fe, 'stream_id_type', context)
        stream_type = STREAM_ID_TYPE_XML.get(stream_type_name)
        if stream_type is None:
            raise ValueError(f'{context}: 未知の stream_id_type です: {stream_type_name}')
        attrs = {
            'streamId': require_key(fe, 'stream_id', context),
            'streamIdType': stream_type,
            'modulation': require_key(fe, 'modulation', context),
            'coderate': require_key(fe, 'coderate', context),
            'symbolRate': require_key(fe, 'symbol_rate', context),
            'rolloff': require_key(fe, 'rolloff', context),
        }
        settings_attrs = ' '.join(f'{key}="{escape(str(value))}"' for key, value in attrs.items())
        return f'      <frontend {common}><isdbsFrontendSettings {settings_attrs}/></frontend>'
    raise ValueError(f'対象外 frontend type {fe_type}')


def monitor_event_types(profile: dict, section: dict, key: str) -> str:
    value = str(section.get(key, '0'))
    if value != '0':
        raise ValueError(f'{profile_name_for_error(profile)}: {key} は r51 では 0 固定です')
    return '0'


def filters_xml(profile: dict) -> list[str]:
    ids = normalized_ids(profile)
    live = require_section(profile, 'live')
    rec = profile.get('record', {})
    if not isinstance(rec, dict):
        raise ValueError(f'{profile_name_for_error(profile)}: record が不正です')
    audio_pid = require_key(live, 'audio_pid', f'{profile_name_for_error(profile)}.live')
    video_pid = require_key(live, 'video_pid', f'{profile_name_for_error(profile)}.live')
    values = [
        f'      <filter id="{ids["audio_filter"]}" mainType="TS" subType="AUDIO" bufferSize="16777216" pid="{escape(audio_pid)}" useFMQ="false" monitorEventTypes="{monitor_event_types(profile, live, "audio_monitor_event_types")}"><avFilterSettings isPassthrough="false" isSecureMemory="false"><audioStreamType>2</audioStreamType></avFilterSettings></filter>',
        f'      <filter id="{ids["video_filter"]}" mainType="TS" subType="VIDEO" bufferSize="16777216" pid="{escape(video_pid)}" useFMQ="false" monitorEventTypes="{monitor_event_types(profile, live, "video_monitor_event_types")}"><avFilterSettings isPassthrough="false" isSecureMemory="false"><videoStreamType>2</videoStreamType></avFilterSettings></filter>',
    ]
    if rec:
        if 'playback_dvr_id' in rec:
            raise ValueError(f'{profile_name_for_error(profile)}: r51 では playback DVR を出力しません')
        values.append(
            f'      <filter id="{ids["record_filter"]}" mainType="TS" subType="RECORD" bufferSize="16777216" pid="{escape(require_key(rec, "pid", f"{profile_name_for_error(profile)}.record"))}" useFMQ="false"><recordFilterSettings tsIndexMask="1" scIndexType="NONE"/></filter>'
        )
    return values


def dvr_xml(profile: dict) -> list[str]:
    ids = normalized_ids(profile)
    rec = profile.get('record', {})
    if not rec:
        return []
    if not isinstance(rec, dict):
        raise ValueError(f'{profile_name_for_error(profile)}: record が不正です')
    if 'playback_dvr_id' in rec:
        raise ValueError(f'{profile_name_for_error(profile)}: r51 では playback DVR を出力しません')
    common = (
        f'bufferSize="{DVR_BUFFER_SIZE}" statusMask="15" '
        f'lowThreshold="{DVR_LOW_THRESHOLD}" highThreshold="{DVR_HIGH_THRESHOLD}" '
        f'dataFormat="TS" packetSize="188"'
    )
    return [f'      <dvr id="{ids["record_dvr"]}" type="RECORD" {common}/>']


def data_flow_xml(profile: dict) -> list[str]:
    ids = normalized_ids(profile)
    rec = profile.get('record', {})
    values = [
        f'    <clearLiveBroadcast frontendConnection="{ids["frontend"]}" audioFilterConnection="{ids["audio_filter"]}" videoFilterConnection="{ids["video_filter"]}"/>',
    ]
    if rec:
        values.append(
            f'    <dvrRecord hasFrontendConnection="true" frontendConnection="{ids["frontend"]}" recordFilterConnection="{ids["record_filter"]}" dvrRecordConnection="{ids["record_dvr"]}"/>'
        )
    return values


def select_profile(profiles: list[dict], selected_name: str | None) -> dict:
    seen: set[str] = set()
    by_name: dict[str, dict] = {}
    for profile in profiles:
        name = profile_name(profile)
        if name in seen:
            raise ValueError(f'name が重複しています: {name}')
        seen.add(name)
        by_name[name] = profile
    if selected_name:
        if selected_name not in by_name:
            raise ValueError(f'--select の対象が見つかりません: {selected_name}')
        return by_name[selected_name]
    if len(profiles) == 1:
        return profiles[0]
    raise ValueError('複数 profile 入力では --select <name> が必要です')


def render(profiles: list[dict], selected_name: str | None = None) -> str:
    profile = select_profile(profiles, selected_name)
    if 'descramble' in profile:
        raise ValueError('VTS用プロファイルでは descramble を宣言しません')
    frontends = [frontend_xml(profile)]
    filters = filters_xml(profile)
    dvrs = dvr_xml(profile)
    flows = data_flow_xml(profile)
    dvr_block = ''
    if dvrs:
        dvr_block = '\n    <dvrs>\n' + '\n'.join(dvrs) + '\n    </dvrs>'
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


def expand_sources(args: list[str]) -> list[Path]:
    sources: list[Path] = []
    for arg in args:
        path = Path(arg)
        if path.is_dir():
            sources.extend(sorted(path.glob('*.yaml')))
        else:
            sources.append(path)
    return sources


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description='Render one AOSP Tuner VTS XML from r51 profile YAML files.')
    parser.add_argument('--select', help='出力対象にする YAML 直下 name')
    parser.add_argument('inputs', nargs='+', help='profile.yaml または profile_dir。最後の引数は出力 XML。')
    ns = parser.parse_args(argv)
    if len(ns.inputs) < 2:
        parser.error('profile.yaml|profile_dir と output.xml が必要です')
    sources = expand_sources(ns.inputs[:-1])
    dst = Path(ns.inputs[-1])
    profiles = [parse_simple_yaml(src) for src in sources]
    dst.write_text(render(profiles, ns.select), encoding='utf-8')
    return 0


if __name__ == '__main__':
    sys.exit(main())
