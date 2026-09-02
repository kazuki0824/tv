from pathlib import Path

source_path = Path(__file__).with_name('codex_apply_pr54_delta5.py')
source = source_path.read_text(encoding='utf-8')
old = '''def replace_once(path, old, new, label):
    p = ROOT / path
    text = p.read_text(encoding='utf-8')
    count = text.count(old)
    if count != 1:
        raise SystemExit(f'{label}: expected 1 occurrence, found {count}')
    p.write_text(text.replace(old, new, 1), encoding='utf-8')
'''
new = '''def replace_once(path, old, new, label):
    p = ROOT / path
    text = p.read_text(encoding='utf-8')
    if label == 'obsolete projection ownership':
        marker = '        override fun deleteObsoletePrograms(channelId: Long, validProgramKeys: Set<String>, windowStartMs: Long, windowEndMs: Long): Result<Int> = runCatching {'
        if text.count(marker) != 1:
            raise SystemExit(f'{label}: delete function marker count={text.count(marker)}')
        prefix, tail = text.split(marker, 1)
        count = tail.count(old)
        if count != 1:
            raise SystemExit(f'{label}: expected 1 occurrence in delete function, found {count}')
        p.write_text(prefix + marker + tail.replace(old, new, 1), encoding='utf-8')
        return
    count = text.count(old)
    if count != 1:
        raise SystemExit(f'{label}: expected 1 occurrence, found {count}')
    p.write_text(text.replace(old, new, 1), encoding='utf-8')
'''
if source.count(old) != 1:
    raise SystemExit(f'replace_once definition count={source.count(old)}')
exec(compile(source.replace(old, new, 1), str(source_path), 'exec'))
