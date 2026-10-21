#ifndef MLHQ_TEST_FRAMESINK_H
#define MLHQ_TEST_FRAMESINK_H
#include <stdbool.h>
#include "libavutil/frame.h"
struct sc_stream_session;
struct AVCodecContext;
typedef struct AVCodecContext AVCodecContext;
struct sc_frame_sink;
#ifdef MLHQ_ENABLE_D3D11
enum sc_sink_result {SC_SINK_OK=0,SC_SINK_KO=1,SC_SINK_STOPPED=2};
#endif
struct sc_frame_sink_ops {
    bool (*open)(struct sc_frame_sink *,const struct AVCodecContext *,
                 const struct sc_stream_session *);
    void (*close)(struct sc_frame_sink *);
#ifdef MLHQ_ENABLE_D3D11
    enum sc_sink_result (*push)(struct sc_frame_sink *,const AVFrame *);
    enum sc_sink_result (*push_session)(
        struct sc_frame_sink *,const struct sc_stream_session *);
    void (*apply_backpressure)(struct sc_frame_sink *);
#else
    bool (*push)(struct sc_frame_sink *,const AVFrame *);
    bool (*push_session)(struct sc_frame_sink *,const struct sc_stream_session *);
#endif
};
struct sc_frame_sink {const struct sc_frame_sink_ops *ops;};
#endif
