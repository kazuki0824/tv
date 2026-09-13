from pathlib import Path

p = Path('cas_plugin/DESIGN_JA.md')
s = p.read_text(encoding='utf-8')

old = "引数なしの `openSession(CasSessionId*)` はframeworkのdefault session openであり、B25/B1とも各CA方式のscheme-default MULTI2 sessionを生成する。typed `openSession(intent, mode, ...)` はB25/B1で `LIVE + MULTI2` を通常入力として受理する。AOSP Tuner AIDL VTSのdescrambling profileで当該CA systemを使用する場合は `LIVE + RESERVED` をscheme-default MULTI2への互換入力として受理し、default open / `LIVE + MULTI2` と同じsession coreを生成する。`RESERVED` を別のscrambling algorithmとして広告・実装しない。これ以外の非対応intent/modeはstateを変更せずcannot-handle相当statusを返す。"
new = "引数なしの `openSession(CasSessionId*)` はframeworkのdefault session openであり、B25/B1とも各CA方式のscheme-default MULTI2 sessionを生成する。typed `openSession(intent, mode, ...)` はB25/B1で `LIVE + MULTI2` を通常入力として受理する。これ以外の非対応intent/modeはstateを変更せずcannot-handle相当statusを返す。"
assert old in s
s = s.replace(old, new, 1)

old = "B1 pluginは§3の共通AOSP `CasPlugin` ABI契約と§4の共通lifecycle契約に従う。default `openSession(CasSessionId*)` はB1 scheme-default MULTI2 sessionを生成し、typed `openSession(intent, mode, ...)` は `LIVE + MULTI2` を受理する。AOSP Tuner AIDL VTSのdescrambling profileでB1を使用する場合は `LIVE + RESERVED` も同じB1 MULTI2 session coreへの互換入力として受理する。その他の非対応intent/modeはstateを変更せずcannot-handle相当statusを返す。"
new = "B1 pluginは§3の共通AOSP `CasPlugin` ABI契約と§4の共通lifecycle契約に従う。default `openSession(CasSessionId*)` はB1 scheme-default MULTI2 sessionを生成し、typed `openSession(intent, mode, ...)` は `LIVE + MULTI2` を受理する。その他の非対応intent/modeはstateを変更せずcannot-handle相当statusを返す。"
assert old in s
s = s.replace(old, new, 1)

for line in [
    "- B25をTuner VTS descrambling profileに使う場合、typed `LIVE + RESERVED` がscheme-default MULTI2互換入力として成功する\n",
    "- B1をTuner VTS descrambling profileに使う場合、typed `LIVE + RESERVED` がscheme-default MULTI2互換入力として成功する\n",
]:
    assert line in s
    s = s.replace(line, "", 1)

p.write_text(s, encoding='utf-8')
