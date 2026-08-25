#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

void *aribcc_context_alloc(void) { return NULL; }
void aribcc_context_free(void *context) { (void)context; }
void *aribcc_decoder_alloc(void *context) {
    (void)context;
    return NULL;
}
void aribcc_decoder_free(void *decoder) { (void)decoder; }
bool aribcc_decoder_initialize(
        void *decoder,
        int encoding_scheme,
        int caption_type,
        int profile,
        int language_id) {
    (void)decoder;
    (void)encoding_scheme;
    (void)caption_type;
    (void)profile;
    (void)language_id;
    return false;
}
int aribcc_decoder_decode(
        void *decoder,
        const uint8_t *pes_data,
        size_t length,
        int64_t pts,
        void *out_caption) {
    (void)decoder;
    (void)pes_data;
    (void)length;
    (void)pts;
    (void)out_caption;
    return 0;
}
void aribcc_decoder_flush(void *decoder) { (void)decoder; }
void aribcc_caption_cleanup(void *caption) { (void)caption; }
void *aribcc_renderer_alloc(void *context) {
    (void)context;
    return NULL;
}
void aribcc_renderer_free(void *renderer) { (void)renderer; }
bool aribcc_renderer_initialize(
        void *renderer,
        int caption_type,
        int font_provider_type,
        int text_renderer_type) {
    (void)renderer;
    (void)caption_type;
    (void)font_provider_type;
    (void)text_renderer_type;
    return false;
}
bool aribcc_renderer_set_frame_size(void *renderer, int frame_width, int frame_height) {
    (void)renderer;
    (void)frame_width;
    (void)frame_height;
    return false;
}
bool aribcc_renderer_append_caption(void *renderer, const void *caption) {
    (void)renderer;
    (void)caption;
    return false;
}
int aribcc_renderer_render(void *renderer, int64_t pts, void *out_result) {
    (void)renderer;
    (void)pts;
    (void)out_result;
    return 0;
}
void aribcc_renderer_flush(void *renderer) { (void)renderer; }
void aribcc_render_result_cleanup(void *result) { (void)result; }
