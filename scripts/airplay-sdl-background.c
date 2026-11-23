#define WIN32_LEAN_AND_MEAN
#include <windows.h>

enum {
    SDL_WINDOW_SHOWN = 0x00000004,
    SDL_WINDOW_HIDDEN = 0x00000008
};

typedef void *(__cdecl *CreateWindowFn)(const char *, int, int, int, int, unsigned);

/* Loaded only by MirroLeap's private AirPlay receiver runtime. */
void *__cdecl mirroleap_create_window(
    const char *title, int x, int y, int width, int height, unsigned flags
) {
    HMODULE sdl = GetModuleHandleW(L"SDL2_original.dll");
    if (!sdl) {
        sdl = LoadLibraryExW(L"SDL2_original.dll", NULL, LOAD_LIBRARY_SEARCH_APPLICATION_DIR);
    }
    CreateWindowFn create = sdl ? (CreateWindowFn)GetProcAddress(sdl, "SDL_CreateWindow") : NULL;
    if (!create) {
        return NULL;
    }
    return create(title, x, y, width, height, (flags & ~SDL_WINDOW_SHOWN) | SDL_WINDOW_HIDDEN);
}
