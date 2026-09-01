#include "psi_parser.h"

#include <aidl/android/hardware/common/fmq/MQDescriptor.h>
#include <aidl/android/hardware/common/fmq/SynchronizedReadWrite.h>
#include <aidl/android/hardware/tv/tuner/BnFilterCallback.h>
#include <aidl/android/hardware/tv/tuner/BnFrontendCallback.h>
#include <aidl/android/hardware/tv/tuner/DemuxFilterMainType.h>
#include <aidl/android/hardware/tv/tuner/DemuxFilterSettings.h>
#include <aidl/android/hardware/tv/tuner/DemuxFilterSubType.h>
#include <aidl/android/hardware/tv/tuner/DemuxFilterType.h>
#include <aidl/android/hardware/tv/tuner/DemuxTsFilterSettings.h>
#include <aidl/android/hardware/tv/tuner/DemuxTsFilterSettingsFilterSettings.h>
#include <aidl/android/hardware/tv/tuner/DemuxTsFilterType.h>
#include <aidl/android/hardware/tv/tuner/FrontendEventType.h>
#include <aidl/android/hardware/tv/tuner/FrontendIsdbsCoderate.h>
#include <aidl/android/hardware/tv/tuner/FrontendIsdbsModulation.h>
#include <aidl/android/hardware/tv/tuner/FrontendIsdbsRolloff.h>
#include <aidl/android/hardware/tv/tuner/FrontendIsdbsSettings.h>
#include <aidl/android/hardware/tv/tuner/FrontendIsdbsStreamIdType.h>
#include <aidl/android/hardware/tv/tuner/FrontendIsdbtSettings.h>
#include <aidl/android/hardware/tv/tuner/FrontendSettings.h>
#include <aidl/android/hardware/tv/tuner/FrontendType.h>
#include <aidl/android/hardware/tv/tuner/IDemux.h>
#include <aidl/android/hardware/tv/tuner/IFilter.h>
#include <aidl/android/hardware/tv/tuner/IFrontend.h>
#include <aidl/android/hardware/tv/tuner/ITuner.h>
#include <android/binder_manager.h>
#include <android/binder_process.h>
#include <fmq/AidlMessageQueue.h>

#include <algorithm>
#include <cerrno>
#include <chrono>
#include <climits>
#include <condition_variable>
#include <cstdint>
#include <cstdlib>
#include <iostream>
#include <map>
#include <memory>
#include <mutex>
#include <optional>
#include <sstream>
#include <string>
#include <thread>
#include <vector>

using namespace aidl::android::hardware::tv::tuner;
using ::aidl::android::hardware::common::fmq::MQDescriptor;
using ::aidl::android::hardware::common::fmq::SynchronizedReadWrite;
using ::android::AidlMessageQueue;
using FilterMQ = AidlMessageQueue<int8_t, SynchronizedReadWrite>;
using FilterMQDesc = MQDescriptor<int8_t, SynchronizedReadWrite>;

