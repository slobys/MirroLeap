#ifndef MLHQ_TEST_FRAMESINK_H
#define MLHQ_TEST_FRAMESINK_H
#include <stdbool.h>
#include "libavutil/frame.h"
struct sc_stream_session;
struct AVCodecContext;
typedef struct AVCodecContext AVCodecContext;
struct sc_frame_sink;
struct sc_frame_sink_ops {
    bool (*open)(struct sc_frame_sink *,const struct AVCodecContext *,
                 const struct sc_stream_session *);
    void (*close)(struct sc_frame_sink *);
    bool (*push)(struct sc_frame_sink *,const AVFrame *);
    bool (*push_session)(struct sc_frame_sink *,const struct sc_stream_session *);
};
struct sc_frame_sink {const struct sc_frame_sink_ops *ops;};
#endif
