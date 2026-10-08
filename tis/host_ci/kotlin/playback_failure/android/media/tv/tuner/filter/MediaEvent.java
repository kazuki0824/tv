package android.media.tv.tuner.filter;

import android.media.MediaCodec;

// Kotlinの実API型照合後だけ、途中処理失敗と残余入力の解放失敗を注入する。
public class MediaEvent extends FilterEvent {
    public boolean rejectRead;
    public boolean rejectRelease;
    public int releases;
    public long getOffset() {
        if (rejectRead) throw new IllegalStateException("injected MediaEvent read");
        return -1L;
    }
    public long getDataLength() { return 1L; }
    public boolean isSecureMemory() { return false; }
    public MediaCodec.LinearBlock getLinearBlock() { return null; }
    public long getPts() { return 0L; }
    public void release() {
        releases++;
        if (rejectRelease) throw new IllegalStateException("injected MediaEvent release");
    }
}
