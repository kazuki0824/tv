#include "cas_service_boundary.h"

#include <aidl/android/hardware/cas/BnMediaCasService.h>
#include <algorithm>
#include <utility>

namespace maleicacid::cas {
namespace {
using namespace aidl::android::hardware::cas;
constexpr int32_t kClearKeySystemId = 0xF6D8;

class ServiceBoundary final : public BnMediaCasService {
public:
    ServiceBoundary(std::shared_ptr<Service> product, std::shared_ptr<Service> reference,
                    std::vector<AidlCasPluginDescriptor> descriptors)
        : product_(std::move(product)), reference_(std::move(reference)),
          descriptors_(std::move(descriptors)) {}

    ndk::ScopedAStatus enumeratePlugins(std::vector<AidlCasPluginDescriptor>* result) override {
        *result = descriptors_;
        return ndk::ScopedAStatus::ok();
    }

    ndk::ScopedAStatus isSystemIdSupported(int32_t id, bool* result) override {
        *result = supports(id);
        return ndk::ScopedAStatus::ok();
    }

    ndk::ScopedAStatus isDescramblerSupported(int32_t id, bool* result) override {
        *result = id == kClearKeySystemId;
        return ndk::ScopedAStatus::ok();
    }

    ndk::ScopedAStatus createPlugin(int32_t id, const std::shared_ptr<ICasListener>& listener,
                                    std::shared_ptr<ICas>* result) override {
        result->reset();
        // V1 AIDLのRust Strong戻り値では表現できない成功/nullをNDK境界で返す。
        if (!supports(id)) return ndk::ScopedAStatus::ok();
        return (id == kClearKeySystemId ? reference_ : product_)->createPlugin(id, listener, result);
    }

    ndk::ScopedAStatus createDescrambler(int32_t id, std::shared_ptr<IDescrambler>* result) override {
        result->reset();
        if (id != kClearKeySystemId) return ndk::ScopedAStatus::ok();
        return reference_->createDescrambler(id, result);
    }

private:
    bool supports(int32_t id) const {
        return std::any_of(descriptors_.begin(), descriptors_.end(), [id](const auto& entry) {
            return entry.caSystemId == id;
        });
    }

    const std::shared_ptr<Service> product_;
    const std::shared_ptr<Service> reference_;
    const std::vector<AidlCasPluginDescriptor> descriptors_;
};
}  // namespace

std::shared_ptr<Service> make_service_boundary(
        std::shared_ptr<Service> product, std::shared_ptr<Service> reference) {
    if (!product || !reference) return nullptr;
    std::vector<AidlCasPluginDescriptor> descriptors;
    std::vector<AidlCasPluginDescriptor> reference_descriptors;
    if (!product->enumeratePlugins(&descriptors).isOk() ||
        !reference->enumeratePlugins(&reference_descriptors).isOk()) return nullptr;
    bool plugin = false;
    bool descrambler = false;
    if (!reference->isSystemIdSupported(kClearKeySystemId, &plugin).isOk() || !plugin ||
        !reference->isDescramblerSupported(kClearKeySystemId, &descrambler).isOk() || !descrambler)
        return nullptr;
    const auto count = std::count_if(reference_descriptors.begin(), reference_descriptors.end(),
                                   [](const auto& entry) { return entry.caSystemId == kClearKeySystemId; });
    if (count != 1) return nullptr;
    const auto clear_key = std::find_if(reference_descriptors.begin(), reference_descriptors.end(),
                                      [](const auto& entry) { return entry.caSystemId == kClearKeySystemId; });
    descriptors.push_back(*clear_key);
    std::sort(descriptors.begin(), descriptors.end(), [](const auto& left, const auto& right) {
        return left.caSystemId < right.caSystemId;
    });
    if (std::adjacent_find(descriptors.begin(), descriptors.end(), [](const auto& left, const auto& right) {
            return left.caSystemId == right.caSystemId;
        }) != descriptors.end()) return nullptr;
    return ndk::SharedRefBase::make<ServiceBoundary>(std::move(product), std::move(reference),
                                                   std::move(descriptors));
}
}  // namespace maleicacid::cas
