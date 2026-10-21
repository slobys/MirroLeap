/* MirroLeap raw source-frame receiver for PINNED scrcpy 5.0.1 (Apache-2.0).
 * Uses the same reviewed v2 IPC as the scrcpy 4.0 reference, but consumes
 * FFmpeg D3D11VA hardware AVFrames only on a separate bounded worker.
 * A clone on the decoder thread is metadata/reference only. This is NOT
 * a D3D11 GPU magnifier renderer or a zero-copy cross-process path.
 */
#define MLHQ_ENABLE_D3D11 1
#include "mirroleap_hq_sink.c"
