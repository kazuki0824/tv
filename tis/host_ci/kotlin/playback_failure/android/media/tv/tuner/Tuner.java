package android.media.tv.tuner;

import android.content.Context;
import android.media.tv.tuner.frontend.FrontendSettings;
import android.media.tv.tuner.filter.Filter;
import android.media.tv.tuner.filter.FilterCallback;
import java.util.concurrent.Executor;

// 資源失敗のホスト試験だけでnative初期化を省く。Kotlinは実Android APIに対してコンパイルする。
public class Tuner {
    public static final byte[] VOID_KEYTOKEN = new byte[] {0};
    public static Filter nextFilter;
    public static FilterCallback lastCallback;
    public static int openFilterCalls;
    public static int sectionFilterCount = 16;

    // 実ControllerがTuner不在から正規に再試行する経路だけをhost fixtureで受ける。
    public Tuner(Context context, String sessionId, int useCase) {}
    public interface OnResourceLostListener { void onResourceLost(Tuner tuner); }
    public void setResourceLostListener(Executor executor, OnResourceLostListener listener) {}
    public int tune(FrontendSettings settings) { return 1; } // RESULT_UNAVAILABLE
    public void clearOnTuneEventListener() {}

    public static int getTunerVersion() { return 0x00030000; }

    public DemuxCapabilities getDemuxCapabilities() {
        return new DemuxCapabilities(sectionFilterCount);
    }

    public Filter openFilter(int type, int subtype, long bufferSize, Executor executor, FilterCallback callback) {
        openFilterCalls++;
        if (nextFilter != null) nextFilter.callback = callback;
        lastCallback = callback;
        if (nextFilter != null) nextFilter.callback = callback;
        return nextFilter;
    }
}
