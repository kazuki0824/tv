from pathlib import Path

path = Path("tuner_hal/DESIGN_JA.md")
text = path.read_text(encoding="utf-8")

old = "フィルターの通常FMQペイロード、DVR記録ストリーム、TS/MMTP記録コールバックのメタデータは、互いに独立した3つの経路として扱う。TS/MMTP記録フィルターは通常のフィルターFMQを公開しない。ペイロードは接続先のRecord DVR FMQだけへ書き込み、PID、索引、バイト番号、PTS、開始コードのメタデータは `DemuxFilterTsRecordEvent` または `DemuxFilterMmtpRecordEvent` のコールバックで通知する。SectionとTS生データのペイロードフィルターは通常のフィルターFMQを使用する。PESペイロードフィルターはARIB字幕用の明示`streamId=0xBD`かつbounded PESに限定して公開し、通常FMQを使用する。長さ0のvideo PESは公開対象にしない。"
new = "フィルターの通常FMQペイロード、DVR記録ストリーム、TS/MMTP記録コールバックのメタデータは、互いに独立した3つの経路として扱う。TS/MMTP記録フィルターは通常のフィルターFMQを公開しない。ペイロードは接続先のRecord DVR FMQだけへ書き込み、PID、索引、バイト番号、PTS、開始コードのメタデータは `DemuxFilterTsRecordEvent` または `DemuxFilterMmtpRecordEvent` のコールバックで通知する。Section、PES、TS生データのペイロードフィルターは通常のフィルターFMQを使用する。PESは有効な明示`streamId 0..255`とwildcard `0xFFFF`を同じ能力で受理し、映像`0xE0..0xEF`の長さ0 PESもruntime組立て対象とする。"
if text.count(old) != 1:
    raise SystemExit(f"PES state paragraph match count={text.count(old)}")
text = text.replace(old, new)

old = "AV payloadは配送時、bounded PESは開始時、FMQとplayback処理中bufferはconfigure時に実領域を確保する。"
new = "AV payloadは配送時、宣言長ありPESはヘッダーから必要量を確定した時点、長さ0映像PESは受信量の増加時、FMQとplayback処理中bufferはconfigure時に実領域を確保する。"
if text.count(old) != 1:
    raise SystemExit(f"allocation timing match count={text.count(old)}")
text = text.replace(old, new)

for forbidden in (
    "PESペイロードフィルターはARIB字幕用の明示`streamId=0xBD`",
    "長さ0のvideo PESは公開対象にしない",
    "bounded PESは開始時",
):
    if forbidden in text:
        raise SystemExit(f"stale contract remains: {forbidden}")

path.write_text(text, encoding="utf-8")
