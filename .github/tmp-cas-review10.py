from pathlib import Path

p = Path('tis/DESIGN_JA.md')
s = p.read_text(encoding='utf-8')
old = '''ES PIDのlogical ownerは全active CA systemの集合で照合する。別systemが同じPIDを使用している間はremoveしない。addPid成功済みのPIDだけをdescramblerPidsへ記録し、全owner消滅後のremovePid成功でのみ除去する。この集合とactive集合の差が未解放PIDであり、独立したpending集合を複製しない。非SUCCESSはCAS診断failureとして保持し、次のmetadata更新・clearで未解放分だけ再試行する。bridge全体のclose成功時はPID所有も解消する。'''
new = '''ES PIDの物理所有はCA system専用Descramblerごとに管理する。各DescramblerへaddPid成功済みのPIDだけを当該systemのdescramblerPidsへ記録し、そのsystemのbinding消滅後のremovePid成功でのみ除去する。同一ES PIDが同じcurrent snapshotで複数supported CA systemへbindingされる入力は前段のambiguous判定でfail-closedにするため、異なるDescrambler間で同一PIDの共有所有を作らない。descramblerPidsと当該systemのactive PID集合の差が未解放PIDであり、独立したpending集合を複製しない。非SUCCESSはCAS診断failureとして保持し、次のmetadata更新・clearで当該systemの未解放分だけ再試行する。bridge全体のclose成功時はそのsystemのPID所有も解消する。'''
if s.count(old) != 1:
    raise SystemExit(f'expected one legacy ownership paragraph, got {s.count(old)}')
p.write_text(s.replace(old, new, 1), encoding='utf-8')
