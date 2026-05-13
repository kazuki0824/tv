# AV sync wallclock 後続検討

## 目的

この文書は、Tuner HAL の A/V sync wallclock 補間について、現行実装で維持する境界と後続改善候補を整理する。過去版の調査経緯ではなく、将来の実装判断に必要な現行仕様だけを残す。

## 現行で維持する境界

- PCR を観測できない場合は、有効な sync id を返さない。
- PTS だけを根拠にした fallback sync id は採用しない。
- PCR と monotonic clock による最小補間を維持する。
- `AvSyncState` は、PCR PID 明示管理、service clock、jitter smoothing、PLL 型補正へ拡張できる構造にする。

## 後続改善候補

- PCR PID の明示管理。
- service clock モデルの導入。
- jitter smoothing と clock discipline の導入。
- 複数 clock source の品質評価。
- 実波、CTS、VTS を使った補正品質評価。

## 採用しない方針

- PCR 未観測時に PTS だけで valid sync id を生成すること。
- wallclock 補間の誤差を診断せず、成功扱いにすること。
- MediaEvent を shared handle 未 export 状態で配送すること。
