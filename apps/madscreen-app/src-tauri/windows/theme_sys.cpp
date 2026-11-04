// Process-local NSIS theme. Native controls retain their IDs, input and tab order.
#define UNICODE
#define _UNICODE
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <commctrl.h>
#include <dwmapi.h>
#include <objidl.h>
#include <gdiplus.h>
#include <uxtheme.h>
#include <vector>
#include <cwchar>

namespace {
constexpr UINT_PTR kSubclass = 0x4d53;
constexpr UINT kApplyMessage = WM_APP + 42;
bool applyPending;
bool applyingLayout;
constexpr wchar_t kHot[] = L"MadscreenThemeHot";
constexpr wchar_t kApplied[] = L"MadscreenThemeApplied";
HBRUSH background;
COLORREF bg = RGB(0, 0, 0), fg = RGB(250, 250, 250);
bool highContrast;
ULONG_PTR graphicsToken;
HWND rootWindow;
void layoutFooter(HWND hwnd);
struct Font { LONG height, weight; HFONT handle; };
std::vector<Font> fonts;

HFONT themedFont(HWND hwnd) {
    LOGFONTW lf{};
    auto current = reinterpret_cast<HFONT>(SendMessageW(hwnd, WM_GETFONT, 0, 0));
    if (!current || !GetObjectW(current, sizeof(lf), &lf)) {
        lf.lfHeight = -12;
        lf.lfWeight = FW_NORMAL;
    }
    // At least 10pt; body stays regular and headings keep their weight.
    HDC screen = GetDC(hwnd);
    const int dpi = GetDeviceCaps(screen, LOGPIXELSY);
    ReleaseDC(hwnd, screen);
    if (lf.lfHeight >= 0 || -lf.lfHeight < MulDiv(10, dpi, 72)) lf.lfHeight = -MulDiv(10, dpi, 72);
    for (const auto& font : fonts)
        if (font.height == lf.lfHeight && font.weight == lf.lfWeight) return font.handle;
    // Never inherit raster-font width/pitch from the NSIS dialog fallback.
    const LONG height = lf.lfHeight, weight = lf.lfWeight;
    lf = {};
    lf.lfHeight = height;
    lf.lfWeight = weight;
    lf.lfCharSet = DEFAULT_CHARSET;
    lf.lfOutPrecision = OUT_TT_ONLY_PRECIS;
    wcscpy_s(lf.lfFaceName, L"OpenAI Sans");
    lf.lfQuality = CLEARTYPE_QUALITY;
    HFONT handle = CreateFontIndirectW(&lf);
    if (handle) fonts.push_back({lf.lfHeight, lf.lfWeight, handle});
    return handle;
}

void paintButton(HWND hwnd, HDC dc) {
    RECT rc; GetClientRect(hwnd, &rc);
    const LONG style = GetWindowLongW(hwnd, GWL_STYLE);
    const UINT type = style & BS_TYPEMASK;
    const bool radio = type == BS_AUTORADIOBUTTON || type == BS_RADIOBUTTON;
    const bool check = type == BS_AUTOCHECKBOX || type == BS_CHECKBOX;
    const bool primary = GetDlgCtrlID(hwnd) == IDOK && !radio && !check;
    const bool disabled = !IsWindowEnabled(hwnd);
    const bool down = (SendMessageW(hwnd, BM_GETSTATE, 0, 0) & BST_PUSHED) != 0;
    const bool hover = GetPropW(hwnd, kHot) != nullptr;
    FillRect(dc, &rc, background);
    const int dpi = GetDeviceCaps(dc, LOGPIXELSX);
    auto px = [dpi](int v) { return MulDiv(v, dpi, 96); };
    RECT text = rc;
    COLORREF textColor = disabled ? RGB(130, 130, 130) : (primary ? RGB(0, 0, 0) : fg);
    if (radio || check) {
        const float side = static_cast<float>(px(16));
        const float x = static_cast<float>(px(2)), y = (static_cast<float>(rc.bottom) - side) / 2.0f;
        Gdiplus::Graphics graphics(dc);
        graphics.SetSmoothingMode(Gdiplus::SmoothingModeAntiAlias);
        const bool selected = SendMessageW(hwnd, BM_GETCHECK, 0, 0) == BST_CHECKED;
        const BYTE shade = disabled ? 100 : 245;
        Gdiplus::Pen outline(Gdiplus::Color(255, shade, shade, shade), static_cast<float>(px(1)));
        Gdiplus::SolidBrush white(Gdiplus::Color(255, shade, shade, shade));
        if (radio) {
            graphics.DrawEllipse(&outline, x, y, side, side);
            if (selected) graphics.FillEllipse(&white, x + side * .25f, y + side * .25f, side * .5f, side * .5f);
        } else {
            graphics.DrawRectangle(&outline, x, y, side, side);
            if (selected) {
                graphics.FillRectangle(&white, x, y, side, side);
                Gdiplus::Pen tick(Gdiplus::Color(255, 0, 0, 0), static_cast<float>(px(2)));
                graphics.DrawLine(&tick, x + side * .2f, y + side * .5f, x + side * .43f, y + side * .73f);
                graphics.DrawLine(&tick, x + side * .43f, y + side * .73f, x + side * .82f, y + side * .25f);
            }
        }
        text.left += px(28);
    } else {
        const COLORREF fillColor = disabled ? RGB(34, 34, 34) : primary
            ? (down ? RGB(190, 190, 190) : hover ? RGB(230, 230, 230) : fg)
            : (down ? RGB(50, 50, 50) : hover ? RGB(40, 40, 40) : RGB(20, 20, 20));
        HBRUSH fill = CreateSolidBrush(fillColor);
        HPEN pen = CreatePen(PS_SOLID, px(1), disabled ? RGB(52, 52, 52) : RGB(130, 130, 130));
        auto oldBrush = SelectObject(dc, fill); auto oldPen = SelectObject(dc, pen);
        RoundRect(dc, 0, 0, rc.right, rc.bottom, px(8), px(8));
        SelectObject(dc, oldBrush); SelectObject(dc, oldPen); DeleteObject(fill); DeleteObject(pen);
    }
    wchar_t label[512]; GetWindowTextW(hwnd, label, 512);
    auto oldFont = SelectObject(dc, reinterpret_cast<HFONT>(SendMessageW(hwnd, WM_GETFONT, 0, 0)));
    SetBkMode(dc, TRANSPARENT); SetTextColor(dc, textColor);
    const LRESULT cues = SendMessageW(hwnd, WM_QUERYUISTATE, 0, 0);
    DrawTextW(dc, label, -1, &text, DT_SINGLELINE | DT_VCENTER | (radio || check ? DT_LEFT : DT_CENTER)
        | ((cues & UISF_HIDEACCEL) ? DT_HIDEPREFIX : 0));
    SelectObject(dc, oldFont);
    if (GetFocus() == hwnd && !(cues & UISF_HIDEFOCUS)) {
        RECT focus = text; InflateRect(&focus, -px(3), -px(3));
        SetTextColor(dc, primary ? bg : fg); DrawFocusRect(dc, &focus);
    }
}

BOOL CALLBACK applyChild(HWND hwnd, LPARAM);

LRESULT CALLBACK controlProc(HWND hwnd, UINT msg, WPARAM wp, LPARAM lp, UINT_PTR, DWORD_PTR button) {
    // SAFETY: SetWindowSubclass binds this callback only to live controls on this UI thread.
    if (button && !highContrast) {
        if (msg == WM_PAINT) {
            PAINTSTRUCT ps; HDC dc = BeginPaint(hwnd, &ps);
            RECT rc; GetClientRect(hwnd, &rc);
            HDC buffer = CreateCompatibleDC(dc);
            HBITMAP bitmap = CreateCompatibleBitmap(dc, rc.right, rc.bottom);
            if (buffer && bitmap) {
                auto previous = SelectObject(buffer, bitmap);
                paintButton(hwnd, buffer);
                BitBlt(dc, 0, 0, rc.right, rc.bottom, buffer, 0, 0, SRCCOPY);
                SelectObject(buffer, previous);
            } else { paintButton(hwnd, dc); }
            if (bitmap) DeleteObject(bitmap);
            if (buffer) DeleteDC(buffer);
            EndPaint(hwnd, &ps); return 0;
        }
        if (msg == WM_PRINTCLIENT) { paintButton(hwnd, reinterpret_cast<HDC>(wp)); return 0; }
        if (msg == WM_ERASEBKGND) return 1;
        // Native BUTTON hover paints directly outside WM_PAINT. Do not let it
        // draw over the custom theme, and repaint only on enter/leave.
        if (msg == WM_MOUSEMOVE) {
            if (!GetPropW(hwnd, kHot)) {
                SetPropW(hwnd, kHot, reinterpret_cast<HANDLE>(1));
                TRACKMOUSEEVENT track{sizeof(track), TME_LEAVE, hwnd, 0}; TrackMouseEvent(&track);
                InvalidateRect(hwnd, nullptr, FALSE);
            }
            // Captured drag still needs native pressed-state/hit testing.
            if (GetCapture() != hwnd) return 0;
        }
        if (msg == WM_MOUSELEAVE) {
            if (RemovePropW(hwnd, kHot)) InvalidateRect(hwnd, nullptr, FALSE);
            if (GetCapture() != hwnd) return 0;
        }
        if (msg == WM_MOUSEHOVER) return 0;
    }
    if (msg == WM_CTLCOLORSTATIC || msg == WM_CTLCOLORDLG || msg == WM_CTLCOLOREDIT
        || msg == WM_CTLCOLORLISTBOX || msg == WM_CTLCOLORBTN) {
        HDC dc = reinterpret_cast<HDC>(wp); SetTextColor(dc, fg); SetBkColor(dc, bg);
        return reinterpret_cast<LRESULT>(background);
    }
    if (msg == WM_ERASEBKGND) {
        wchar_t cls[32]; GetClassNameW(hwnd, cls, 32);
        if (_wcsicmp(cls, L"#32770") == 0) { RECT rc; GetClientRect(hwnd, &rc); FillRect(reinterpret_cast<HDC>(wp), &rc, background); return 1; }
    }
    const LRESULT result = DefSubclassProc(hwnd, msg, wp, lp);
    if (button && (msg == WM_SETFOCUS || msg == WM_KILLFOCUS || msg == BM_SETCHECK
        || msg == BM_SETSTATE || msg == WM_ENABLE
        || msg == WM_UPDATEUISTATE)) InvalidateRect(hwnd, nullptr, FALSE);
    if (msg == WM_NCDESTROY) { RemovePropW(hwnd, kHot); RemovePropW(hwnd, kApplied); RemoveWindowSubclass(hwnd, controlProc, kSubclass); }
    if (!applyingLayout && (msg == WM_PARENTNOTIFY || msg == WM_SHOWWINDOW)) {
        if (!applyPending) { applyPending = true; PostMessageW(GetAncestor(hwnd, GA_ROOT), kApplyMessage, 0, 0); }
    }
    if (msg == kApplyMessage) {
        // Keep the notification gate closed until layout has finished.
        EnumChildWindows(hwnd, applyChild, 0); layoutFooter(hwnd); applyPending = false;
    }
    return result;
}

BOOL CALLBACK applyChild(HWND hwnd, LPARAM) {
    if (GetPropW(hwnd, kApplied)) return TRUE;
    SetPropW(hwnd, kApplied, reinterpret_cast<HANDLE>(1));
    wchar_t cls[64]; GetClassNameW(hwnd, cls, 64);
    if (_wcsicmp(cls, L"#32770") == 0) SetWindowLongW(hwnd, GWL_STYLE, GetWindowLongW(hwnd, GWL_STYLE) | WS_CLIPCHILDREN);
    const bool button = _wcsicmp(cls, L"Button") == 0 && (GetWindowLongW(hwnd, GWL_STYLE) & BS_TYPEMASK) != BS_GROUPBOX;
    SetWindowSubclass(hwnd, controlProc, kSubclass, button);
    if (!highContrast) SetWindowTheme(hwnd, L"", L"");
    if (auto font = themedFont(hwnd)) SendMessageW(hwnd, WM_SETFONT, reinterpret_cast<WPARAM>(font), TRUE);
    if (!highContrast && _wcsicmp(cls, PROGRESS_CLASSW) == 0) {
        SendMessageW(hwnd, PBM_SETBKCOLOR, 0, RGB(34, 34, 34));
        SendMessageW(hwnd, PBM_SETBARCOLOR, 0, fg);
    }
    InvalidateRect(hwnd, nullptr, TRUE);
    return TRUE;
}

void layoutFooter(HWND hwnd) {
    if (applyingLayout) return;
    applyingLayout = true;
    RECT rc; GetClientRect(hwnd, &rc);
    HDC dc = GetDC(hwnd); const int dpi = GetDeviceCaps(dc, LOGPIXELSX); ReleaseDC(hwnd, dc);
    const int gap = MulDiv(12, dpi, 96), width = MulDiv(100, dpi, 96);
    const int height = MulDiv(34, dpi, 96), margin = MulDiv(16, dpi, 96);
    const int ids[] = {IDCANCEL, IDOK, 3};
    for (int i = 0; i < 3; ++i) {
        HWND button = GetDlgItem(hwnd, ids[i]);
        if (button) SetWindowPos(button, nullptr, rc.right - margin - width - i * (width + gap),
            rc.bottom - margin - height, width, height, SWP_NOZORDER | SWP_NOACTIVATE);
    }
    HWND wordmark = GetDlgItem(hwnd, 19001);
    if (wordmark) {
        HWND bitmap = GetDlgItem(hwnd, 1046);
        if (bitmap && IsWindowVisible(bitmap)) ShowWindow(bitmap, SW_HIDE);
        SetWindowPos(wordmark, HWND_TOP, rc.right - MulDiv(164, dpi, 96), MulDiv(14, dpi, 96),
            MulDiv(140, dpi, 96), MulDiv(40, dpi, 96), SWP_NOACTIVATE);
        const bool show = IsWindowVisible(GetDlgItem(hwnd, 1037)) != FALSE;
        if ((IsWindowVisible(wordmark) != FALSE) != show) ShowWindow(wordmark, show ? SW_SHOWNA : SW_HIDE);
    }
    // Header title and subtitle share the same left edge in device pixels.
    HWND title = GetDlgItem(hwnd, 1037), subtitle = GetDlgItem(hwnd, 1038);
    if (title && subtitle) {
        RECT titleRect, subtitleRect;
        GetWindowRect(title, &titleRect); GetWindowRect(subtitle, &subtitleRect);
        MapWindowPoints(nullptr, hwnd, reinterpret_cast<POINT*>(&titleRect), 2);
        MapWindowPoints(nullptr, hwnd, reinterpret_cast<POINT*>(&subtitleRect), 2);
        const int left = MulDiv(24, dpi, 96);
        SetWindowPos(title, nullptr, left, titleRect.top,
            titleRect.right - left, titleRect.bottom - titleRect.top, SWP_NOZORDER | SWP_NOACTIVATE);
        SetWindowPos(subtitle, nullptr, left, subtitleRect.top,
            subtitleRect.right - left, subtitleRect.bottom - subtitleRect.top, SWP_NOZORDER | SWP_NOACTIVATE);
    }
    // NSIS branding is not product information and crowds the footer.
    for (int id : {1034, 1028, 1256}) { HWND label = GetDlgItem(hwnd, id); if (label && IsWindowVisible(label)) ShowWindow(label, SW_HIDE); }
    applyingLayout = false;
}



}

