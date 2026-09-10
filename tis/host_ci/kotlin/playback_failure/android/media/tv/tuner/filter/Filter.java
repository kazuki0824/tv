package android.media.tv.tuner.filter;

// 本番のFilter構築経路へ、戻り値失敗・例外・解放失敗を注入するホスト専用境界。
public class Filter implements AutoCloseable {
    public int configureFailure;
    public int startFailure;
    public boolean rejectClose;
    public int configurations;
    public int starts;
    public int closes;

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
        closes++;
        if (rejectClose) throw new IllegalStateException("injected filter close");
    }
}
