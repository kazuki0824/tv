from __future__ import annotations
from copy import deepcopy
from typing import Any
from xml.sax.saxutils import escape
from .model import FRONTEND_ID, RECORD_FILTER_FMQ_PROBE_VARIANT, validate_profile

SECTION_LENGTH_FIELD_BITS = 12
SECTION_DELAY_HINT_MS = 100


def _frontend_xml(profile: dict[str, Any]) -> str:
    fe = profile["frontend"]
    fe_type = fe["type"]
    attrs = [
        f'id="{FRONTEND_ID[fe_type]}"',
        f'type="{fe_type}"',
        'isSoftwareFrontend="false"',
        'supportBlindScan="false"',
        f'frequency="{int(fe["frequency_hz"])}"',
    ]
    if fe_type == "ISDBT":
        return f'      <frontend {" ".join(attrs)}/>'
    settings = {
        "streamId": fe["stream_id"], "streamIdType": fe["stream_id_type"],
        "modulation": fe["modulation"], "coderate": fe["coderate"],
        "symbolRate": fe["symbol_rate"], "rolloff": fe["rolloff"],
    }
    settings_text = " ".join(f'{key}="{escape(str(value))}"' for key, value in settings.items())
    return f'      <frontend {" ".join(attrs)}><isdbsFrontendSettings {settings_text}/></frontend>'


def _dvr_xml(dvr_id: str, dvr_type: str, size: int, *, input_file_path: str | None = None) -> str:
    input_attr = "" if input_file_path is None else f' inputFilePath="{escape(input_file_path)}"'
    return (
        f'      <dvr id="{dvr_id}" type="{dvr_type}" bufferSize="{size}" statusMask="15" '
        f'lowThreshold="{size // 4}" highThreshold="{size * 3 // 4}" dataFormat="TS" '
        f'packetSize="188"{input_attr}/>'
    )


def _av_filter_xml(
    filter_id: str,
    subtype: str,
    buffer_size: int,
    pid: int,
    stream_type: int,
) -> str:
    stream_tag = "audioStreamType" if subtype == "AUDIO" else "videoStreamType"
    return (
        f'      <filter id="{filter_id}" mainType="TS" subType="{subtype}" '
        f'bufferSize="{buffer_size}" pid="{pid}" useFMQ="false">'
        '<avFilterSettings isPassthrough="false" isSecureMemory="false">'
        f'<{stream_tag}>{stream_type}</{stream_tag}></avFilterSettings></filter>'
    )


def _section_filter_xml(
    filter_id: str,
    buffer_size: int,
    pid: int,
    *,
    delay_hint_ms: int | None = None,
) -> str:
    delay_attr = "" if delay_hint_ms is None else f' timeDelayInMs="{delay_hint_ms}"'
    return (
        f'      <filter id="{filter_id}" mainType="TS" subType="SECTION" '
        f'bufferSize="{buffer_size}" pid="{pid}" useFMQ="true"{delay_attr}>'
        f'<sectionFilterSettings isCheckCrc="false" isRepeat="true" isRaw="false" '
        f'bitWidthOfLengthField="{SECTION_LENGTH_FIELD_BITS}"/></filter>'
    )


