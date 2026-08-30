# Linux tc90522 ISDB-S symbol-rate capability metadata 改善候補

## 位置付け

この文書は、upstream Linuxの`tc90522` ISDB-S frontendが固定symbol rateを使用する一方、`FE_GET_INFO`用のcapability metadataへその値を公開していない点について、上流改善の調査候補と再評価条件を記録する。

現行製品のcapability、public settings受付、backend投影の規範正本は`../../tuner_hal/DESIGN_JA.md`とし、本書を現行仕様、実装済み範囲、対応宣言または完了判定の根拠にしない。現行製品runtimeはupstream変更へ依存せず、pinned earth-pt1 / tc90522 / qm1d1b0004 profileの固定28,860,000 sym/s契約だけで完結する。

## 現状

Linux v6.6 `drivers/media/dvb-frontends/tc90522.c`の`tc90522_ops_sat.info`はISDB-Sの周波数範囲を設定するが、`symbol_rate_min` / `symbol_rate_max`を設定しない。このためDVB coreの`FE_GET_INFO`が返すsymbol-rate capabilityは0/0となる。

同driverの`tc90522s_get_frontend()`はcurrent propertyへ`symbol_rate=28,860,000`を設定する。採用tuner `drivers/media/tuners/qm1d1b0004.c`もproperty cacheのsymbol rateをLPF設定に使用し、28.86 Mbaud時の設定を記載している。現行製品ではこのpinned module構成を固定profileの証拠とし、`FE_GET_INFO`の0/0を受信不能または0 sym/s能力とは解釈しない。

## upstream候補

実hardwareとdriver/module構成の契約が固定28,860,000 sym/sであることを十分に検証した上で、`tc90522_ops_sat.info.symbol_rate_min`と`symbol_rate_max`へ28,860,000相当を設定する変更をLinux media subsystemへ提案できるか調査する。

投稿前に少なくとも次を確認する。

- 固定値がtc90522 demodulator単体の能力か、接続するtunerまたはmodule構成に依存する値か。
- earth-pt1、PT1、PT2以外のtc90522利用者と接続tunerへの影響。
- 0/0をunknown capabilityとして扱う既存userspaceに対する互換性影響。
- fixed symbol-rate capabilityをdemodulatorとtuner/moduleのどのdriver層で公開するのがLinux media maintainerの期待に合うか。

上記が未確認の間は「upstream kernelの不具合」と断定せず、capability metadata改善候補として扱う。

## 再評価条件

次のすべてが成立した場合だけ、現行製品を`FE_GET_INFO`由来のsymbol-rate capabilityへ戻す設計変更を検討する。

- upstream採用または製品kernelへの正式backportが存在する。
- 採用commitを製品証跡として固定できる。
- 対象earth-pt1構成で`FE_GET_INFO`の非0値と`DTV_SYMBOL_RATE`適用を確認できる。
- 公開`FrontendInfo`、public settings受付、backend投影を同じ証跡へ同期できる。

## 参照

- Linux v6.6 `tc90522.c`: https://github.com/torvalds/linux/blob/v6.6/drivers/media/dvb-frontends/tc90522.c
- Linux v6.6 DVB core `dvb_frontend.c`: https://github.com/torvalds/linux/blob/v6.6/drivers/media/dvb-core/dvb_frontend.c
- Linux v6.6 `qm1d1b0004.c`: https://github.com/torvalds/linux/blob/v6.6/drivers/media/tuners/qm1d1b0004.c
