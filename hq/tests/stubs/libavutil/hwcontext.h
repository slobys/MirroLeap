/* Test-only minimal libavutil/hwcontext.h. Do not ship this stub. */
#ifndef MLHQ_TEST_HWCONTEXT_H
#define MLHQ_TEST_HWCONTEXT_H
#include <stdint.h>
#include <libavutil/pixfmt.h>
typedef struct AVBufferRef {
    uint8_t *data;
} AVBufferRef;
typedef struct AVHWFramesContext {
    int sw_format;
} AVHWFramesContext;
#include <libavutil/frame.h>
AVFrame *av_frame_alloc(void);
int av_hwframe_transfer_data(AVFrame *dst,const AVFrame *src,int flags);
#endif
