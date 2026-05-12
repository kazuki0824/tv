package com.maleicacid.tvinput.tis

enum class ScanPurpose {
    SETUP_SCAN,
    BOOT_EPG_SYNC,
    BACKGROUND_MAINTENANCE,
}

sealed class ScanState {
    object Idle : ScanState()
    data class Running(
        val startedAtMillis: Long,
        val generation: Int,
        val purpose: ScanPurpose,
    ) : ScanState()
    data class Completed(
        val result: ChannelScanController.ScanResult,
        val generation: Int,
        val purpose: ScanPurpose,
    ) : ScanState()
    data class Failed(
        val message: String,
        val generation: Int,
        val purpose: ScanPurpose,
    ) : ScanState()
    data class Cancelled(
        val generation: Int,
        val purpose: ScanPurpose,
    ) : ScanState()
}
