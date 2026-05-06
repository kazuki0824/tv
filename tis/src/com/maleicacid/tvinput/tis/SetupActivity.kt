package com.maleicacid.tvinput.tis

import android.app.Activity
import android.os.Bundle
import android.view.ViewGroup
import android.widget.Button
import android.widget.LinearLayout
import android.widget.TextView
import com.maleicacid.tvinput.common.AppIds

class SetupActivity : Activity(), ChannelScanManager.Listener {
    private lateinit var statusView: TextView
    private lateinit var scanButton: Button
    private lateinit var cancelButton: Button

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val layout = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(32, 32, 32, 32)
            layoutParams = ViewGroup.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT)
        }
        statusView = TextView(this).apply {
            textSize = 18f
            text = "Maleicacid TV Input setup\nReady to scan channels."
        }
        scanButton = Button(this).apply {
            text = "Start channel scan"
            setOnClickListener { ChannelScanManager.startIfIdle(this@SetupActivity, resolveInputId()) }
        }
        cancelButton = Button(this).apply {
            text = "Cancel scan"
            setOnClickListener { ChannelScanManager.cancel() }
        }
        layout.addView(statusView)
        layout.addView(scanButton)
        layout.addView(cancelButton)
        setContentView(layout)
        ChannelScanManager.addListener(this)
        ChannelScanManager.startIfIdle(this, resolveInputId())
    }

    private fun resolveInputId(): String {
        val extras = intent?.extras
        return extras?.getString("android.media.tv.extra.INPUT_ID")
            ?: extras?.getString("android.media.tv.extra.input_id")
            ?: intent?.getStringExtra("inputId")
            ?: AppIds.TV_INPUT_SERVICE
    }

    override fun onScanStateChanged(state: ScanState) {
        runOnUiThread {
            when (state) {
                is ScanState.Idle -> {
                    scanButton.isEnabled = true
                    cancelButton.isEnabled = false
                    statusView.text = "Ready to scan channels."
                }
                is ScanState.Running -> {
                    scanButton.isEnabled = false
                    cancelButton.isEnabled = true
                    statusView.text = "Scanning channels..."
                }
                is ScanState.Completed -> {
                    scanButton.isEnabled = true
                    cancelButton.isEnabled = false
                    val diagnostics = state.result.diagnostics.joinToString("\n") { "${it.candidate.displayChannel}: ${it.message}" }
                    statusView.text = "Scan complete\nscanned=${state.result.scanned} published=${state.result.published}" +
                        if (diagnostics.isNotBlank()) "\n$diagnostics" else ""
                    if (state.result.published > 0) {
                        setResult(RESULT_OK)
                        finish()
                    }
                }
                is ScanState.Failed -> {
                    scanButton.isEnabled = true
                    cancelButton.isEnabled = false
                    statusView.text = "Scan failed: ${state.message}"
                    setResult(RESULT_CANCELED)
                }
                is ScanState.Cancelled -> {
                    scanButton.isEnabled = true
                    cancelButton.isEnabled = false
                    statusView.text = "Scan cancelled"
                    setResult(RESULT_CANCELED)
                }
            }
        }
    }

    override fun onDestroy() {
        ChannelScanManager.removeListener(this)
        super.onDestroy()
    }
}
