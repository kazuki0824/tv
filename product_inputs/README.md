# 製品入力

このディレクトリは、共通product入口が既定で使用する製品入力を保持する。

- `b25_multi2_parameters`: B25 MULTI2用の40 byte binary。先頭32 byteがsystem key、後続8 byteがCBC初期値。値は秘密情報として扱わない。
- `bcas_keys`: Yakisobaの構文を満たすdummy credential。ビルド・組込み確認用であり、実放送を復号できない。

実放送用のYakisoba credentialはGitへ追加しない。製品側で実credentialを使用するときは、共通product入口を継承する前に`MALEICACID_BCAS_KEYS_FILE`をrepository外または秘密管理されたファイルへ上書きする。

`b25_multi2_parameters`のSHA-256は`e7fd0183fc43322cc2512c27a6f5347510311e223bca516c83ccbe0b593f7e91`である。
