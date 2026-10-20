#ifndef MLHQ_TEST_AVFRAME_H
#define MLHQ_TEST_AVFRAME_H
#include <stdint.h>
typedef struct AVFrame {
    uint8_t *data[8];
    int linesize[8];
    int width;
    int height;
    int format;
    int64_t pts;
} AVFrame;
AVFrame *av_frame_clone(const AVFrame *);
void av_frame_free(AVFrame **);
#endif
