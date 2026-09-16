from pathlib import Path


def replace_once(text: str, old: str, new: str) -> str:
    if text.count(old) != 1:
        raise SystemExit(f"expected exactly one match: {old[:80]!r}")
    return text.replace(old, new, 1)


path = Path("cas_plugin/DESIGN_JA.md")
text = path.read_text()

old = """Yakisobaの認証情報は製品に固定配置する初期入力とする。初期化時に一度だけ読み込み、通常ファイルでない場合、空または上限を超える場合、開く処理や読取りに失敗した場合は未初期化として拒否する。構文の解釈は採用したlibyakisobaに委ねる。初期化成功後は、ディスク上の入力の削除・差替え・所有者や権限の変更を監視・再検証せず、動的鍵状態の失効原因にしない。配置とアクセス権の設定は [INTEGRATION.md](INTEGRATION.md) が扱う。
"""
new = old + """
EMMで確定したwork keyと権利状態は、固定credentialを書き換えず、CASが所有する可変状態として`/data/vendor/maleicacid/cas/yakisoba_state`へ永続化する。永続状態はcard ID、各BroadcasterGroupのwork key、更新番号、有効期限、権利bitmap、重複配送判定に必要な情報を一体で保持し、再起動後の正本とする。libyakisobaのprocess内Keysetはこの永続状態から復元する実行時状態であり、別の永続正本にしない。永続状態が壊れている、card IDと一致しない、または確定結果を判定できない場合は旧状態を推測して継続せず失効させる。配置、所有者、mode、SELinuxは [INTEGRATION.md](INTEGRATION.md) が扱う。
"""
text = replace_once(text, old, new)

old = """libyakisobaのwork key台帳と初期化状態はprocess内で共有されるため、複数pluginにまたがる初期化、ECM参照、EMM更新は同じbackend resource ownerで直列化する。台帳の複製、pluginごとの別Kw cache、service全体のbackend選択器を追加しない。初期化前の登録が後続初期化で失われない順序を守る。内部 `Register()` の拒否を成功へ変換せず、同一内容の既適用更新と、未対応更新・状態不整合を区別する。

EMM処理は適用対象messageごとに検証と更新を確定し、関連ECMが更新途中の台帳を観測しないようにする。複数messageの途中失敗では、既に適用した更新を未適用と偽らず、未適用分を成功扱いせず、対応する失敗を返す。後続再配送では既適用更新を重複適用しない。全sectionを巻き戻すための第二台帳は必須化しない。
"""
new = """libyakisobaのwork key台帳と初期化状態はprocess内で共有されるため、複数pluginにまたがる初期化、ECM参照、EMM更新は同じbackend resource ownerで直列化する。pluginごとの別Kw cache、service全体のbackend選択器を追加しない。初期化前の登録が後続初期化で失われない順序を守る。内部 `Register()` の拒否を成功へ変換せず、同一内容の既適用更新と、未対応更新・状態不整合を区別する。

採用するlibyakisobaの`Register()`は呼出しごとにprocess内Keysetをその場で変更し、複数登録をまとめて検証する操作も、途中まで書き換えた登録を元へ戻す操作も提供しない。このため1個のEMM messageに複数work key更新がある場合、先の`Register()`だけ成功した後に後続登録が拒否されると、message全体は失敗なのにKeysetだけ一部更新された状態になる。これを避けるため、adapterは最初の書込み前に、対応group、`WorkKeyID % 10`の登録先、既存WorkKeyIDとの大小・同一内容という`Register()`の受理条件だけを同じ採用sourceに合わせて事前検査する。この内部知識の重複は書込み可否の事前判定に限定し、独立したKeyset実装へ拡張しない。採用libyakisobaの固定revisionに対して、この事前判定と実際の`Register()`の結果が一致することを試験する。

EMM処理は適用対象messageごとに、復号後command、権利条件、全work key登録の事前検査を完了してから次状態を構成する。次状態は一時fileへの全量書込み、fileの同期、同一filesystem内の置換、親directoryの同期まで完了した時点を永続確定点とし、その後に同じ更新をprocess内Keysetへ`Register()`して権利状態を切り替える。永続確定前の失敗ではprocess内状態を変更しない。永続確定後の`Register()`失敗は事前判定とlibyakisoba実装の不一致を意味するため継続せず失効させる。置換後に永続確定の成否を判定できない場合も失効させる。複数messageの途中失敗では、既にmessage単位で永続確定した更新を未適用と偽らず、未適用分を成功扱いしない。後続再配送では永続化した更新番号と配送識別情報を用いて既適用更新を重複適用しない。
"""
text = replace_once(text, old, new)

old = "- KsやKw等の可変秘密情報の一時表現は必要期間を越えて保持・永続化しない。特定のzeroize APIやmemory primitiveを必須化しない。"
new = "- sessionごとのKsと復号途中の一時鍵表現は必要期間を越えて保持・永続化しない。EMMで確定したYakisobaのKwと権利状態だけは§6のCAS所有永続状態へ保存し、Tuner側、固定credential、別cacheへ永続化しない。特定のzeroize APIやmemory primitiveを必須化しない。"
text = replace_once(text, old, new)

path.write_text(text)