namespace {

struct Args {
    std::string delivery_system;
    int64_t frequency_hz = 0;
    int timeout_ms = 5000;
    std::optional<uint16_t> service_id;
    int32_t stream_id = 0;
    int32_t stream_id_type = 0;
    int32_t symbol_rate = 0;
    int32_t modulation = 0;
    int32_t coderate = 0;
    int32_t rolloff = 0;
};

bool parse_i64(const std::string& text, int64_t* out) {
    char* end = nullptr;
    errno = 0;
    long long value = std::strtoll(text.c_str(), &end, 10);
    if (errno != 0 || end == text.c_str() || *end != '\0') return false;
    *out = static_cast<int64_t>(value);
    return true;
}

std::optional<Args> parse_args(int argc, char** argv) {
    std::map<std::string, std::string> values;
    for (int i = 1; i < argc; i += 2) {
        if (i + 1 >= argc || std::string(argv[i]).rfind("--", 0) != 0) return std::nullopt;
        values[argv[i]] = argv[i + 1];
    }
    Args args;
    if (!values.count("--delivery-system") || !values.count("--frequency-hz")) return std::nullopt;
    args.delivery_system = values["--delivery-system"];
    int64_t tmp = 0;
    if (!parse_i64(values["--frequency-hz"], &tmp) || tmp <= 0) return std::nullopt;
    args.frequency_hz = tmp;
    if (values.count("--timeout-ms")) {
        if (!parse_i64(values["--timeout-ms"], &tmp) || tmp <= 0 || tmp > 60000) return std::nullopt;
        args.timeout_ms = static_cast<int>(tmp);
    }
    if (values.count("--service-id")) {
        if (!parse_i64(values["--service-id"], &tmp) || tmp <= 0 || tmp > 0xffff) return std::nullopt;
        args.service_id = static_cast<uint16_t>(tmp);
    }
    auto assign_i32 = [&](const char* key, int32_t* out) -> bool {
        if (!values.count(key)) return true;
        if (!parse_i64(values[key], &tmp) || tmp < 0 || tmp > INT32_MAX) return false;
        *out = static_cast<int32_t>(tmp);
        return true;
    };
    if (!assign_i32("--stream-id", &args.stream_id) ||
        !assign_i32("--stream-id-type", &args.stream_id_type) ||
        !assign_i32("--symbol-rate", &args.symbol_rate) ||
        !assign_i32("--modulation", &args.modulation) ||
        !assign_i32("--coderate", &args.coderate) ||
        !assign_i32("--rolloff", &args.rolloff)) return std::nullopt;
    if (args.delivery_system != "ISDBT" && args.delivery_system != "ISDBS") return std::nullopt;
    return args;
}

int fail(const std::string& message) {
    std::cerr << "error: " << message << "\n";
    return 2;
}

class FrontendCallback final : public BnFrontendCallback {
  public:
    ::ndk::ScopedAStatus onEvent(FrontendEventType event) override {
        std::lock_guard<std::mutex> lock(mutex_);
        if (event == FrontendEventType::LOCKED) locked_ = true;
        if (event == FrontendEventType::NO_SIGNAL || event == FrontendEventType::LOST_LOCK) terminal_failure_ = true;
        cv_.notify_all();
        return ::ndk::ScopedAStatus::ok();
    }
    ::ndk::ScopedAStatus onScanMessage(FrontendScanMessageType, const FrontendScanMessage&) override {
        return ::ndk::ScopedAStatus::ok();
    }
    bool wait_for_lock(int timeout_ms) {
        std::unique_lock<std::mutex> lock(mutex_);
        cv_.wait_for(lock, std::chrono::milliseconds(timeout_ms), [&] { return locked_ || terminal_failure_; });
        return locked_;
    }
  private:
    std::mutex mutex_;
    std::condition_variable cv_;
    bool locked_ = false;
    bool terminal_failure_ = false;
};

class FilterCallback final : public BnFilterCallback {
  public:
    ::ndk::ScopedAStatus onFilterEvent(const std::vector<DemuxFilterEvent>&) override {
        return ::ndk::ScopedAStatus::ok();
    }
    ::ndk::ScopedAStatus onFilterStatus(DemuxFilterStatus) override {
        return ::ndk::ScopedAStatus::ok();
    }
};

FrontendType requested_type(const Args& args) {
    return args.delivery_system == "ISDBT" ? FrontendType::ISDBT : FrontendType::ISDBS;
}

FrontendSettings make_settings(const Args& args) {
    FrontendSettings settings;
    if (args.delivery_system == "ISDBT") {
        FrontendIsdbtSettings isdbt{};
        isdbt.frequency = args.frequency_hz;
        settings.set<FrontendSettings::Tag::isdbt>(isdbt);
    } else {
        FrontendIsdbsSettings isdbs{};
        isdbs.frequency = args.frequency_hz;
        isdbs.streamId = args.stream_id;
        isdbs.streamIdType = static_cast<FrontendIsdbsStreamIdType>(args.stream_id_type);
        isdbs.symbolRate = args.symbol_rate;
        isdbs.modulation = static_cast<FrontendIsdbsModulation>(args.modulation);
        isdbs.coderate = static_cast<FrontendIsdbsCoderate>(args.coderate);
        isdbs.rolloff = static_cast<FrontendIsdbsRolloff>(args.rolloff);
        settings.set<FrontendSettings::Tag::isdbs>(isdbs);
    }
    return settings;
}

std::optional<int32_t> find_frontend(const std::shared_ptr<ITuner>& tuner, FrontendType type) {
    std::vector<int32_t> ids;
    if (!tuner->getFrontendIds(&ids).isOk()) return std::nullopt;
    for (int32_t id : ids) {
        FrontendInfo info;
        if (tuner->getFrontendInfo(id, &info).isOk() && info.type == type) return id;
    }
    return std::nullopt;
}

struct FilterSession {
    FilterSession() = default;
    FilterSession(std::shared_ptr<IFilter> f, std::unique_ptr<FilterMQ> q)
        : filter(std::move(f)), queue(std::move(q)) {}
    FilterSession(FilterSession&&) noexcept = default;
    FilterSession& operator=(FilterSession&&) noexcept = default;
    FilterSession(const FilterSession&) = delete;
    FilterSession& operator=(const FilterSession&) = delete;
    std::shared_ptr<IFilter> filter;
    std::unique_ptr<FilterMQ> queue;
    ~FilterSession() {
        if (filter) {
            filter->stop();
            filter->close();
        }
    }
};

std::optional<FilterSession> open_ts_filter(const std::shared_ptr<IDemux>& demux, uint16_t pid) {
    DemuxFilterType type{};
    type.mainType = DemuxFilterMainType::TS;
    type.subType.set<DemuxFilterSubType::Tag::tsFilterType>(DemuxTsFilterType::TS);
    auto callback = ::ndk::SharedRefBase::make<FilterCallback>();
    std::shared_ptr<IFilter> filter;
    if (!demux->openFilter(type, 1024 * 1024, callback, &filter).isOk() || !filter) return std::nullopt;
    DemuxTsFilterSettings ts{};
    ts.tpid = pid;
    ts.filterSettings.set<DemuxTsFilterSettingsFilterSettings::Tag::noinit>(true);
    DemuxFilterSettings settings;
    settings.set<DemuxFilterSettings::Tag::ts>(ts);
    if (!filter->configure(settings).isOk()) { filter->close(); return std::nullopt; }
    FilterMQDesc desc;
    if (!filter->getQueueDesc(&desc).isOk()) { filter->close(); return std::nullopt; }
    auto queue = std::make_unique<FilterMQ>(desc, true);
    if (!queue || !queue->isValid()) { filter->close(); return std::nullopt; }
    if (!filter->start().isOk()) { filter->close(); return std::nullopt; }
    return FilterSession{filter, std::move(queue)};
}

std::optional<std::vector<uint8_t>> read_section(FilterSession& session, uint8_t table_id, int timeout_ms) {
    tuner_hal2_vts::SectionAssembler assembler(table_id);
    const auto deadline = std::chrono::steady_clock::now() + std::chrono::milliseconds(timeout_ms);
    std::vector<int8_t> raw;
    std::vector<uint8_t> carry;
    while (std::chrono::steady_clock::now() < deadline) {
        const size_t available = session.queue->availableToRead();
        if (available == 0) {
            std::this_thread::sleep_for(std::chrono::milliseconds(10));
            continue;
        }
        raw.resize(available);
        if (!session.queue->read(raw.data(), raw.size())) return std::nullopt;
        carry.insert(carry.end(), raw.begin(), raw.end());
        size_t off = 0;
        while (carry.size() - off >= 188) {
            if (carry[off] != 0x47) {
                auto it = std::find(carry.begin() + static_cast<std::ptrdiff_t>(off + 1), carry.end(), static_cast<uint8_t>(0x47));
                if (it == carry.end()) { carry.clear(); off = 0; break; }
                off = static_cast<size_t>(it - carry.begin());
                continue;
            }
            if (auto section = assembler.push_ts_packet(carry.data() + off, 188)) return section;
            off += 188;
        }
        if (off != 0 && off <= carry.size()) {
            carry.erase(carry.begin(), carry.begin() + static_cast<std::ptrdiff_t>(off));
        }
    }
    return std::nullopt;
}

std::string json_result(const Args& args, uint16_t service_id, uint16_t pmt_pid,
                        const tuner_hal2_vts::PmtInfo& pmt) {
    std::optional<uint16_t> video;
    std::optional<uint16_t> audio;
    std::ostringstream elementary;
    elementary << "[";
    for (size_t i = 0; i < pmt.streams.size(); ++i) {
        if (i) elementary << ",";
        elementary << pmt.streams[i].pid;
        if (!video && tuner_hal2_vts::is_video_stream_type(pmt.streams[i].stream_type)) video = pmt.streams[i].pid;
        if (!audio && tuner_hal2_vts::is_audio_stream_type(pmt.streams[i].stream_type)) audio = pmt.streams[i].pid;
    }
    elementary << "]";
    std::ostringstream out;
    out << "{\"frequency_hz\":" << args.frequency_hz
        << ",\"service_id\":" << service_id
        << ",\"pmt_pid\":" << pmt_pid
        << ",\"video_pid\":";
    if (video) out << *video; else out << "null";
    out << ",\"audio_pid\":";
    if (audio) out << *audio; else out << "null";
    out << ",\"elementary_pids\":" << elementary.str() << "}";
    return out.str();
}

}  // namespace

