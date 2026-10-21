/* Compile twice: once exactly as scrcpy.c sees the HQ sink type and once
 * with the D3D11 sink's compilation flag. The sizes MUST match, otherwise
 * sink_init() zeroes beyond scrcpy.c's allocation and can kill the session.
 */
#include "../scrcpy/mirroleap_hq_sink.h"
#include <stdio.h>
int main(void) {
    printf("%zu\n",sizeof(struct sc_mirroleap_hq_sink));
    return 0;
}
