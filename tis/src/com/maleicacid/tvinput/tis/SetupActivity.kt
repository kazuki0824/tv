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
        val layout = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(32, 32, 32, 32)
            layoutParams = ViewGroup.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT)
        }
        statusView = TextView(this).apply {
            textSize = 18f
            text = if (invalidInputId) {
                "Maleicacid TV Input setup\n不正なsetup intentです。inputIdがないか、このTvInputServiceに属していません。"
            } else {
                "Maleicacid TV Input setup\nチャンネルスキャンを開始できます。"
            }
        }
        scanButton = Button(this).apply {
            text = "チャンネルスキャン開始"
            isEnabled = !invalidInputId
            setOnClickListener {
                val resolved = inputId
                if (resolved.isNullOrBlank() || !isOwnInputId(resolved)) {
                    statusView.text = "不正なsetup intentです。inputIdがないか、このTvInputServiceに属していません。"
                    setResult(RESULT_CANCELED)
                } else {
                    setupGeneration = ChannelScanManager.startIfIdle(this@SetupActivity, resolved)
                }
            }
        }
        cancelButton = Button(this).apply {
            text = "Cancel scan"
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
                    statusView.text = if (invalidInputId) "不正なsetup intentです。inputIdがないか、このTvInputServiceに属していません。" else "チャンネルスキャンを開始できます。"
                }
                is ScanState.Running -> {
                    scanButton.isEnabled = false
                    cancelButton.isEnabled = true
                    statusView.text = when (state.purpose) {
                        ScanPurpose.SETUP_SCAN -> "Scanning channels..."
                        ScanPurpose.BOOT_EPG_SYNC -> "Boot EPG sync is running in the background."
                        ScanPurpose.BACKGROUND_MAINTENANCE -> "Background channel maintenance is running."
                    }
                }
                is ScanState.Completed -> {
                    scanButton.isEnabled = !invalidInputId
                    cancelButton.isEnabled = false
                    val diagnostics = state.result.diagnostics.joinToString("\n") { "${it.candidate.displayChannel}: ${it.message}" }
                    statusView.text = "${state.purpose} complete\nscanned=${state.result.scanned} published=${state.result.published}" +
                        if (diagnostics.isNotBlank()) "\n$diagnostics" else ""
                    if (shouldFinishSetupForStateForTest(state, setupGeneration, invalidInputId)) {
                        setResult(RESULT_OK)
                        finish()
                    }
                }
                is ScanState.Failed -> {
                    scanButton.isEnabled = !invalidInputId
                    cancelButton.isEnabled = false
                    statusView.text = "${state.purpose} failed: ${state.message}"
                    if (state.purpose == ScanPurpose.SETUP_SCAN && state.generation == setupGeneration) {
                        setResult(RESULT_CANCELED)
                    }
                }
                is ScanState.Cancelled -> {
                    scanButton.isEnabled = !invalidInputId
                    cancelButton.isEnabled = false
                    statusView.text = "${state.purpose} cancelled"
                    if (state.purpose == ScanPurpose.SETUP_SCAN && state.generation == setupGeneration) {
                        setResult(RESULT_CANCELED)
                    }
                }
            }
        }
    }

    private fun drainDirectBootPending(source: String) {
        val state = DirectBootGuard.pendingStateForTest(applicationContext)
        if (state.pending) {
            statusView.text = "${statusView.text}\nBoot EPG sync is pending and will be drained outside setup. source=$source"
        }
    }

    companion object {
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
