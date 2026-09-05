package com.maleicacid.tvinput.tis

/** ライブ再生の純粋判定。Session lifecycleとAndroid資源はMaleicacidLiveSessionが所有する。 */
object PlaybackPolicy {
    private const val SERVICE_TYPE_DIGITAL_AUDIO = 0x02

    fun isAudioOnlyService(serviceType: Int?): Boolean = serviceType == SERVICE_TYPE_DIGITAL_AUDIO

    fun shouldRejectSelection(
        serviceType: Int,
        selection: TunerController.AvStreamSelection,
    ): Boolean = if (isAudioOnlyService(serviceType)) selection.audio == null else selection.video == null

    fun updateUnblockStateForProgramChange(
        previousIdentityKey: String?,
        nextIdentityKey: String?,
        unblockedContentKeys: MutableSet<String>,
    ): String? {
        if (previousIdentityKey != nextIdentityKey) unblockedContentKeys.clear()
        return nextIdentityKey
    }
}
