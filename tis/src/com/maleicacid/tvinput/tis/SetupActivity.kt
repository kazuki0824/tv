package com.maleicacid.tvinput.tis

import android.app.Activity
import android.os.Bundle
import android.view.ViewGroup
import android.widget.Button
import android.widget.LinearLayout
import android.widget.TextView

class SetupActivity : Activity(), ChannelScanManager.Listener {
    private lateinit var statusView: TextView
    private lateinit var scanButton: Button
    private lateinit var cancelButton: Button
    private var inputId: String? = null
    private var invalidInputId: Boolean = false
    private var setupGeneration: Int? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        inputId = resolveInputId()
        invalidInputId = inputId.isNullOrBlank() || !isOwnInputId(inputId)
        setupGeneration = savedInstanceState?.getInt(STATE_SETUP_GENERATION)?.takeIf { it > 0 }
        val layout = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(32, 32, 32, 32)
            layoutParams = ViewGroup.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT)
        }
        statusView = TextView(this).apply {
            textSize = 18f
            text = if (invalidInputId) {
                "Maleicacid TV入力 設定\n不正な設定要求です。inputIdがないか、このTvInputServiceに属していません。"
            } else {
                "Maleicacid TV入力 設定\nチャンネルスキャンを開始できます。"
            }
        }
        scanButton = Button(this).apply {
            text = "チャンネルスキャン開始"
            isEnabled = !invalidInputId
            setOnClickListener {
                val resolved = inputId
                if (resolved.isNullOrBlank() || !isOwnInputId(resolved)) {
                    statusView.text = "不正な設定要求です。inputIdがないか、このTvInputServiceに属していません。"
                    setResult(RESULT_CANCELED)
                } else {
                    setupGeneration = ChannelScanManager.startIfIdle(this@SetupActivity, resolved)
                }
            }
        }
        cancelButton = Button(this).apply {
            text = "スキャン中止"
            isEnabled = false
            setOnClickListener { ChannelScanManager.cancel() }
        }
        layout.addView(statusView)
        layout.addView(scanButton)
        layout.addView(cancelButton)
        setContentView(layout)
        ChannelScanManager.addListener(this)
        drainDirectBootPending("SetupActivity.onCreate")
        if (invalidInputId) {
            setResult(RESULT_CANCELED)
        }
    }

    override fun onSaveInstanceState(outState: Bundle) {
        setupGeneration?.let { outState.putInt(STATE_SETUP_GENERATION, it) }
        super.onSaveInstanceState(outState)
    }

    private fun resolveInputId(): String? {
        val extras = intent?.extras
        return extras?.getString("android.media.tv.extra.INPUT_ID")
            ?: extras?.getString("android.media.tv.extra.input_id")
            ?: intent?.getStringExtra("inputId")
    }

    private fun isOwnInputId(candidate: String?): Boolean = TisInputIdResolver.isOwnInputId(this, candidate)

    override fun onScanStateChanged(state: ScanState) {
        runOnUiThread {
            when (state) {
                is ScanState.Idle -> {
                    scanButton.isEnabled = !invalidInputId
                    cancelButton.isEnabled = false
                    statusView.text = if (invalidInputId) "不正な設定要求です。inputIdがないか、このTvInputServiceに属していません。" else "チャンネルスキャンを開始できます。"
                }
                is ScanState.Running -> {
                    scanButton.isEnabled = false
                    cancelButton.isEnabled = true
                    statusView.text = when (state.purpose) {
                        ScanPurpose.SETUP_SCAN -> "チャンネルスキャン中です。"
                        ScanPurpose.BOOT_EPG_SYNC -> "起動後EPG同期を実行中です。"
                        ScanPurpose.BACKGROUND_MAINTENANCE -> "バックグラウンドチャンネル保守を実行中です。"
                    }
                }
                is ScanState.Completed -> {
                    scanButton.isEnabled = !invalidInputId
                    cancelButton.isEnabled = false
                    val diagnostics = state.result.diagnostics.joinToString("\n") { "${it.candidate.displayChannel}: ${it.message}" }
                    statusView.text = "${purposeLabel(state.purpose)} 完了\nスキャン数=${state.result.scanned} 公開数=${state.result.published}" +
                        if (diagnostics.isNotBlank()) "\n$diagnostics" else ""
                    if (shouldFinishSetupForStateForTest(state, setupGeneration, invalidInputId)) {
                        setResult(RESULT_OK)
                        finish()
                    }
                }
                is ScanState.Failed -> {
                    scanButton.isEnabled = !invalidInputId
                    cancelButton.isEnabled = false
                    statusView.text = "${purposeLabel(state.purpose)} 失敗: ${state.message}"
                    if (state.purpose == ScanPurpose.SETUP_SCAN && state.generation == setupGeneration) {
                        setResult(RESULT_CANCELED)
                    }
                }
                is ScanState.Cancelled -> {
                    scanButton.isEnabled = !invalidInputId
                    cancelButton.isEnabled = false
                    statusView.text = "${purposeLabel(state.purpose)} 中止"
                    if (state.purpose == ScanPurpose.SETUP_SCAN && state.generation == setupGeneration) {
                        setResult(RESULT_CANCELED)
                    }
                }
            }
        }
    }


    private fun purposeLabel(purpose: ScanPurpose): String = when (purpose) {
        ScanPurpose.SETUP_SCAN -> "設定スキャン"
        ScanPurpose.BOOT_EPG_SYNC -> "起動後EPG同期"
        ScanPurpose.BACKGROUND_MAINTENANCE -> "バックグラウンドチャンネル保守"
    }

    private fun drainDirectBootPending(source: String) {
        val state = DirectBootGuard.pendingStateForTest(applicationContext)
        if (state.pending) {
            statusView.text = "${statusView.text}\n起動後EPG同期は保留中です。設定画面の外で処理します。source=$source"
        }
    }

    companion object {
        private const val STATE_SETUP_GENERATION = "maleicacid.setupGeneration"
        fun scanStartAllowedForTest(candidateInputId: String?, isOwnInputId: Boolean): Boolean =
            !candidateInputId.isNullOrBlank() && isOwnInputId

        fun shouldFinishSetupForStateForTest(
            state: ScanState,
            activeSetupGeneration: Int?,
            invalidInputId: Boolean,
        ): Boolean = shouldFinishSetupForState(state, activeSetupGeneration, invalidInputId)

        private fun shouldFinishSetupForState(
            state: ScanState,
            activeSetupGeneration: Int?,
            invalidInputId: Boolean,
        ): Boolean = state is ScanState.Completed &&
            !invalidInputId &&
            activeSetupGeneration != null &&
            state.purpose == ScanPurpose.SETUP_SCAN &&
            state.generation == activeSetupGeneration &&
            state.result.published > 0
    }

    override fun onDestroy() {
        ChannelScanManager.removeListener(this)
        super.onDestroy()
    }
}