int main(int argc, char** argv) {
    auto args_opt = parse_args(argc, argv);
    if (!args_opt) return fail("invalid arguments");
    const Args& args = *args_opt;
    ABinderProcess_setThreadPoolMaxThreadCount(2);
    ABinderProcess_startThreadPool();
    const std::string service_name = std::string(ITuner::descriptor) + "/default";
    ::ndk::SpAIBinder binder(AServiceManager_waitForService(service_name.c_str()));
    auto tuner = ITuner::fromBinder(binder);
    if (!tuner) return fail("Tuner AIDL service is unavailable");
    auto frontend_id = find_frontend(tuner, requested_type(args));
    if (!frontend_id) return fail("no matching frontend");
    std::shared_ptr<IFrontend> frontend;
    if (!tuner->openFrontendById(*frontend_id, &frontend).isOk() || !frontend) return fail("openFrontendById failed");
    auto frontend_callback = ::ndk::SharedRefBase::make<FrontendCallback>();
    if (!frontend->setCallback(frontend_callback).isOk()) { frontend->close(); return fail("setCallback failed"); }
    if (!frontend->tune(make_settings(args)).isOk()) { frontend->close(); return fail("tune failed"); }
    if (!frontend_callback->wait_for_lock(args.timeout_ms)) {
        frontend->stopTune(); frontend->close(); return fail("frontend did not reach LOCKED");
    }

    std::vector<int32_t> demux_ids;
    if (!tuner->getDemuxIds(&demux_ids).isOk() || demux_ids.empty()) {
        frontend->stopTune(); frontend->close(); return fail("no demux available");
    }
    std::shared_ptr<IDemux> demux;
    if (!tuner->openDemuxById(demux_ids.front(), &demux).isOk() || !demux) {
        frontend->stopTune(); frontend->close(); return fail("openDemuxById failed");
    }
    if (!demux->setFrontendDataSource(*frontend_id).isOk()) {
        demux->close(); frontend->stopTune(); frontend->close(); return fail("setFrontendDataSource failed");
    }

    auto pat_filter = open_ts_filter(demux, 0);
    if (!pat_filter) { demux->close(); frontend->stopTune(); frontend->close(); return fail("PAT filter open failed"); }
    auto pat_section = read_section(*pat_filter, 0x00, args.timeout_ms);
    if (!pat_section) { demux->close(); frontend->stopTune(); frontend->close(); return fail("PAT not received"); }
    auto programs = tuner_hal2_vts::parse_pat(*pat_section);
    if (!programs || programs->empty()) { demux->close(); frontend->stopTune(); frontend->close(); return fail("PAT has no service programs"); }
    tuner_hal2_vts::ProgramMap selected{};
    if (args.service_id) {
        auto it = std::find_if(programs->begin(), programs->end(), [&](const auto& p) { return p.service_id == *args.service_id; });
        if (it == programs->end()) { demux->close(); frontend->stopTune(); frontend->close(); return fail("requested service ID is absent from PAT"); }
        selected = *it;
    } else {
        if (programs->size() != 1) {
            std::ostringstream list;
            list << "multiple services in PAT; specify --service-id (";
            for (size_t i = 0; i < programs->size(); ++i) { if (i) list << ","; list << (*programs)[i].service_id; }
            list << ")";
            demux->close(); frontend->stopTune(); frontend->close(); return fail(list.str());
        }
        selected = programs->front();
    }
    pat_filter.reset();
    auto pmt_filter = open_ts_filter(demux, selected.pmt_pid);
    if (!pmt_filter) { demux->close(); frontend->stopTune(); frontend->close(); return fail("PMT filter open failed"); }
    auto pmt_section = read_section(*pmt_filter, 0x02, args.timeout_ms);
    if (!pmt_section) { demux->close(); frontend->stopTune(); frontend->close(); return fail("PMT not received"); }
    auto pmt = tuner_hal2_vts::parse_pmt(*pmt_section);
    if (!pmt || pmt->service_id != selected.service_id || pmt->streams.empty()) {
        demux->close(); frontend->stopTune(); frontend->close(); return fail("PMT is invalid or empty");
    }

    std::cout << json_result(args, selected.service_id, selected.pmt_pid, *pmt) << "\n";
    pmt_filter.reset();
    demux->close();
    frontend->stopTune();
    frontend->close();
    return 0;
}
