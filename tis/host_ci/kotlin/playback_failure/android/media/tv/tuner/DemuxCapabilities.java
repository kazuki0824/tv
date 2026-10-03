package android.media.tv.tuner;

// TunerController の本番 capacity preflight を通すホスト専用境界。
public final class DemuxCapabilities {
    private final int sectionFilterCount;

    public DemuxCapabilities(int sectionFilterCount) {
        this.sectionFilterCount = sectionFilterCount;
    }

    public int getSectionFilterCount() {
        return sectionFilterCount;
    }
}
