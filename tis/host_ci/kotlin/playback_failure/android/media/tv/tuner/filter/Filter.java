package android.media.tv.tuner.filter;

// 本番のFilter構築経路へ、戻り値失敗・例外・解放失敗を注入するホスト専用境界。
public class Filter implements AutoCloseable {
    public int configureFailure;
    public int startFailure;
    public boolean rejectClose;
    public int configurations;
    public int starts;
    public int closes;
    public FilterCallback callback;
    public byte[][] sectionPayloads;
    public int reads;
    public boolean deliver(FilterEvent[] events) {
        if (callback == null) return false;
        callback.onFilterEvent(this, events);
        return true;
    }
    public int read(byte[] buffer, long offset, long size) {
        byte[] payload = sectionPayloads[reads++];
        System.arraycopy(payload, 0, buffer, (int) offset, payload.length);
        return payload.length;
    }

    public int configure(FilterConfiguration configuration) {
        configurations++;
        if (configureFailure == 2) throw new IllegalStateException("injected configure");
        return configureFailure;
    }
    public int start() {
        starts++;
        if (startFailure == 2) throw new IllegalStateException("injected start");
        return startFailure;
    }
    public int stop() { return 0; }
    @Override public void close() {
        // AOSP同様、native closeの成否より先に配送を解除する。
        callback = null;
        closes++;
        if (rejectClose) throw new IllegalStateException("injected filter close");
    }
}