extern "C" __declspec(dllexport) void __cdecl Apply(HWND hwnd) {
    EnumChildWindows(hwnd, applyChild, 0);
    layoutFooter(hwnd);
}

extern "C" __declspec(dllexport) void __cdecl Initialize(HWND hwnd, const wchar_t* fontPath) {
    rootWindow = hwnd;
    Gdiplus::GdiplusStartupInput input;
    Gdiplus::GdiplusStartup(&graphicsToken, &input, nullptr);
    HIGHCONTRASTW contrast{sizeof(contrast)};
    SystemParametersInfoW(SPI_GETHIGHCONTRAST, sizeof(contrast), &contrast, 0);
    highContrast = (contrast.dwFlags & HCF_HIGHCONTRASTON) != 0;
    if (highContrast) { bg = GetSysColor(COLOR_WINDOW); fg = GetSysColor(COLOR_WINDOWTEXT); }
    background = CreateSolidBrush(bg);
    AddFontResourceExW(fontPath, FR_PRIVATE, nullptr);
    SetWindowSubclass(hwnd, controlProc, kSubclass, 0);
    if (auto font = themedFont(hwnd)) SendMessageW(hwnd, WM_SETFONT, reinterpret_cast<WPARAM>(font), TRUE);
    if (!highContrast) { BOOL dark = TRUE; DwmSetWindowAttribute(hwnd, 20, &dark, sizeof(dark)); }

    // Replace the bitmap header with an outline-font native text control.
    // MUI still owns its visibility when switching to/from full-window pages.
    HWND wordmark = CreateWindowExW(0, L"STATIC", L"madscreen",
        WS_CHILD | SS_RIGHT | SS_CENTERIMAGE | SS_NOPREFIX,
        0, 0, 0, 0, hwnd, reinterpret_cast<HMENU>(19001), GetModuleHandleW(nullptr), nullptr);
    if (wordmark) {
        HDC wordmarkDc = GetDC(hwnd);
        const int wordmarkDpi = GetDeviceCaps(wordmarkDc, LOGPIXELSY);
        ReleaseDC(hwnd, wordmarkDc);
        HFONT wordmarkFont = CreateFontW(-MulDiv(14, wordmarkDpi, 72), 0, 0, 0,
            FW_MEDIUM, FALSE, FALSE, FALSE, DEFAULT_CHARSET, OUT_TT_ONLY_PRECIS,
            CLIP_DEFAULT_PRECIS, CLEARTYPE_QUALITY, DEFAULT_PITCH, L"OpenAI Sans");
        if (wordmarkFont) {
            fonts.push_back({-MulDiv(14, wordmarkDpi, 72), FW_MEDIUM, wordmarkFont});
            SendMessageW(wordmark, WM_SETFONT, reinterpret_cast<WPARAM>(wordmarkFont), FALSE);
        }
    }
    Apply(hwnd);
    RedrawWindow(hwnd, nullptr, nullptr, RDW_INVALIDATE | RDW_ALLCHILDREN);
}
