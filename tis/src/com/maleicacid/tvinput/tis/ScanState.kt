package com.maleicacid.tvinput.tis

sealed class ScanState {
    object Idle : ScanState()
    data class Running(val startedAtMillis: Long) : ScanState()
    data class Completed(val result: ChannelScanController.ScanResult) : ScanState()
    data class Failed(val message: String) : ScanState()
    object Cancelled : ScanState()
}
