from pathlib import Path

p = Path('tis/DESIGN_JA.md')
s = p.read_text()
old = '''各`MediaEvent`のtimestamp metadataもevent単位で透過的に扱う。`MediaEvent.getOffset()`をPTSの適用位置またはPES header位置とは解釈しない。`isPtsPresent()==true`なら、そのeventに付随する33-bit 90 kHz `getPts()`だけを`PtsNormalizer`へ入力し、同じeventのQueueRequestの`presentationTimeUs`に変換する。`isPtsPresent()==false`ならpayloadをdropせず、0、直前PTS、PCR、wallclock、frame rate、sample rateからPTSを捏造せず、別eventやcodec AUへPTSを再関連付けしない。明示PTSのないeventについてはfabricated timestampを設定せずdirect queueし、この入力を受理できないdecoder/profileは上記qualificationで非対応にする。'''
new = '''各`MediaEvent`のtimestamp metadataもevent単位で透過的に扱う。`MediaEvent.getOffset()`をPTSの適用位置またはPES header位置とは解釈しない。`isPtsPresent()==true`なら、そのeventに付随する33-bit 90 kHz `getPts()`だけを`PtsNormalizer`へ入力し、同じeventのQueueRequestの`presentationTimeUs`に変換する。Android 14の`MediaCodec.QueueRequest`はpresentation timestampのabsenceを表現できず、setterを呼ばない場合も0がqueueされるため、setter未呼出しを「timestampなし」として利用しない。一方、Tuner HALの`DemuxFilterMediaEvent.isPtsPresent`はPTSがPES headerに存在するかを表現する有効状態である。したがって本製品のclear non-passthrough direct-input **成功対応profile** は、MediaCodecへ渡すすべてのnon-empty `MediaEvent`が`isPtsPresent()==true`を満たすことをplayback capability qualificationの必須条件とする。これはTuner AIDL/VINTF/VTS契約を強化・変更するものではなく、TISが成功対応として表明するproduct capabilityの境界である。`isPtsPresent()==false`を受けた場合は当該eventを`QueueRequest`へqueueせず、0、直前PTS、PCR、wallclock、frame rate、sample rateからtimestampを捏造せず、別eventやcodec AUへPTSを再関連付けせず、個別fragmentのみdropして再生継続もしない。current selected track/profileを`UNSUPPORTED_DIRECT_INPUT_MISSING_PTS`の型付きplayback failureとして扱い、current playback generationを終了し、そのprofileを成功対応として表明しない。qualificationの最低試験には`isPtsPresent()==false`のnon-empty eventを注入し、`QueueRequest.queue()`が発行されないこと、測定値0が投入されないこと、当該profileが成功対応へ昇格しないことを含める。'''
assert old in s
s = s.replace(old, new, 1)
s = s.replace('`isPtsPresent()==false`のeventはcoordinator / normalizerを進めない。', '`isPtsPresent()==false`のeventは上記product capability境界でMediaCodec queue前にplayback failureへ遷移させ、coordinator / normalizerを進めない。', 1)
p.write_text(s)

p = Path('arib_si_engine_rs/DESIGN_JA.md')
s = p.read_text()
old = '''ARIB適合性の規範対象と検証証拠の分離は `../開発規則.md` を正とする。本decoderについて条項単位に取得・確認し検証証拠として使用する本文は ARIB 公式英語版 STD-B24 6.4-E1 Fascicle 1 とし、従来の8単位符号については7.1.1.1〜7.1.2.4をinvocation・designation・文字集合・Macro・制御機能の根拠として用いる。UCS符号方式についても同FascicleのUCS文字符号化規定を検証証拠に含める。ARIB公式の改定履歴上、UCSは既存STD-B24の正式な符号方式として維持・修正されているため、本crateのSI/EPG文字列対応から時点依存で除外しない。この英語版を現行日本語原文そのものとは扱わず、版差がある場合は未証明差分を残す。改定概要、版一覧、二次資料を未取得本文の具体規定の代用にしない。STD-B24の字幕レンダリングや他Fascicleへの適合は本decoderの主張に含めない。'''
new = '''ARIB適合性の規範対象と検証証拠の分離は `../開発規則.md` を正とする。本decoderについて条項単位に取得・確認し検証証拠として使用する本文は ARIB 公式英語版 STD-B24 6.4-E1 Fascicle 1 とし、従来の8単位符号については7.1.1.1〜7.1.2.4をinvocation・designation・文字集合・Macro・制御機能の根拠として用いる。UCSは同Fascicle第一編第2部7.2.1〜7.2.3を根拠とし、特に7.2.3のcharacter encoding schemeをcoding formとBOM/byte-order判定のSSOTにする。7.2.3で伝送に用いる符号化方式はISO/IEC 10646に基づくUTF-8またはUTF-16であり、UTF-16はhigh-byte-first (big-endian) かつBOMを省略せず、UTF-8ではBOMを使用しない。したがってUCS入力を受けたdecoderはheuristicなbyte-order推測を行わず、先頭`FE FF`をUTF-16BEの必須BOMとして認識して除去し、`FF FE`はlittle-endianとして規格外入力にし、`EF BB BF`はUTF-8で禁止されたBOMとして規格外入力にする。`FE FF`がない入力をUTF-16とは解釈せずUTF-8として検証する。UCSの文字集合・制御符号は7.2.1／7.2.2の規定を同じdecoder stateへ適用する。ARIB公式の改定履歴上、UCSは既存STD-B24の正式な符号方式として維持・修正されているため、本crateのSI/EPG文字列対応から時点依存で除外しない。この英語版を現行日本語原文そのものとは扱わず、版差がある場合は未証明差分を残す。改定概要、版一覧、二次資料を未取得本文の具体規定の代用にしない。STD-B24の字幕レンダリングや他Fascicleへの適合は本decoderの主張に含めない。'''
assert old in s
s = s.replace(old, new, 1)
old = '''| UCS | STD-B24でUCS符号方式としてsignalingされたSI/EPG文字列を対応能力に含める。適用されるUCS符号化方式としてUTF-8／UTF-16を判別し、妥当なUnicode scalar列へ復号する。BOM、byte order、切詰めcode unit、illegal sequenceを推測で修復せず、strict APIではエラー、lossy APIでは`U+FFFD`と診断にする |'''
new = '''| UCS | STD-B24 Fascicle 1 第一編第2部7.2.1〜7.2.3に従うUCS文字列を対応能力に含める。7.2.3に従いUTF-8／UTF-16だけを許可し、`FE FF`先頭ならBOMを消費してUTF-16BE、BOMなしならUTF-8として検証する。UTF-16LE (`FF FE`)、BOMなしUTF-16、UTF-8 BOM (`EF BB BF`)、切詰めcode unit／surrogate、illegal UTF-8 sequenceは推測修復しない。strict APIでは規格外／不正入力をエラー、lossy APIでは`U+FFFD`と条項・offset付き診断にする。最低試験はvalid UTF-8 without BOM、valid UTF-16BE with `FE FF`、UTF-8 BOM拒否、UTF-16LE BOM拒否、UTF-16 BOM欠落をUTF-16へ推測しないこと、truncated/illegal sequenceのstrict/lossy差を含む |'''
assert old in s
s = s.replace(old, new, 1)
p.write_text(s)
