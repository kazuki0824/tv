#pragma once

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct vts_agent_fmq vts_agent_fmq;

typedef struct vts_agent_fmq_grantor {
    int32_t fd_index;
    int32_t offset;
    int64_t extent;
} vts_agent_fmq_grantor;

vts_agent_fmq* vts_agent_fmq_import(
        int32_t quantum, int32_t flags,
        const vts_agent_fmq_grantor* grantors, size_t grantor_count,
        const int32_t* fds, size_t fd_count,
        const int32_t* ints, size_t int_count);
void vts_agent_fmq_destroy(vts_agent_fmq* queue);
size_t vts_agent_fmq_available_to_read(const vts_agent_fmq* queue);
size_t vts_agent_fmq_read(vts_agent_fmq* queue, uint8_t* data, size_t size);

#ifdef __cplusplus
}
#endif
