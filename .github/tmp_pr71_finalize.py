from pathlib import Path
import subprocess

p = Path('tuner_hal2/INTEGRATION.md')
s = p.read_text(encoding='utf-8')
old = '''`init`では具体的な受信チャンネルを必須入力にせず、地上波では地域指定から候補を導出できる。地域入力は少なくとも郵便番号、住所、緯度経度のいずれかを表現可能とし、市区町村等の粗い入力で複数候補が残る場合は候補集合を同じprofileファイルに保存して一意の周波数を捏造しない。

地域resolverは、放送エリア/チャンネル計画datasetを入力とする。dataset versionの識別規格は設けず、`VtsEnvironmentProfile`にもdataset versionまたはdataset識別情報を永続化しない。resolverはその実行で使用するdatasetから受信候補を生成する。

地上波の地域resolverが生成してよいのは、送信所または受信エリアに対応するdelivery system、物理チャンネル、frequency等の**受信候補**である。候補に含まれることを、その地点・アンテナ・配線・tunerで実際に受信可能である証明として扱わない。BS/110度CS等、地域による送信周波数候補の選択を必要としない方式では、地域情報を周波数選択の擬似根拠にせず、対象transport候補表から候補を構成する。
'''
new = '''`init`では具体的な受信チャンネルを必須入力にせず、地上波では地域指定から候補を導出できる。地域入力は住所、郵便番号、緯度経度を表現可能とし、都道府県等の粗い入力で複数候補が残る場合は候補集合を同じprofileファイルに保存して一意の周波数を捏造しない。

住所と郵便番号はまず緯度経度へ解決し、緯度経度入力はその座標を直接使用する。都道府県だけを明示した粗い入力を除き、以後の地上波候補解決は座標を共通の中間表現とする。座標からGSI reverse geocoderで市区町村コードを取得し、そのコードに対応する行政エリアを放送エリア/チャンネル計画datasetの「主なカバーエリア」と照合して物理チャンネル候補を生成する。住所文字列を直接channel-planのarea keyへsubstring照合する経路、住所を町丁目・番地まで正規化してから再び文字列照合する経路は設けない。

地域resolverは、放送エリア/チャンネル計画datasetを入力とする。dataset versionの識別規格は設けず、`VtsEnvironmentProfile`にもdataset versionまたはdataset識別情報を永続化しない。resolverはその実行で使用するdatasetから受信候補を生成する。

現在利用できる公開データは送信波の正確な受信可能polygonを提供するものではないため、行政エリアと「主なカバーエリア」の対応は候補生成だけに使用する。地上波の地域resolverが生成してよいのは、送信所または受信エリアに対応するdelivery system、物理チャンネル、frequency等の**受信候補**である。候補に含まれることを、その地点・アンテナ・配線・tunerで実際に受信可能である証明として扱わない。BS/110度CS等、地域による送信周波数候補の選択を必要としない方式では、地域情報を周波数選択の擬似根拠にせず、対象transport候補表から候補を構成する。
'''
if old not in s:
    raise SystemExit('INTEGRATION 6.4 block not found')
p.write_text(s.replace(old, new, 1), encoding='utf-8')

p = Path('tuner_hal2/tools/vts_profile/cli.py')
s = p.read_text(encoding='utf-8')
s = s.replace('optional explicit versioned region dataset', 'optional explicit region dataset')
s = s.replace('repository snapshot to map the address to regional physical-channel candidates', 'repository snapshot after resolving the region input through coordinates')
p.write_text(s, encoding='utf-8')

subprocess.run(['python', '-m', 'unittest', 'test_vts_region_defaults.py', 'test_vts_profile.py'], cwd='tuner_hal2/tools', check=True)
subprocess.run(['python', '-m', 'py_compile', 'vts_profile/region.py', 'vts_profile/cli.py', 'test_vts_region_defaults.py', 'test_vts_profile.py'], cwd='tuner_hal2/tools', check=True)
region = Path('tuner_hal2/tools/vts_profile/region.py').read_text(encoding='utf-8')
assert 'japanese_address_parser_py' not in region
assert '_normalize_address' not in region
assert '_region_coordinates' in region
assert '_coordinate_area' in region
assert 'dataset_version' not in region

subprocess.run(['git', 'config', 'user.name', 'github-actions[bot]'], check=True)
subprocess.run(['git', 'config', 'user.email', '41898282+github-actions[bot]@users.noreply.github.com'], check=True)
subprocess.run(['git', 'add', 'tuner_hal2/INTEGRATION.md', 'tuner_hal2/tools/vts_profile/cli.py'], check=True)
subprocess.run(['git', 'commit', '-m', 'docs: align VTS region flow with coordinates'], check=True)
subprocess.run(['git', 'push', 'origin', 'HEAD:fix/tuner-hal2-vts-auto-region'], check=True)
