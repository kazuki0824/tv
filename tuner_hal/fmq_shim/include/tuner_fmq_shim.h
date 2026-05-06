#pragma once

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct tuner_fmq_queue tuner_fmq_queue;

tuner_fmq_queue* tuner_fmq_queue_create(size_t num_bytes, bool configure_event_flag);
void tuner_fmq_queue_destroy(tuner_fmq_queue* queue);
size_t tuner_fmq_queue_available_to_read(const tuner_fmq_queue* queue);
size_t tuner_fmq_queue_available_to_write(const tuner_fmq_queue* queue);
size_t tuner_fmq_queue_write(tuner_fmq_queue* queue, const uint8_t* data, size_t size);
size_t tuner_fmq_queue_read(tuner_fmq_queue* queue, uint8_t* data, size_t size);
int tuner_fmq_queue_wake(tuner_fmq_queue* queue, uint32_t bits);
int tuner_fmq_queue_wait(tuner_fmq_queue* queue, uint32_t bits, int64_t timeout_ns, uint32_t* state);
int32_t tuner_fmq_queue_quantum(const tuner_fmq_queue* queue);
int32_t tuner_fmq_queue_flags(const tuner_fmq_queue* queue);
size_t tuner_fmq_queue_grantor_count(const tuner_fmq_queue* queue);
bool tuner_fmq_queue_grantor_at(const tuner_fmq_queue* queue, size_t index,
                                int32_t* fd_index, int32_t* offset, int64_t* extent);
size_t tuner_fmq_queue_fd_count(const tuner_fmq_queue* queue);
int tuner_fmq_queue_dup_fd_at(const tuner_fmq_queue* queue, size_t index);
size_t tuner_fmq_queue_int_count(const tuner_fmq_queue* queue);
bool tuner_fmq_queue_int_at(const tuner_fmq_queue* queue, size_t index, int32_t* value);

// AOSP libdmabufheap の "system" heap から CPU でアクセス可能な dma-buf を確保する。
// 成功時は所有権を持つ fd を返し、失敗時は -errno を返す。
int tuner_dmabuf_heap_alloc_system(size_t len);

#ifdef __cplusplus
}
#endif
