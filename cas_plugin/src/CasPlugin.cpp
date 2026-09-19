// SPDX-License-Identifier: GPL-3.0-only
#include <media/cas/CasAPI.h>
#include <media/stagefright/MediaErrors.h>

#include "KeyServer.h"
#include "YakisobaBackend.h"

#include <algorithm>
#include <map>
#include <new>

namespace maleicacid::cas {
namespace {

android::status_t status(Result result) {
    using namespace android;
    switch (result) {
        case Result::Ok: return OK;
        case Result::BadValue: return BAD_VALUE;
        case Result::Unsupported: return ERROR_CAS_CANNOT_HANDLE;
        case Result::NotProvisioned: return ERROR_CAS_NOT_PROVISIONED;
        case Result::NoLicense: return ERROR_CAS_NO_LICENSE;
        case Result::SessionClosed: return ERROR_CAS_SESSION_NOT_OPENED;
        case Result::Busy: return ERROR_CAS_RESOURCE_BUSY;
        case Result::Decrypt: return ERROR_CAS_DECRYPT;
        case Result::Revoked: return ERROR_CAS_DEVICE_REVOKED;
        // Android 15 の CasImpl/TypeConvert が INVALID_STATE へ写像する native status。
        case Result::InvalidState: return ERROR_CAS_TAMPER_DETECTED;
        case Result::Unknown: return ERROR_CAS_UNKNOWN;
    }
    return ERROR_CAS_UNKNOWN;
}

template <typename F> android::status_t guarded(F&& operation) {
    try { return status(operation()); }
    catch (const std::bad_alloc&) { return android::ERROR_CAS_RESOURCE_BUSY; }
    catch (...) { return android::ERROR_CAS_UNKNOWN; }
}

class B25Plugin final : public android::CasPlugin {
public:
    B25Plugin(void* appData, std::shared_ptr<KeyServer> server)
        : appData_(appData), server_(std::move(server)) {}

    ~B25Plugin() override {
        std::lock_guard lock(mutex_);
        releasing_ = true;
        callback_ = nullptr;
        for (const auto& [id, session] : sessions_) KeyRegistry::instance().close(session.slot);
        sessions_.clear();
    }

    android::status_t setStatusCallback(android::CasPluginStatusCallback callback) override {
        return guarded([&] {
            {
                std::lock_guard lock(mutex_);
                if (releasing_) return Result::InvalidState;
                callback_ = callback;
            }
            if (callback) callback(appData_, 1, kSessionCapacity);
            return Result::Ok;
        });
    }

    android::status_t setPrivateData(const android::CasData& data) override {
        return guarded([&] {
            std::lock_guard lock(mutex_);
            if (releasing_) return Result::InvalidState;
            if (data.size() > 251) return Result::BadValue;
            privateData_ = data;
            return Result::Ok;
        });
    }

    android::status_t openSession(android::CasSessionId* id) override {
        return openSession(0, 8, id);
    }

    android::status_t openSession(uint32_t intent, uint32_t mode, android::CasSessionId* id) override {
        if (id == nullptr) return android::BAD_VALUE;
        id->clear();
        return guarded([&] {
            if (intent != 0 || mode != 8) return Result::Unsupported;
            std::lock_guard lock(mutex_);
            if (releasing_ || !server_->healthy()) return Result::InvalidState;
            std::shared_ptr<KeyRegistry::Slot> slot;
            auto result = KeyRegistry::instance().open(&slot);
            if (result != Result::Ok) return result;
            android::CasSessionId identity(slot->token.begin(), slot->token.end());
            sessions_.emplace(identity, Session{slot, {}});
            *id = std::move(identity);
            return Result::Ok;
        });
    }

    android::status_t closeSession(const android::CasSessionId& id) override {
        return guarded([&] {
            std::lock_guard lock(mutex_);
            auto it = sessions_.find(id);
            if (it == sessions_.end()) return Result::SessionClosed;
            KeyRegistry::instance().close(it->second.slot);
            sessions_.erase(it);
            return Result::Ok;
        });
    }

