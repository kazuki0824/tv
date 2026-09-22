# TIS 実装規約

共通の実装規約は `../GLOBAL_CODE_CONVENTION.md` を正とする。SIの実行失敗に対するTISの振る舞いは `DESIGN_JA.md` の「SI収集の期限と失敗境界」に従う。

## SIのJNI呼出し

- `NativeAribSiParser` の文字列を返す `external` 宣言は `String?` とし、戻り値は `requireNativeString` を通して受け取る。呼出し先ごとにnull検査を複製しない。
- `NativeSiException` はコンストラクターで受け取った失敗理由の識別子を `NativeSiFailureReason` へ厳密に変換する。
- `ProviderDataBridge` でJSON解析を `runCatching` により囲む場合、JNI呼出しはその外で実行し、取得した文字列だけを解析処理へ渡す。
