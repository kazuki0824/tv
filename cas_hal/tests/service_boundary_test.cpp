#include "cas_service_boundary.h"

#include <aidl/android/hardware/cas/BnMediaCasService.h>
#include <gtest/gtest.h>

namespace maleicacid::cas {
namespace {
using namespace aidl::android::hardware::cas;

class RecordingService final : public BnMediaCasService {
public:
    std::vector<AidlCasPluginDescriptor> descriptors;
    int32_t error = 8;
    int plugins = 0;
    int descramblers = 0;

    ndk::ScopedAStatus enumeratePlugins(std::vector<AidlCasPluginDescriptor>* result) override {
        *result = descriptors;
        return ndk::ScopedAStatus::ok();
    }
    ndk::ScopedAStatus isSystemIdSupported(int32_t id, bool* result) override {
        *result = false;
        for (const auto& entry : descriptors) *result |= entry.caSystemId == id;
        return ndk::ScopedAStatus::ok();
    }
    ndk::ScopedAStatus isDescramblerSupported(int32_t id, bool* result) override {
        return isSystemIdSupported(id, result);
    }
    ndk::ScopedAStatus createPlugin(int32_t, const std::shared_ptr<ICasListener>&,
                                    std::shared_ptr<ICas>*) override {
        ++plugins;
        return ndk::ScopedAStatus::fromServiceSpecificError(error);
    }
    ndk::ScopedAStatus createDescrambler(int32_t, std::shared_ptr<IDescrambler>*) override {
        ++descramblers;
        return ndk::ScopedAStatus::fromServiceSpecificError(error);
    }
};

AidlCasPluginDescriptor descriptor(int32_t id) {
    AidlCasPluginDescriptor result;
    result.caSystemId = id;
    result.name = id == 0xF6D8 ? "Clear Key CAS" : "Maleicacid B25 CAS";
    return result;
}

TEST(ServiceBoundary, EmptyProductProfileStillExposesClearKey) {
    auto product = ndk::SharedRefBase::make<RecordingService>();
    auto reference = ndk::SharedRefBase::make<RecordingService>();
    reference->descriptors = {descriptor(0xF6D8)};
    auto service = make_service_boundary(product, reference);
    ASSERT_NE(service, nullptr);
    std::vector<AidlCasPluginDescriptor> descriptors;
    ASSERT_TRUE(service->enumeratePlugins(&descriptors).isOk());
    ASSERT_EQ(descriptors.size(), 1u);
    EXPECT_EQ(descriptors.front().caSystemId, 0xF6D8);
    bool supported = false;
    EXPECT_TRUE(service->isSystemIdSupported(0xF6D8, &supported).isOk());
    EXPECT_TRUE(supported);
    EXPECT_TRUE(service->isDescramblerSupported(0xF6D8, &supported).isOk());
    EXPECT_TRUE(supported);
    std::shared_ptr<ICas> plugin;
    EXPECT_EQ(service->createPlugin(0xF6D8, nullptr, &plugin).getServiceSpecificError(), 8);
    std::shared_ptr<IDescrambler> descrambler;
    EXPECT_EQ(service->createDescrambler(0xF6D8, &descrambler).getServiceSpecificError(), 8);
    EXPECT_EQ(reference->plugins, 1);
    EXPECT_EQ(reference->descramblers, 1);
    EXPECT_EQ(product->plugins, 0);
}

TEST(ServiceBoundary, UnsupportedFactoriesReturnSuccessfulNullWithoutDelegation) {
    auto product = ndk::SharedRefBase::make<RecordingService>();
    auto reference = ndk::SharedRefBase::make<RecordingService>();
    product->descriptors = {descriptor(5)};
    product->error = 6;
    reference->descriptors = {descriptor(0xF6D8)};
    auto service = make_service_boundary(product, reference);
    ASSERT_NE(service, nullptr);
    std::shared_ptr<ICas> plugin;
    std::shared_ptr<IDescrambler> descrambler;
    bool supported = true;
    EXPECT_TRUE(service->isSystemIdSupported(-1, &supported).isOk());
    EXPECT_FALSE(supported);
    EXPECT_TRUE(service->createPlugin(-1, nullptr, &plugin).isOk());
    EXPECT_EQ(plugin, nullptr);
    for (int32_t id : {-1, 1, 5}) {
        EXPECT_TRUE(service->isDescramblerSupported(id, &supported).isOk());
        EXPECT_FALSE(supported);
        EXPECT_TRUE(service->createDescrambler(id, &descrambler).isOk());
        EXPECT_EQ(descrambler, nullptr);
    }
    EXPECT_EQ(service->createPlugin(5, nullptr, &plugin).getServiceSpecificError(), 6);
    EXPECT_EQ(product->plugins, 1);
    EXPECT_EQ(product->descramblers, 0);
    EXPECT_EQ(reference->plugins, 0);
    EXPECT_EQ(reference->descramblers, 0);
}

TEST(ServiceBoundary, MissingOrDuplicateClearKeyPreventsRegistration) {
    auto product = ndk::SharedRefBase::make<RecordingService>();
    auto reference = ndk::SharedRefBase::make<RecordingService>();
    EXPECT_EQ(make_service_boundary(product, reference), nullptr);
    reference->descriptors = {descriptor(0xF6D8), descriptor(0xF6D8)};
    EXPECT_EQ(make_service_boundary(product, reference), nullptr);
    reference->descriptors = {descriptor(0xF6D8)};
    product->descriptors = {descriptor(0xF6D8)};
    EXPECT_EQ(make_service_boundary(product, reference), nullptr);
}
}  // namespace
}  // namespace maleicacid::cas
