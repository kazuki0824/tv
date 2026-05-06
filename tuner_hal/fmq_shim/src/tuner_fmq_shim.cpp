#include "tuner_fmq_shim.h"

#include <aidl/android/hardware/common/fmq/GrantorDescriptor.h>
#include <aidl/android/hardware/common/fmq/MQDescriptor.h>
#include <aidl/android/hardware/common/fmq/SynchronizedReadWrite.h>
#include <android-base/unique_fd.h>
#include <BufferAllocator/BufferAllocator.h>
#include <fmq/AidlMessageQueue.h>
#include <fmq/EventFlag.h>
#include <unistd.h>
#include <errno.h>
#include <vector>

using aidl::android::hardware::common::fmq::SynchronizedReadWrite;
using android::AidlMessageQueue;
using QueueType = AidlMessageQueue<int8_t, SynchronizedReadWrite>;

struct tuner_fmq_queue {
    QueueType queue;
    android::hardware::EventFlag* event_flag = nullptr;

    tuner_fmq_queue(size_t num_bytes, bool configure_event_flag)
        : queue(num_bytes, configure_event_flag) {
        if (configure_event_flag) {
            auto* word = queue.getEventFlagWord();
            if (word != nullptr) {
                android::hardware::EventFlag::createEventFlag(word, &event_flag);
            }
        }
    }

    ~tuner_fmq_queue() {
        if (event_flag != nullptr) {
            android::hardware::EventFlag::deleteEventFlag(&event_flag);
        }
    }
};

extern "C" tuner_fmq_queue* tuner_fmq_queue_create(size_t num_bytes, bool configure_event_flag) {
    auto* q = new tuner_fmq_queue(num_bytes, configure_event_flag);
    return q;
}

extern "C" void tuner_fmq_queue_destroy(tuner_fmq_queue* queue) {
    delete queue;
}

extern "C" size_t tuner_fmq_queue_available_to_read(const tuner_fmq_queue* queue) {
    return queue ? queue->queue.availableToRead() : 0;
}

extern "C" size_t tuner_fmq_queue_available_to_write(const tuner_fmq_queue* queue) {
    return queue ? queue->queue.availableToWrite() : 0;
}

extern "C" size_t tuner_fmq_queue_write(tuner_fmq_queue* queue, const uint8_t* data, size_t size) {
    if (queue == nullptr || data == nullptr || size == 0) return 0;
    if (!queue->queue.write(reinterpret_cast<const int8_t*>(data), size)) return 0;
    return size;
}

extern "C" size_t tuner_fmq_queue_read(tuner_fmq_queue* queue, uint8_t* data, size_t size) {
    if (queue == nullptr || data == nullptr || size == 0) return 0;
    size_t avail = queue->queue.availableToRead();
    size_t to_read = avail < size ? avail : size;
    if (to_read == 0) return 0;
    if (!queue->queue.read(reinterpret_cast<int8_t*>(data), to_read)) return 0;
    return to_read;
}

extern "C" int tuner_fmq_queue_wake(tuner_fmq_queue* queue, uint32_t bits) {
    if (queue == nullptr || queue->event_flag == nullptr) return -1;
    return queue->event_flag->wake(bits);
}

extern "C" int tuner_fmq_queue_wait(tuner_fmq_queue* queue, uint32_t bits, int64_t timeout_ns, uint32_t* state) {
    if (queue == nullptr || queue->event_flag == nullptr) return -1;
    return queue->event_flag->wait(bits, state, timeout_ns, true);
}

static auto desc_for(const tuner_fmq_queue* queue) {
    return const_cast<QueueType&>(queue->queue).dupeDesc();
}

extern "C" int32_t tuner_fmq_queue_quantum(const tuner_fmq_queue* queue) {
    if (queue == nullptr) return 0;
    return desc_for(queue).quantum;
}

extern "C" int32_t tuner_fmq_queue_flags(const tuner_fmq_queue* queue) {
    if (queue == nullptr) return 0;
    return desc_for(queue).flags;
}

extern "C" size_t tuner_fmq_queue_grantor_count(const tuner_fmq_queue* queue) {
    if (queue == nullptr) return 0;
    return desc_for(queue).grantors.size();
}

extern "C" bool tuner_fmq_queue_grantor_at(const tuner_fmq_queue* queue, size_t index,
                                             int32_t* fd_index, int32_t* offset, int64_t* extent) {
    if (queue == nullptr) return false;
    auto desc = desc_for(queue);
    if (index >= desc.grantors.size()) return false;
    const auto& grantor = desc.grantors[index];
    if (fd_index) *fd_index = grantor.fdIndex;
    if (offset) *offset = grantor.offset;
    if (extent) *extent = grantor.extent;
    return true;
}

extern "C" size_t tuner_fmq_queue_fd_count(const tuner_fmq_queue* queue) {
    if (queue == nullptr) return 0;
    auto desc = desc_for(queue);
    return desc.handle.fds.size();
}

extern "C" int tuner_fmq_queue_dup_fd_at(const tuner_fmq_queue* queue, size_t index) {
    if (queue == nullptr) return -1;
    auto desc = desc_for(queue);
    if (index >= desc.handle.fds.size()) return -1;
    return dup(desc.handle.fds[index].get());
}

extern "C" size_t tuner_fmq_queue_int_count(const tuner_fmq_queue* queue) {
    if (queue == nullptr) return 0;
    auto desc = desc_for(queue);
    return desc.handle.ints.size();
}

extern "C" bool tuner_fmq_queue_int_at(const tuner_fmq_queue* queue, size_t index, int32_t* value) {
    if (queue == nullptr) return false;
    auto desc = desc_for(queue);
    if (index >= desc.handle.ints.size()) return false;
    if (value) *value = desc.handle.ints[index];
    return true;
}


extern "C" int tuner_dmabuf_heap_alloc_system(size_t len) {
    if (len == 0) return -EINVAL;
    BufferAllocator allocator;
    int fd = allocator.Alloc("system", len, 0, 0);
    if (fd >= 0) return fd;
    int captured_errno = errno;
    if (captured_errno == 0) captured_errno = EIO;
    return -captured_errno;
}
