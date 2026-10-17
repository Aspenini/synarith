// Private, synchronous UI-thread bridge; this is not an installed public C API.
// All strings are UTF-8. Input strings/pixels are borrowed for the call only.
// Output buffers are caller-owned; getters return required bytes including NUL.
// The callback context remains owned by Rust throughout syar_ui_run.
#pragma once
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <string>

enum Field { Source, Width, Height, Time, Samples, Backend, Output, Parameters,
             Format, Quality, Seed, FieldCount };
enum Action { Ready, PreviewAction, ExportAction, CancelAction, Tick, Changed,
              OpenAction, CloseAction };
using Callback = void (*)(void *, int);
inline size_t copy_text(const std::string &text, char *buffer, size_t capacity) {
    if (buffer && capacity) {
        size_t count = text.size() < capacity - 1 ? text.size() : capacity - 1;
        std::memcpy(buffer, text.data(), count);
        buffer[count] = 0;
    }
    return text.size() + 1;
}
extern "C" {
int syar_ui_run(Callback callback, void *context, const char *source);
size_t syar_ui_get(int field, char *buffer, size_t capacity);
void syar_ui_set(int field, const char *value);
void syar_ui_status(const char *message, bool busy);
void syar_ui_log(const char *message);
void syar_ui_image(const uint8_t *rgba, int width, int height);
bool syar_ui_choose(bool save, const char *suggested, const char *extension,
                    char *buffer, size_t capacity);
bool syar_ui_confirm(const char *message);
void syar_ui_quit();
size_t syar_ui_capture(uint8_t *rgba, size_t capacity, int *width, int *height);
}
