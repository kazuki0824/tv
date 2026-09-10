package android.media.tv.tuner;

import android.media.tv.tuner.filter.Filter;
import android.media.tv.tuner.filter.FilterCallback;
import java.util.concurrent.Executor;

// 資源失敗のホスト試験だけでnative初期化を省く。Kotlinは実Android APIに対してコンパイルする。
public class Tuner {
    public static Filter nextFilter;
    public static int getTunerVersion() { return 0x00030000; }
    public Filter openFilter(int type, int subtype, long bufferSize, Executor executor, FilterCallback callback) {
        return nextFilter;
    }
}
