from pathlib import Path

path = Path("arib_si_engine_rs/src/provider_data.rs")
text = path.read_text(encoding="utf-8")

# These are cfg(test) initializers only; production serde fields remain optional/defaulted.
video_old = '''            stream_type: {stream_type},
            component_tag: {component_tag},
            component_type: {component_type},
            codec: {codec},
            resolution: {resolution},
'''

def patch_video(block: str) -> str:
    lines = block.splitlines()
    out = []
    for line in lines:
        out.append(line)
        if "stream_type:" in line:
            out.append("            stream_content: None,")
        if "codec:" in line:
            out.append("            language: None,")
            out.append("            text: None,")
    return "\n".join(out)

# Patch the two VideoComponentV1 test literals inside nullable_component_fact_tests.
start = text.index("mod nullable_component_fact_tests")
section = text[start:]
needle = "VideoComponentV1 {"
pos = 0
for _ in range(2):
    begin = section.index(needle, pos)
    end = section.index("        }));", begin) + len("        }))")
    block = section[begin:end]
    patched = patch_video(block)
    section = section[:begin] + patched + section[end:]
    pos = begin + len(patched)
text = text[:start] + section

# Patch the single AudioComponentV1 test literal.
start = text.index("mod nullable_component_fact_tests")
section = text[start:]
begin = section.index("AudioComponentV1 {")
end = section.index("        }));", begin) + len("        }))")
block = section[begin:end]
lines = block.splitlines()
out = []
for line in lines:
    out.append(line)
    if "stream_type:" in line:
        out.append("            stream_content: None,")
    if "channel_configuration:" in line:
        out.append("            simulcast_group_tag: None,")
        out.append("            sampling_rate: None,")
    if "sampling_info:" in line:
        out.append("            text: None,")
        out.append("            main: None,")
        out.append("            multi_lingual: None,")
        out.append("            quality_indicator: None,")
patched = "\n".join(out)
section = section[:begin] + patched + section[end:]
text = text[:start] + section

path.write_text(text, encoding="utf-8")
print("updated nullable component fact Rust test initializers")
