# r50cw
- r51対象外の準備領域として、録画側の動的SI/CASフィルタ更新が廃止 snapshot wrapper を参照しないよう `casDiscoverySnapshot()` 経由へ追随した。
- rec は引き続き r53 準備領域であり、r51 product package / release確認条件には含めない。
- Android/Soong build、Kotlin compile、instrumentationテスト、atest、CTS、実機確認は未実施。静的差分確認のみ実施した。

# r50cb
- WP-13対応として、`MaleicacidRecScopeTests` が r51確認対象外であり r53で明示実行する試験モジュールであることは、`tis/INTEGRATION.md` の録画・予約除外および r51 ビルド・試験確認ゲートを正とする。
- rec 実装コードは変更していない。Android/Soong build、Kotlin compile、instrumentationテスト、atest、CTS、実機確認は未実施。

# r50bz
- WP-11対応として、`rec/` 配下は r53 準備領域であり r51 product package / release確認条件へ含めないことを明記した。
- `MaleicacidRecScopeTests` を device-tests suite から外し、r53作業で明示指定する test module とした。
- Android/Soong build、Kotlin compile、instrumentationテスト、atest、CTS、実機確認は未実施。

# CHANGELOG

## r50ba2

- Replaced the direct `../tis/src/com/maleicacid/tvinput/common/ChannelKeys.kt` source path with the `//vendor/maleicacid/tv/tis:maleicacid_tvinput_channel_keys_sources` filegroup reference.
- No reservation Kotlin implementation, manifest, or test logic was changed.