    android::status_t setSessionPrivateData(const android::CasSessionId& id,
                                           const android::CasData& data) override {
        return guarded([&] {
            std::lock_guard lock(mutex_);
            auto it = sessions_.find(id);
            if (it == sessions_.end()) return Result::SessionClosed;
            if (data.size() > 251) return Result::BadValue;
            it->second.privateData = data;
            return Result::Ok;
        });
    }

    android::status_t processEcm(const android::CasSessionId& id, const android::CasEcm& section) override {
        return guarded([&] {
            std::lock_guard lock(mutex_);
            const auto it = sessions_.find(id);
            if (it == sessions_.end()) return Result::SessionClosed;
            if (!server_->healthy()) return Result::InvalidState;
            View payload;
            auto result = ecmPayload(section, &payload);
            if (result != Result::Ok) return result;
            result = YakisobaBackend::instance().processEcm(it->second.slot, payload);
            return server_->healthy() ? result : Result::InvalidState;
        });
    }

    android::status_t processEmm(const android::CasEmm& section) override {
        return guarded([&] {
            std::lock_guard lock(mutex_);
            if (releasing_ || !server_->healthy()) return Result::InvalidState;
            std::vector<EmmMessage> messages;
            auto result = emmMessages(section, &messages);
            return result == Result::Ok ? YakisobaBackend::instance().processEmm(messages) : result;
        });
    }

    android::status_t sendEvent(int32_t, int32_t, const android::CasData&) override {
        return android::ERROR_CAS_CANNOT_HANDLE;
    }
    android::status_t sendSessionEvent(const android::CasSessionId& id, int32_t, int32_t,
                                     const android::CasData&) override {
        return guarded([&] {
            std::lock_guard lock(mutex_);
            return sessions_.count(id) ? Result::Unsupported : Result::SessionClosed;
        });
    }
    android::status_t provision(const android::String8&) override {
        return android::ERROR_CAS_CANNOT_HANDLE;
    }
    android::status_t refreshEntitlements(int32_t, const android::CasData&) override {
        return android::ERROR_CAS_CANNOT_HANDLE;
    }

private:
    struct Session {
        std::shared_ptr<KeyRegistry::Slot> slot;
        Bytes privateData;
    };
    void* const appData_;
    std::shared_ptr<KeyServer> server_;
    std::mutex mutex_;
    bool releasing_ = false;
    android::CasPluginStatusCallback callback_ = nullptr;
    Bytes privateData_;
    std::map<android::CasSessionId, Session> sessions_;
};

class B25Factory final : public android::CasFactory {
public:
    bool isSystemIdSupported(int32_t id) const override { return id == kB25SystemId; }
    android::status_t queryPlugins(std::vector<android::CasPluginDescriptor>* descriptors) const override {
        if (descriptors == nullptr) return android::BAD_VALUE;
        return guarded([&] {
            std::vector<android::CasPluginDescriptor> supported;
            supported.push_back({kB25SystemId, android::String8("Maleicacid B25 (Yakisoba)")});
            *descriptors = std::move(supported);
            return Result::Ok;
        });
    }
    android::status_t createPlugin(int32_t id, void* data, android::CasPluginCallback,
                                  android::CasPlugin** plugin) override { return create(id, data, plugin); }
    android::status_t createPlugin(int32_t id, void* data, android::CasPluginCallbackExt,
                                  android::CasPlugin** plugin) override { return create(id, data, plugin); }
private:
    android::status_t create(int32_t id, void* data, android::CasPlugin** plugin) {
        if (plugin == nullptr) return android::BAD_VALUE;
        *plugin = nullptr;
        return guarded([&] {
            if (!isSystemIdSupported(id)) return Result::Unsupported;
            auto server = KeyServer::acquire();
            if (!server) return Result::Busy;
            *plugin = new B25Plugin(data, std::move(server));
            return Result::Ok;
        });
    }
};

}  // namespace
}  // namespace maleicacid::cas

extern "C" __attribute__((visibility("default"))) android::CasFactory* createCasFactory() {
    return new (std::nothrow) maleicacid::cas::B25Factory();
}
