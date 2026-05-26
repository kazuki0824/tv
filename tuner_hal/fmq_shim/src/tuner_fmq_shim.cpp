#include "tuner_fmq_shim.h"

#include <aidl/android/hardware/common/fmq/GrantorDescriptor.h>
#include <aidl/android/hardware/common/fmq/MQDescriptor.h>
#include <aidl/android/hardware/common/fmq/SynchronizedReadWrite.h>
#include <android-base/unique_fd.h>
#include <BufferAllocator/BufferAllocator.h>
#include <fmq/AidlMessageQueue.h>
#include <fmq/EventFlag.h>
#include <utils/Errors.h>
#include <unistd.h>
#include <errno.h>
#include <new>
#include <utility>
#include <vector>

using aidl::android::hardware::common::fmq::SynchronizedReadWrite;
using android::AidlMessageQueue;
using QueueType = AidlMessageQueue<int8_t, SynchronizedReadWrite>;

struct cached_fmq_descriptor {
    int32_t quantum = 0;
    int32_t flags = 0;
    std::vector<aidl::android::hardware::common::fmq::GrantorDescriptor> grantors;
    std::vector<android::base::unique_fd> fds;
    std::vector<int32_t> ints;
    bool valid = false;

    void reset() {
        quantum = 0;
        flags = 0;
        grantors.clear();
        fds.clear();
        ints.clear();
        valid = false;
    }
};

struct tuner_fmq_queue {
    QueueType queue;
    cached_fmq_descriptor desc;
    android::hardware::EventFlag* event_flag = nullptr;
    bool valid = false;

    tuner_fmq_queue(size_t num_bytes, bool configure_event_flag)
        : queue(num_bytes, configure_event_flag) {
        if (!queue.isValid()) {
            return;
        }

        auto exported_desc = queue.dupeDesc();
        desc.quantum = exported_desc.quantum;
        desc.flags = exported_desc.flags;
        desc.grantors = exported_desc.grantors;
        desc.ints = exported_desc.handle.ints;
        desc.fds.reserve(exported_desc.handle.fds.size());
        for (const auto& fd : exported_desc.handle.fds) {
            int duped_fd = dup(fd.get());
            if (duped_fd < 0) {
                desc.reset();
                return;
            }
            desc.fds.emplace_back(duped_fd);
        }
        desc.valid = true;

        if (configure_event_flag) {
            auto* word = queue.getEventFlagWord();
            if (word == nullptr) {
                desc.reset();
                return;
            }
            if (android::hardware::EventFlag::createEventFlag(word, &event_flag) != android::OK
                || event_flag == nullptr) {
                if (event_flag != nullptr) {
                    android::hardware::EventFlag::deleteEventFlag(&event_flag);
                    event_flag = nullptr;
                }
                desc.reset();
                return;
            }
        }
        valid = true;
    }

    ~tuner_fmq_queue() {
        if (event_flag != nullptr) {
            android::hardware::EventFlag::deleteEventFlag(&event_flag);
            event_flag = nullptr;
        }
    }
};

extern "C" tuner_fmq_queue* tuner_fmq_queue_create(size_t num_bytes, bool configure_event_flag) {
    if (num_bytes == 0) return nullptr;
    auto* q = new (std::nothrow) tuner_fmq_queue(num_bytes, configure_event_flag);
    if (q == nullptr) return nullptr;
    if (!q->valid) {
        delete q;
        return nullptr;
    }
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

extern "C" int tuner_fmq_queue_write_checked(
        tuner_fmq_queue* queue, const uint8_t* data, size_t size, size_t* out_written) {
    if (out_written != nullptr) {
        *out_written = 0;
    }
    if (queue == nullptr || out_written == nullptr) {
        return -1;
    }
    if (size == 0) {
        return 0;
    }
    if (data == nullptr) {
        return -1;
    }
    if (!queue->queue.write(reinterpret_cast<const int8_t*>(data), size)) {
        return -2;
    }
    *out_written = size;
    return 0;
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

extern "C" int32_t tuner_fmq_queue_quantum(const tuner_fmq_queue* queue) {
    if (queue == nullptr || !queue->desc.valid) return 0;
    return queue->desc.quantum;
}

extern "C" int32_t tuner_fmq_queue_flags(const tuner_fmq_queue* queue) {
    if (queue == nullptr || !queue->desc.valid) return 0;
    return queue->desc.flags;
}

extern "C" size_t tuner_fmq_queue_grantor_count(const tuner_fmq_queue* queue) {
    if (queue == nullptr || !queue->desc.valid) return 0;
    return queue->desc.grantors.size();
}

extern "C" bool tuner_fmq_queue_grantor_at(const tuner_fmq_queue* queue, size_t index,
                                             int32_t* fd_index, int32_t* offset, int64_t* extent) {
    if (queue == nullptr || !queue->desc.valid) return false;
    if (index >= queue->desc.grantors.size()) return false;
    const auto& grantor = queue->desc.grantors[index];
    if (fd_index) *fd_index = grantor.fdIndex;
    if (offset) *offset = grantor.offset;
    if (extent) *extent = grantor.extent;
    return true;
}

extern "C" size_t tuner_fmq_queue_fd_count(const tuner_fmq_queue* queue) {
    if (queue == nullptr || !queue->desc.valid) return 0;
    return queue->desc.fds.size();
}

extern "C" int tuner_fmq_queue_dup_fd_at(const tuner_fmq_queue* queue, size_t index) {
    if (queue == nullptr || !queue->desc.valid) return -1;
    if (index >= queue->desc.fds.size()) return -1;
    return dup(queue->desc.fds[index].get());
}

extern "C" size_t tuner_fmq_queue_int_count(const tuner_fmq_queue* queue) {
    if (queue == nullptr || !queue->desc.valid) return 0;
    return queue->desc.ints.size();
}

extern "C" bool tuner_fmq_queue_int_at(const tuner_fmq_queue* queue, size_t index, int32_t* value) {
    if (queue == nullptr || !queue->desc.valid) return false;
    if (index >= queue->desc.ints.size()) return false;
    if (value) *value = queue->desc.ints[index];
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
