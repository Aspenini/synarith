#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#define UNICODE
#include <windows.h>
#include <commdlg.h>
#include <commctrl.h>
#include <algorithm>
#include <vector>
#include "widgets.h"
#ifdef _MSC_VER
#pragma comment(linker, "\"/manifestdependency:type='win32' name='Microsoft.Windows.Common-Controls' version='6.0.0.0' processorArchitecture='*' publicKeyToken='6595b64144ccf1df' language='*'\"")
#endif

namespace {
HWND window, fields[FieldCount], labels[FieldCount], preview, status, log_box;
HWND open_button, render_button, export_button, cancel_button;
HFONT font;
Callback handler;
void *state;
bool changing = false, active = false;
int image_width = 0, image_height = 0;
double scale = 1;
std::vector<uint8_t> pixels;
std::wstring wide(const char *text) {
    int size = MultiByteToWideChar(CP_UTF8, 0, text, -1, nullptr, 0);
    if (size <= 0) return {};
    std::wstring result(size, L'\0');
    MultiByteToWideChar(CP_UTF8, 0, text, -1, result.data(), size);
    result.pop_back();
    return result;
}
std::string utf8(const std::wstring &text) {
    int size = WideCharToMultiByte(CP_UTF8, 0, text.data(), (int)text.size(), nullptr, 0, nullptr, nullptr);
    std::string result(size, '\0');
    WideCharToMultiByte(CP_UTF8, 0, text.data(), (int)text.size(), result.data(), size, nullptr, nullptr);
    return result;
}
std::wstring text(HWND control) {
    int length = GetWindowTextLengthW(control);
    std::wstring result(length + 1, L'\0');
    GetWindowTextW(control, result.data(), length + 1);
    result.resize(length);
    return result;
}
int px(int n) { return (int)(n * scale); }
void move(HWND h, int x, int y, int w, int hgt) { MoveWindow(h, px(x), px(y), px(w), px(hgt), TRUE); }
HWND control(const wchar_t *kind, const wchar_t *value, DWORD style, int id) {
    HWND child = CreateWindowExW(kind == std::wstring(L"EDIT") ? WS_EX_CLIENTEDGE : 0,
        kind, value, WS_CHILD | WS_VISIBLE | style, 0, 0, 1, 1, window,
        reinterpret_cast<HMENU>((INT_PTR)id), GetModuleHandleW(nullptr), nullptr);
    SendMessageW(child, WM_SETFONT, (WPARAM)font, TRUE);
    return child;
}
void layout() {
    RECT client; GetClientRect(window, &client);
    int w = (int)((client.right - client.left) / scale);
    int h = (int)((client.bottom - client.top) / scale);
    move(labels[Source], 16, 18, 88, 22);
    move(fields[Source], 104, 14, w - 210, 26);
    move(open_button, w - 96, 13, 80, 28);
    for (int i = Width; i < FieldCount; ++i) {
        int y = 62 + (i - Width) * 34;
        move(labels[i], 16, y + 4, 95, 24);
        move(fields[i], 112, y, 214, i == Samples || i == Backend || i == Format ? 180 : 27);
    }
    move(render_button, 16, 418, 96, 30);
    move(export_button, 120, 418, 96, 30);
    move(cancel_button, 224, 418, 102, 30);
    move(status, 16, 462, 310, 72);
    move(log_box, 16, 542, 310, std::max(70, h - 558));
    move(preview, 346, 60, std::max(200, w - 362), std::max(200, h - 76));
}
LRESULT CALLBACK image_proc(HWND hwnd, UINT message, WPARAM wp, LPARAM lp) {
    if (message == WM_ERASEBKGND) return 1;
    if (message == WM_PAINT) {
        PAINTSTRUCT ps; HDC dc = BeginPaint(hwnd, &ps);
        RECT rect; GetClientRect(hwnd, &rect);
        FillRect(dc, &rect, (HBRUSH)GetStockObject(WHITE_BRUSH));
        if (!pixels.empty()) {
            int w = rect.right, h = rect.bottom;
            double factor = std::min((double)w / image_width, (double)h / image_height);
            int dw = (int)(image_width * factor), dh = (int)(image_height * factor);
            BITMAPINFO info{};
            info.bmiHeader.biSize = sizeof(BITMAPINFOHEADER);
            info.bmiHeader.biWidth = image_width;
            info.bmiHeader.biHeight = -image_height;
            info.bmiHeader.biPlanes = 1;
            info.bmiHeader.biBitCount = 32;
            info.bmiHeader.biCompression = BI_RGB;
            SetStretchBltMode(dc, HALFTONE);
            SetBrushOrgEx(dc, 0, 0, nullptr);
            StretchDIBits(dc, (w - dw) / 2, (h - dh) / 2, dw, dh, 0, 0, image_width, image_height,
                pixels.data(), &info, DIB_RGB_COLORS, SRCCOPY);
        } else {
            SelectObject(dc, font); SetBkMode(dc, TRANSPARENT);
            DrawTextW(dc, L"Open a .syar document and render a preview", -1, &rect, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
        }
        EndPaint(hwnd, &ps); return 0;
    }
    return DefWindowProcW(hwnd, message, wp, lp);
}
LRESULT CALLBACK window_proc(HWND hwnd, UINT message, WPARAM wp, LPARAM lp) {
    switch (message) {
    case WM_SIZE: if (preview) layout(); return 0;
    case WM_GETMINMAXINFO:
        reinterpret_cast<MINMAXINFO *>(lp)->ptMinTrackSize = {px(900), px(700)}; return 0;
    case WM_TIMER: if (active) handler(state, Tick); return 0;
    case WM_COMMAND: {
        int id = LOWORD(wp), code = HIWORD(wp);
        if (id >= 100 && id <= 103 && (code == BN_CLICKED || code == 1)) {
            const int actions[] = {OpenAction, PreviewAction, ExportAction, CancelAction};
            handler(state, actions[id - 100]);
        } else if (!changing && active && id >= 200 && id < 200 + FieldCount
                   && (code == EN_CHANGE || code == CBN_SELCHANGE)) {
            handler(state, Changed);
        }
        return 0;
    }
    case WM_CLOSE:
        active = false; handler(state, CloseAction); DestroyWindow(hwnd); return 0;
    case WM_DESTROY: PostQuitMessage(0); return 0;
    }
    return DefWindowProcW(hwnd, message, wp, lp);
}
}

extern "C" int syar_ui_run(Callback callback, void *context, const char *source) {
    handler = callback; state = context;
    SetProcessDPIAware();
    HDC screen = GetDC(nullptr); scale = GetDeviceCaps(screen, LOGPIXELSX) / 96.; ReleaseDC(nullptr, screen);
    NONCLIENTMETRICSW metrics{}; metrics.cbSize = sizeof(metrics);
    SystemParametersInfoW(SPI_GETNONCLIENTMETRICS, sizeof(metrics), &metrics, 0);
    font = CreateFontIndirectW(&metrics.lfMessageFont);
    INITCOMMONCONTROLSEX common{sizeof(common), ICC_STANDARD_CLASSES}; InitCommonControlsEx(&common);
    HINSTANCE instance = GetModuleHandleW(nullptr);
    WNDCLASSW wc{}; wc.hInstance = instance; wc.hCursor = LoadCursorW(nullptr, IDC_ARROW);
    wc.hbrBackground = (HBRUSH)(COLOR_BTNFACE + 1); wc.lpszClassName = L"SynarithNativeViewer"; wc.lpfnWndProc = window_proc;
    RegisterClassW(&wc);
    wc.lpszClassName = L"SynarithPreview"; wc.lpfnWndProc = image_proc; RegisterClassW(&wc);
    window = CreateWindowExW(WS_EX_CONTROLPARENT, L"SynarithNativeViewer", L"Synarith Viewer", WS_OVERLAPPEDWINDOW,
        CW_USEDEFAULT, CW_USEDEFAULT, px(1200), px(800), nullptr, nullptr, instance, nullptr);
    if (!window) { DeleteObject(font); return 1; }
    const wchar_t *names[] = {L"Document", L"Width", L"Height", L"Time (seconds)", L"Samples / axis",
        L"Backend", L"Output name", L"Overrides (;)", L"Format", L"JPEG quality", L"Seed (optional)"};
    const wchar_t *defaults[] = {L"", L"1024", L"1024", L"0", L"1", L"auto", L"main", L"", L"png", L"90", L""};
    for (int i = 0; i < FieldCount; ++i) {
        labels[i] = control(L"STATIC", names[i], SS_LEFT, 0);
        if (i == Samples || i == Backend || i == Format) {
            fields[i] = control(L"COMBOBOX", L"", WS_TABSTOP | CBS_DROPDOWNLIST | WS_VSCROLL, 200 + i);
            const wchar_t *choices = i == Samples ? L"1\0" L"2\0" L"4\0" L"8\0\0" :
                i == Backend ? L"auto\0cpu\0gpu\0\0" : L"png\0jpeg\0bmp\0tga\0ppm\0\0";
            while (*choices) { SendMessageW(fields[i], CB_ADDSTRING, 0, (LPARAM)choices); choices += wcslen(choices) + 1; }
            SendMessageW(fields[i], CB_SETCURSEL, 0, 0);
        } else {
            fields[i] = control(L"EDIT", defaults[i], WS_TABSTOP | ES_AUTOHSCROLL, 200 + i);
            SendMessageW(fields[i], EM_SETLIMITTEXT, 65535, 0);
        }
    }
    SetWindowTextW(fields[Source], wide(source).c_str());
    open_button = control(L"BUTTON", L"Open…", WS_TABSTOP | BS_PUSHBUTTON, 100);
    render_button = control(L"BUTTON", L"Preview", WS_TABSTOP | BS_DEFPUSHBUTTON, 101);
    export_button = control(L"BUTTON", L"Export…", WS_TABSTOP | BS_PUSHBUTTON, 102);
    cancel_button = control(L"BUTTON", L"Cancel", WS_TABSTOP | BS_PUSHBUTTON, 103);
    EnableWindow(cancel_button, FALSE);
    status = control(L"STATIC", L"Choose a document. All rendering runs through syar.", SS_LEFT, 0);
    log_box = control(L"EDIT", L"", ES_MULTILINE | ES_READONLY | ES_AUTOVSCROLL | WS_VSCROLL | WS_TABSTOP, 0);
    preview = CreateWindowExW(WS_EX_CLIENTEDGE, L"SynarithPreview", L"", WS_CHILD | WS_VISIBLE,
        0, 0, 1, 1, window, nullptr, instance, nullptr);
    layout(); ShowWindow(window, SW_SHOW); UpdateWindow(window);
    active = true; SetTimer(window, 1, 40, nullptr); handler(state, Ready);
    ACCEL shortcuts[] = {{FVIRTKEY | FCONTROL, 'O', 100}, {FVIRTKEY | FCONTROL, 'R', 101}, {FVIRTKEY, VK_ESCAPE, 103}};
    HACCEL accelerators = CreateAcceleratorTableW(shortcuts, 3);
    MSG message{}; int result;
    while ((result = GetMessageW(&message, nullptr, 0, 0)) > 0) {
        if (!TranslateAcceleratorW(window, accelerators, &message) && !IsDialogMessageW(window, &message)) {
            TranslateMessage(&message); DispatchMessageW(&message);
        }
    }
    active = false; DestroyAcceleratorTable(accelerators); DeleteObject(font); pixels.clear();
    return result == -1 ? 1 : 0;
}
extern "C" size_t syar_ui_get(int field, char *buffer, size_t capacity) {
    return field >= 0 && field < FieldCount ? copy_text(utf8(text(fields[field])), buffer, capacity) : 0;
}
extern "C" void syar_ui_set(int field, const char *value) {
    if (field < 0 || field >= FieldCount) return;
    changing = true;
    auto value_w = wide(value);
    if (field == Samples || field == Backend || field == Format) {
        LRESULT index = SendMessageW(fields[field], CB_FINDSTRINGEXACT, (WPARAM)-1, (LPARAM)value_w.c_str());
        if (index != CB_ERR) SendMessageW(fields[field], CB_SETCURSEL, index, 0);
    } else SetWindowTextW(fields[field], value_w.c_str());
    changing = false;
}
extern "C" void syar_ui_status(const char *message, bool busy) {
    SetWindowTextW(status, wide(message).c_str()); EnableWindow(cancel_button, busy);
}
extern "C" void syar_ui_log(const char *message) {
    std::wstring value = wide(message), normalized;
    for (wchar_t c : value) { if (c == L'\n') normalized += L'\r'; normalized += c; }
    SetWindowTextW(log_box, normalized.c_str());
}
extern "C" void syar_ui_image(const uint8_t *rgba, int width, int height) {
    image_width = width; image_height = height; pixels.resize((size_t)width * height * 4);
    for (size_t i = 0; i < pixels.size(); i += 4) {
        unsigned alpha = rgba[i + 3];
        for (int channel = 0; channel < 3; ++channel)
            pixels[i + channel] = (uint8_t)((rgba[i + 2 - channel] * alpha + 255 * (255 - alpha) + 127) / 255);
        pixels[i + 3] = 255;
    }
    InvalidateRect(preview, nullptr, FALSE);
}
extern "C" bool syar_ui_choose(bool save, const char *suggested, const char *extension, char *buffer, size_t capacity) {
    std::vector<wchar_t> path(32768, L'\0');
    std::wstring initial = wide(suggested); std::copy_n(initial.data(), std::min(initial.size(), path.size() - 1), path.data());
    std::wstring ext = wide(extension);
    std::wstring filter = save ? L"Image (*." + ext + L")" : L"Synarith documents (*.syar)";
    filter += L'\0'; filter += save ? L"*." + ext : L"*.syar"; filter += L'\0';
    filter += L"All files"; filter += L'\0'; filter += L"*.*"; filter += L'\0'; filter += L'\0';
    OPENFILENAMEW dialog{}; dialog.lStructSize = sizeof(dialog); dialog.hwndOwner = window;
    dialog.lpstrFile = path.data(); dialog.nMaxFile = (DWORD)path.size(); dialog.lpstrFilter = filter.c_str();
    dialog.lpstrDefExt = ext.c_str(); dialog.lpstrTitle = save ? L"Export Synarith image" : L"Open Synarith document";
    dialog.Flags = OFN_EXPLORER | OFN_NOCHANGEDIR | OFN_PATHMUSTEXIST | (save ? 0 : OFN_FILEMUSTEXIST);
    bool ok = save ? GetSaveFileNameW(&dialog) != 0 : GetOpenFileNameW(&dialog) != 0;
    if (!ok) return false;
    auto selected = utf8(path.data());
    if (selected.size() + 1 > capacity) return false;
    copy_text(selected, buffer, capacity); return true;
}
extern "C" bool syar_ui_confirm(const char *message) {
    return MessageBoxW(window, wide(message).c_str(), L"Synarith Viewer", MB_YESNO | MB_ICONQUESTION | MB_DEFBUTTON2) == IDYES;
}
extern "C" void syar_ui_quit() { PostMessageW(window, WM_CLOSE, 0, 0); }
extern "C" size_t syar_ui_capture(uint8_t *rgba, size_t capacity, int *width, int *height) {
    RECT bounds; GetWindowRect(window, &bounds); *width = bounds.right - bounds.left; *height = bounds.bottom - bounds.top;
    size_t required = (size_t)*width * *height * 4;
    if (!rgba) return required;
    if (capacity < required) return 0;
    HDC screen = GetDC(window), dc = CreateCompatibleDC(screen);
    BITMAPINFO info{}; info.bmiHeader.biSize = sizeof(BITMAPINFOHEADER); info.bmiHeader.biWidth = *width;
    info.bmiHeader.biHeight = -*height; info.bmiHeader.biPlanes = 1; info.bmiHeader.biBitCount = 32;
    void *data = nullptr;
    HBITMAP bitmap = CreateDIBSection(screen, &info, DIB_RGB_COLORS, &data, nullptr, 0);
    if (!bitmap) { DeleteDC(dc); ReleaseDC(window, screen); return 0; }
    HGDIOBJ previous = SelectObject(dc, bitmap);
    BOOL success = PrintWindow(window, dc, 2);
    if (success) {
        auto *bgra = static_cast<uint8_t *>(data);
        for (size_t i = 0; i < required; i += 4) {
            rgba[i] = bgra[i + 2]; rgba[i + 1] = bgra[i + 1]; rgba[i + 2] = bgra[i]; rgba[i + 3] = 255;
        }
    }
    SelectObject(dc, previous); DeleteObject(bitmap); DeleteDC(dc); ReleaseDC(window, screen);
    return success ? required : 0;
}
