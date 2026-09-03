from __future__ import annotations
from typing import Any
from xml.sax.saxutils import escape
from .model import FRONTEND_ID, RECORD_FILTER_FMQ_PROBE_VARIANT, validate_profile


def _frontend_xml(profile: dict[str, Any]) -> str:
    fe = profile["frontend"]
    fe_type = fe["type"]
    attrs = [
        f'id="{FRONTEND_ID[fe_type]}"',
        f'type="{fe_type}"',
        'isSoftwareFrontend="false"',
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


def render_xml(profile: dict[str, Any]) -> str:
    validate_profile(profile, require_resolved=True)
    flows = profile["flows"]
    queues = profile["queues"]
    hardware = ["    <frontends>", _frontend_xml(profile), "    </frontends>"]
    filters: list[str] = []
    dvrs: list[str] = []
    data_flows: list[str] = []
    frontend_id = FRONTEND_ID[profile["frontend"]["type"]]
    if flows["scan"]:
        data_flows.append(f'    <scan frontendConnection="{frontend_id}"/>')
    if flows["record"]["enabled"]:
        pid = int(flows["record"]["pid"])
        record_filter_uses_fmq = (
            profile["vts"].get("variant", "") == RECORD_FILTER_FMQ_PROBE_VARIANT
        )
        filters.append(
            '      <filter id="FILTER_TS_RECORD_0" mainType="TS" subType="RECORD" '
            f'bufferSize="{int(queues["record_filter_bytes"])}" pid="{pid}" '
            f'useFMQ="{"true" if record_filter_uses_fmq else "false"}">'
            '<recordFilterSettings tsIndexMask="1" scIndexType="NONE"/></filter>'
        )
        dvr_size = int(queues["record_dvr_bytes"])
        dvrs.append(
            '      <dvr id="DVR_RECORD_0" type="RECORD" '
            f'bufferSize="{dvr_size}" statusMask="15" lowThreshold="{dvr_size // 4}" '
            f'highThreshold="{dvr_size * 3 // 4}" dataFormat="TS" packetSize="188"/>'
        )
        data_flows.append(
            f'    <dvrRecord hasFrontendConnection="true" frontendConnection="{frontend_id}" '
            'recordFilterConnection="FILTER_TS_RECORD_0" dvrRecordConnection="DVR_RECORD_0"/>'
        )
    if flows["clear_live"]["enabled"]:
        live = flows["clear_live"]
        filters.extend([
            '      <filter id="FILTER_TS_AUDIO_0" mainType="TS" subType="AUDIO" '
            f'bufferSize="{int(queues["audio_filter_bytes"])}" pid="{int(live["audio_pid"])}" useFMQ="false">'
            f'<avFilterSettings isPassthrough="false" isSecureMemory="false"><audioStreamType>{int(live["audio_stream_type"])}</audioStreamType></avFilterSettings></filter>',
            '      <filter id="FILTER_TS_VIDEO_0" mainType="TS" subType="VIDEO" '
            f'bufferSize="{int(queues["video_filter_bytes"])}" pid="{int(live["video_pid"])}" useFMQ="false">'
            f'<avFilterSettings isPassthrough="false" isSecureMemory="false"><videoStreamType>{int(live["video_stream_type"])}</videoStreamType></avFilterSettings></filter>',
        ])
        data_flows.append(
            f'    <clearLiveBroadcast frontendConnection="{frontend_id}" '
            'audioFilterConnection="FILTER_TS_AUDIO_0" videoFilterConnection="FILTER_TS_VIDEO_0"/>'
        )
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
