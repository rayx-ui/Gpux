// gpux reaches io-surface through core-video on every Apple target, and io-surface's OpenGL
// texture binding calls these CGL functions, which exist only on macOS. The shell force-loads the
// whole Rust library, so the linker needs definitions; iOS never calls them.

#include <stdlib.h>

const char *CGLErrorString(int error) {
    (void)error;
    abort();
}

void *CGLGetCurrentContext(void) {
    abort();
}

int CGLTexImageIOSurface2D(void *context, unsigned target, unsigned internal_format, int width,
                           int height, unsigned format, unsigned type, void *io_surface,
                           unsigned plane) {
    (void)context, (void)target, (void)internal_format, (void)width, (void)height, (void)format,
        (void)type, (void)io_surface, (void)plane;
    abort();
}