def render_xml(profile: dict[str, Any]) -> str:
    rendered_profile = deepcopy(profile)
    probe = rendered_profile["vts"].get("variant", "") == RECORD_FILTER_FMQ_PROBE_VARIANT
    if probe:
        # The descriptor probe is intentionally RECORD-only even if a caller copied a
        # canonical profile that still had scan enabled.
        rendered_profile["flows"]["scan"] = False
    validate_profile(rendered_profile, require_resolved=True)
    flows = rendered_profile["flows"]
    queues = rendered_profile["queues"]
    hardware = ["    <frontends>", _frontend_xml(rendered_profile), "    </frontends>"]
    filters: list[str] = []
    dvrs: list[str] = []
    data_flows: list[str] = []
    frontend_id = FRONTEND_ID[rendered_profile["frontend"]["type"]]

    if flows["scan"]:
        data_flows.append(f'    <scan frontendConnection="{frontend_id}"/>')

    if flows["record"]["enabled"]:
        pid = int(flows["record"]["pid"])
        filters.append(
            '      <filter id="FILTER_TS_RECORD_0" mainType="TS" subType="RECORD" '
            f'bufferSize="{int(queues["record_filter_bytes"])}" pid="{pid}" '
            f'useFMQ="{"true" if probe else "false"}">'
            '<recordFilterSettings tsIndexMask="1" scIndexType="NONE"/></filter>'
        )
        dvr_size = int(queues["record_dvr_bytes"])
        dvrs.append(_dvr_xml("DVR_RECORD_0", "RECORD", dvr_size))
        data_flows.append(
            f'    <dvrRecord hasFrontendConnection="true" frontendConnection="{frontend_id}" '
            'recordFilterConnection="FILTER_TS_RECORD_0" dvrRecordConnection="DVR_RECORD_0"/>'
        )

    if flows["clear_live"]["enabled"]:
        live = flows["clear_live"]
        filters.extend([
            _av_filter_xml(
                "FILTER_TS_AUDIO_LIVE_0",
                "AUDIO",
                int(queues["audio_filter_bytes"]),
                int(live["audio_pid"]),
                int(live["audio_stream_type"]),
            ),
            _av_filter_xml(
                "FILTER_TS_VIDEO_LIVE_0",
                "VIDEO",
                int(queues["video_filter_bytes"]),
                int(live["video_pid"]),
                int(live["video_stream_type"]),
            ),
            '      <filter id="FILTER_TS_PCR_LIVE_0" mainType="TS" subType="PCR" '
            f'bufferSize="{int(queues["pcr_filter_bytes"])}" pid="{int(live["pcr_pid"])}" useFMQ="false"/>',
            _section_filter_xml(
                "FILTER_TS_SECTION_LIVE_0",
                int(queues["section_filter_bytes"]),
                int(live["section_pid"]),
                delay_hint_ms=SECTION_DELAY_HINT_MS,
            ),
        ])
        data_flows.append(
            f'    <clearLiveBroadcast frontendConnection="{frontend_id}" '
            'audioFilterConnection="FILTER_TS_AUDIO_LIVE_0" videoFilterConnection="FILTER_TS_VIDEO_LIVE_0" '
            'pcrFilterConnection="FILTER_TS_PCR_LIVE_0" sectionFilterConnection="FILTER_TS_SECTION_LIVE_0"/>'
        )

    if flows["playback"]["enabled"]:
        playback = flows["playback"]
        filters.extend([
            _av_filter_xml(
                "FILTER_TS_AUDIO_PLAYBACK_0",
                "AUDIO",
                int(queues["audio_filter_bytes"]),
                int(playback["audio_pid"]),
                int(playback["audio_stream_type"]),
            ),
            _av_filter_xml(
                "FILTER_TS_VIDEO_PLAYBACK_0",
                "VIDEO",
                int(queues["video_filter_bytes"]),
                int(playback["video_pid"]),
                int(playback["video_stream_type"]),
            ),
            _section_filter_xml(
                "FILTER_TS_SECTION_PLAYBACK_0",
                int(queues["section_filter_bytes"]),
                int(playback["section_pid"]),
            ),
        ])
        dvr_size = int(queues["playback_dvr_bytes"])
        dvrs.append(
            _dvr_xml(
                "DVR_PLAYBACK_0",
                "PLAYBACK",
                dvr_size,
                input_file_path=str(playback["input_file_path"]),
            )
        )
        data_flows.append(
            '    <dvrPlayback dvrConnection="DVR_PLAYBACK_0" '
            'audioFilterConnection="FILTER_TS_AUDIO_PLAYBACK_0" '
            'videoFilterConnection="FILTER_TS_VIDEO_PLAYBACK_0" '
            'sectionFilterConnection="FILTER_TS_SECTION_PLAYBACK_0"/>'
        )

    if rendered_profile["frontend"]["type"] == "ISDBS" and not probe:
        lnb = rendered_profile["lnb"]
        hardware.extend([
            "    <lnbs>",
            f'      <lnb id="LNB_0" voltage="{escape(str(lnb["voltage"]))}" '
            f'tone="{escape(str(lnb["tone"]))}" position="{escape(str(lnb["position"]))}"/>',
            "    </lnbs>",
        ])
        data_flows.extend([
            f'    <lnbLive frontendConnection="{frontend_id}" '
            'audioFilterConnection="FILTER_TS_AUDIO_LIVE_0" '
            'videoFilterConnection="FILTER_TS_VIDEO_LIVE_0" lnbConnection="LNB_0"/>',
            f'    <lnbRecord frontendConnection="{frontend_id}" '
            'recordFilterConnection="FILTER_TS_RECORD_0" '
            'dvrRecordConnection="DVR_RECORD_0" lnbConnection="LNB_0"/>',
        ])

    if filters:
        hardware.extend(["    <filters>", *filters, "    </filters>"])
    if dvrs:
        hardware.extend(["    <dvrs>", *dvrs, "    </dvrs>"])
    return (
        '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n'
        '<TunerConfiguration version="1.0" xmlns:xi="http://www.w3.org/2001/XInclude">\n'
        '  <hardwareConfiguration>\n' + "\n".join(hardware) + '\n  </hardwareConfiguration>\n'
        '  <dataFlowConfiguration>\n' + "\n".join(data_flows) + '\n  </dataFlowConfiguration>\n'
        '</TunerConfiguration>\n'
    )


def output_filename(profile: dict[str, Any]) -> str:
    variant = profile["vts"].get("variant", "")
    return f"tuner_vts_config_aidl_V1{'.' + variant if variant else ''}.xml"