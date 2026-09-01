#include "fmq_import.h"

#include <aidl/android/hardware/common/NativeHandle.h>
#include <aidl/android/hardware/common/fmq/GrantorDescriptor.h>
#include <aidl/android/hardware/common/fmq/MQDescriptor.h>
#include <aidl/android/hardware/common/fmq/SynchronizedReadWrite.h>
#include <android-base/unique_fd.h>
#include <fmq/AidlMessageQueue.h>
#include <unistd.h>

#include <memory>
#include <new>
#include <vector>

using aidl::android::hardware::common::NativeHandle;
using aidl::android::hardware::common::fmq::GrantorDescriptor;
using aidl::android::hardware::common::fmq::MQDescriptor;
using aidl::android::hardware::common::fmq::SynchronizedReadWrite;
using android::AidlMessageQueue;
using Queue = AidlMessageQueue<int8_t, SynchronizedReadWrite>;
using Desc = MQDescriptor<int8_t, SynchronizedReadWrite>;

struct vts_agent_fmq {
    explicit vts_agent_fmq(const Desc& desc) : queue(desc, true) {}
    Queue queue;
};

extern "C" vts_agent_fmq* vts_agent_fmq_import(
        int32_t quantum, int32_t flags,
        const vts_agent_fmq_grantor* grantors, size_t grantor_count,
        const int32_t* fds, size_t fd_count,
        const int32_t* ints, size_t int_count) {
    if ((grantor_count != 0 && grantors == nullptr) ||
        (fd_count != 0 && fds == nullptr) ||
        (int_count != 0 && ints == nullptr)) {
        return nullptr;
    }
    Desc desc;
    desc.quantum = quantum;
    desc.flags = flags;
    desc.grantors.reserve(grantor_count);
    for (size_t i = 0; i < grantor_count; ++i) {
        desc.grantors.push_back(GrantorDescriptor{
            .fdIndex = grantors[i].fd_index,
            .offset = grantors[i].offset,
            .extent = grantors[i].extent,
        });
    }
    desc.handle.fds.reserve(fd_count);
    for (size_t i = 0; i < fd_count; ++i) {
        const int duped = dup(fds[i]);
        if (duped < 0) return nullptr;
        desc.handle.fds.emplace_back(duped);
    }
    if (int_count != 0) {
        desc.handle.ints.assign(ints, ints + int_count);
    }
    auto* imported = new (std::nothrow) vts_agent_fmq(desc);
    if (imported == nullptr) return nullptr;
    if (!imported->queue.isValid()) {
        delete imported;
        return nullptr;
    }
    return imported;
}

extern "C" void vts_agent_fmq_destroy(vts_agent_fmq* queue) {
    delete queue;
}

extern "C" size_t vts_agent_fmq_available_to_read(const vts_agent_fmq* queue) {
    return queue == nullptr ? 0 : queue->queue.availableToRead();
}

extern "C" size_t vts_agent_fmq_read(vts_agent_fmq* queue, uint8_t* data, size_t size) {
    if (queue == nullptr || data == nullptr || size == 0) return 0;
    const size_t available = queue->queue.availableToRead();
    const size_t requested = available < size ? available : size;
    if (requested == 0) return 0;
    if (!queue->queue.read(reinterpret_cast<int8_t*>(data), requested)) return 0;
    return requested;
}
