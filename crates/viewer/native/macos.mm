#import <AppKit/AppKit.h>
#import <CoreGraphics/CoreGraphics.h>
#include "widgets.h"

static NSWindow *window;
static NSControl *fields[FieldCount];
static NSTextField *status;
static NSTextView *log_box;
static NSImageView *preview;
static NSButton *cancel_button;
static Callback handler;
static void *state;
static bool active = false, changing = false;
static bool ready = false;
static NSString *text(const char *value) { return [NSString stringWithUTF8String:value] ?: @""; }

@interface SyarDelegate : NSObject <NSApplicationDelegate, NSWindowDelegate, NSTextFieldDelegate>
- (void)action:(id)sender;
- (void)choice:(id)sender;
- (void)tick:(NSTimer *)timer;
@end
@implementation SyarDelegate
- (void)action:(id)sender { if (active) handler(state, (int)[sender tag]); }
- (void)tick:(NSTimer *)timer {
    (void)timer;
    if (active) { bool first = !ready; ready = true; handler(state, first ? Ready : Tick); }
}
- (void)controlTextDidChange:(NSNotification *)notification {
    (void)notification; if (active && !changing) handler(state, Changed);
}
- (void)choice:(id)sender { (void)sender; if (active && !changing) handler(state, Changed); }
- (void)windowWillClose:(NSNotification *)notification {
    (void)notification; active = false; handler(state, CloseAction); [NSApp stop:nil];
    // Wake NSApplication's event loop when stopped outside a user input event.
    [NSApp postEvent:[NSEvent otherEventWithType:NSEventTypeApplicationDefined location:NSZeroPoint
        modifierFlags:0 timestamp:0 windowNumber:0 context:nil subtype:0 data1:0 data2:0] atStart:NO];
}
- (NSApplicationTerminateReply)applicationShouldTerminate:(NSApplication *)app {
    (void)app; [window performClose:nil]; return NSTerminateCancel;
}
@end
static SyarDelegate *delegate;
static NSTextField *label(NSString *value, NSRect frame, NSView *parent) {
    NSTextField *result = [NSTextField labelWithString:value]; result.frame = frame; [parent addSubview:result]; return result;
}
static NSTextField *edit(int field, NSString *value, NSRect frame, NSView *parent) {
    NSTextField *result = [[NSTextField alloc] initWithFrame:frame]; result.stringValue = value;
    result.delegate = delegate; result.font = [NSFont systemFontOfSize:13];
    fields[field] = result; [parent addSubview:result]; return result;
}
static NSPopUpButton *choice(int field, NSArray<NSString *> *values, NSRect frame, NSView *parent) {
    NSPopUpButton *result = [[NSPopUpButton alloc] initWithFrame:frame pullsDown:NO];
    [result addItemsWithTitles:values]; result.target = delegate; result.action = @selector(choice:);
    fields[field] = result; [parent addSubview:result]; return result;
}
static NSButton *button(NSString *name, int action, NSRect frame, NSView *parent) {
    NSButton *result = [NSButton buttonWithTitle:name target:delegate action:@selector(action:)];
    result.tag = action; result.frame = frame; [parent addSubview:result]; return result;
}
extern "C" int syar_ui_run(Callback callback, void *context, const char *source) {
    @autoreleasepool {
        handler = callback; state = context;
        [NSApplication sharedApplication]; [NSApp setActivationPolicy:NSApplicationActivationPolicyRegular];
        delegate = [SyarDelegate new]; NSApp.delegate = delegate;
        window = [[NSWindow alloc] initWithContentRect:NSMakeRect(0, 0, 1200, 780)
            styleMask:NSWindowStyleMaskTitled | NSWindowStyleMaskClosable | NSWindowStyleMaskMiniaturizable | NSWindowStyleMaskResizable
            backing:NSBackingStoreBuffered defer:NO];
        window.title = @"Synarith Viewer"; window.minSize = NSMakeSize(950, 720);
        window.delegate = delegate; window.releasedWhenClosed = NO;
        NSView *root = window.contentView;
        NSTextField *document = label(@"Document", NSMakeRect(16, 738, 80, 24), root);
        document.autoresizingMask = NSViewMinYMargin;
        NSTextField *source_field = edit(Source, text(source), NSMakeRect(100, 738, 992, 26), root);
        source_field.autoresizingMask = NSViewWidthSizable | NSViewMinYMargin;
        NSButton *open = button(@"Open…", OpenAction, NSMakeRect(1100, 736, 84, 30), root);
        open.autoresizingMask = NSViewMinXMargin | NSViewMinYMargin;
        NSView *panel = [[NSView alloc] initWithFrame:NSMakeRect(16, 16, 320, 700)];
        panel.autoresizingMask = NSViewHeightSizable; [root addSubview:panel];
        NSArray<NSString *> *names = @[@"Width", @"Height", @"Time (seconds)", @"Samples / axis", @"Backend",
            @"Output name", @"Overrides (;)", @"Format", @"JPEG quality", @"Seed (optional)"];
        NSArray<NSString *> *defaults = @[@"1024", @"1024", @"0", @"1", @"auto", @"main", @"", @"png", @"90", @""];
        for (int i = Width; i < FieldCount; ++i) {
            CGFloat y = 662 - (i - Width) * 34;
            NSTextField *caption = label(names[i - Width], NSMakeRect(0, y + 3, 110, 22), panel);
            caption.autoresizingMask = NSViewMinYMargin;
            NSRect frame = NSMakeRect(112, y, 204, 26);
            NSControl *input;
            if (i == Samples) input = choice(i, @[@"1", @"2", @"4", @"8"], frame, panel);
            else if (i == Backend) input = choice(i, @[@"auto", @"cpu", @"gpu"], frame, panel);
            else if (i == Format) input = choice(i, @[@"png", @"jpeg", @"bmp", @"tga", @"ppm"], frame, panel);
            else input = edit(i, defaults[i - Width], frame, panel);
            input.autoresizingMask = NSViewMinYMargin;
        }
        NSButton *render = button(@"Preview", PreviewAction, NSMakeRect(0, 300, 100, 30), panel);
        NSButton *export_button = button(@"Export…", ExportAction, NSMakeRect(106, 300, 100, 30), panel);
        cancel_button = button(@"Cancel", CancelAction, NSMakeRect(212, 300, 104, 30), panel);
        cancel_button.enabled = NO; cancel_button.keyEquivalent = @"\033"; cancel_button.keyEquivalentModifierMask = 0;
        render.autoresizingMask = export_button.autoresizingMask = cancel_button.autoresizingMask = NSViewMinYMargin;
        status = [NSTextField wrappingLabelWithString:@"Choose a document. All rendering runs through syar."];
        status.frame = NSMakeRect(0, 218, 316, 70); status.selectable = YES;
        status.autoresizingMask = NSViewMinYMargin; [panel addSubview:status];
        NSScrollView *scroll = [[NSScrollView alloc] initWithFrame:NSMakeRect(0, 0, 316, 208)];
        scroll.hasVerticalScroller = YES; scroll.borderType = NSBezelBorder; scroll.autoresizingMask = NSViewHeightSizable;
        log_box = [[NSTextView alloc] initWithFrame:NSMakeRect(0, 0, 296, 208)];
        log_box.editable = NO; log_box.richText = NO; log_box.font = [NSFont systemFontOfSize:12];
        log_box.minSize = NSMakeSize(0, 208); log_box.maxSize = NSMakeSize(CGFLOAT_MAX, CGFLOAT_MAX);
        log_box.verticallyResizable = YES; log_box.horizontallyResizable = NO;
        log_box.autoresizingMask = NSViewWidthSizable; log_box.textContainer.widthTracksTextView = YES;
        scroll.documentView = log_box; [panel addSubview:scroll];
        preview = [[NSImageView alloc] initWithFrame:NSMakeRect(352, 16, 832, 700)];
        preview.imageScaling = NSImageScaleProportionallyUpOrDown; preview.imageAlignment = NSImageAlignCenter;
        preview.autoresizingMask = NSViewWidthSizable | NSViewHeightSizable; [root addSubview:preview];
        NSMenu *menu = [NSMenu new]; NSMenuItem *app_item = [NSMenuItem new]; [menu addItem:app_item];
        NSMenu *app_menu = [NSMenu new]; app_item.submenu = app_menu;
        [app_menu addItemWithTitle:@"Quit Synarith Viewer" action:@selector(terminate:) keyEquivalent:@"q"];
        NSMenuItem *file_item = [NSMenuItem new]; file_item.title = @"File"; [menu addItem:file_item];
        NSMenu *file_menu = [[NSMenu alloc] initWithTitle:@"File"]; file_item.submenu = file_menu;
        for (NSArray *item in @[@[@"Open…", @"o", @(OpenAction)], @[@"Preview", @"r", @(PreviewAction)], @[@"Export…", @"s", @(ExportAction)]]) {
            NSMenuItem *entry = [file_menu addItemWithTitle:item[0] action:@selector(action:) keyEquivalent:item[1]];
            entry.tag = [item[2] integerValue]; entry.target = delegate;
        }
        NSMenuItem *edit_item = [NSMenuItem new]; edit_item.title = @"Edit"; [menu addItem:edit_item];
        NSMenu *edit_menu = [[NSMenu alloc] initWithTitle:@"Edit"]; edit_item.submenu = edit_menu;
        [edit_menu addItemWithTitle:@"Cut" action:@selector(cut:) keyEquivalent:@"x"];
        [edit_menu addItemWithTitle:@"Copy" action:@selector(copy:) keyEquivalent:@"c"];
        [edit_menu addItemWithTitle:@"Paste" action:@selector(paste:) keyEquivalent:@"v"];
        [edit_menu addItemWithTitle:@"Select All" action:@selector(selectAll:) keyEquivalent:@"a"];
        NSApp.mainMenu = menu;
        [window center]; [window makeKeyAndOrderFront:nil]; [NSApp activateIgnoringOtherApps:YES];
        active = true; ready = false;
        NSTimer *timer = [NSTimer timerWithTimeInterval:0.04 target:delegate selector:@selector(tick:) userInfo:nil repeats:YES];
        [[NSRunLoop mainRunLoop] addTimer:timer forMode:NSRunLoopCommonModes];
        [NSApp run];
        active = false; [timer invalidate]; preview = nil; log_box = nil; status = nil;
        for (auto &field : fields) field = nil;
        window = nil; NSApp.delegate = nil; delegate = nil;
        return 0;
    }
}
extern "C" size_t syar_ui_get(int field, char *buffer, size_t capacity) {
    if (field < 0 || field >= FieldCount) return 0;
    NSControl *input = fields[field];
    NSString *value = [input isKindOfClass:NSPopUpButton.class] ? [(NSPopUpButton *)input titleOfSelectedItem] : input.stringValue;
    return copy_text(value.UTF8String ?: "", buffer, capacity);
}
extern "C" void syar_ui_set(int field, const char *value) {
    if (field < 0 || field >= FieldCount) return;
    changing = true;
    NSControl *input = fields[field];
    if ([input isKindOfClass:NSPopUpButton.class]) [(NSPopUpButton *)input selectItemWithTitle:text(value)];
    else input.stringValue = text(value);
    changing = false;
}
extern "C" void syar_ui_status(const char *message, bool busy) { status.stringValue = text(message); cancel_button.enabled = busy; }
extern "C" void syar_ui_log(const char *message) { log_box.string = text(message); }
extern "C" void syar_ui_image(const uint8_t *rgba, int width, int height) {
    NSBitmapImageRep *bitmap = [[NSBitmapImageRep alloc] initWithBitmapDataPlanes:nullptr pixelsWide:width pixelsHigh:height
        bitsPerSample:8 samplesPerPixel:4 hasAlpha:YES isPlanar:NO colorSpaceName:NSDeviceRGBColorSpace
        bitmapFormat:NSBitmapFormatAlphaNonpremultiplied bytesPerRow:width * 4 bitsPerPixel:32];
    std::memcpy(bitmap.bitmapData, rgba, (size_t)width * height * 4);
    NSImage *image = [[NSImage alloc] initWithSize:NSMakeSize(width, height)]; [image addRepresentation:bitmap]; preview.image = image;
}
extern "C" bool syar_ui_choose(bool save, const char *suggested, const char *extension, char *buffer, size_t capacity) {
    (void)extension;
    NSSavePanel *panel = save ? [NSSavePanel savePanel] : [NSOpenPanel openPanel];
    panel.title = save ? @"Export Synarith image" : @"Open Synarith document";
    panel.canCreateDirectories = save;
    NSString *path = text(suggested);
    panel.nameFieldStringValue = path.lastPathComponent;
    NSString *directory = path.stringByDeletingLastPathComponent;
    if (directory.length) panel.directoryURL = [NSURL fileURLWithPath:directory isDirectory:YES];
    if (!save) { NSOpenPanel *open = (NSOpenPanel *)panel; open.canChooseDirectories = NO; open.allowsMultipleSelection = NO; }
    if ([panel runModal] != NSModalResponseOK) return false;
    std::string selected(panel.URL.path.UTF8String ?: "");
    if (selected.empty() || selected.size() + 1 > capacity) return false;
    copy_text(selected, buffer, capacity); return true;
}
extern "C" bool syar_ui_confirm(const char *message) {
    NSAlert *dialog = [NSAlert new]; dialog.messageText = @"Synarith Viewer"; dialog.informativeText = text(message);
    [dialog addButtonWithTitle:@"Cancel"]; [dialog addButtonWithTitle:@"Replace"]; return [dialog runModal] == NSAlertSecondButtonReturn;
}
extern "C" void syar_ui_quit() { [window performClose:nil]; }
extern "C" size_t syar_ui_capture(uint8_t *rgba, size_t capacity, int *width, int *height) {
    NSView *view = window.contentView;
    NSBitmapImageRep *bitmap = [view bitmapImageRepForCachingDisplayInRect:view.bounds];
    [view cacheDisplayInRect:view.bounds toBitmapImageRep:bitmap];
    CGImageRef image = bitmap.CGImage;
    *width = (int)CGImageGetWidth(image); *height = (int)CGImageGetHeight(image);
    size_t size = (size_t)*width * *height * 4;
    if (!rgba) return size;
    if (capacity < size) return 0;
    CGColorSpaceRef color = CGColorSpaceCreateWithName(kCGColorSpaceSRGB);
    CGContextRef dc = CGBitmapContextCreate(rgba, *width, *height, 8, (size_t)*width * 4, color,
        kCGImageAlphaPremultipliedLast | kCGBitmapByteOrder32Big);
    if (!dc) { CGColorSpaceRelease(color); return 0; }
    CGContextTranslateCTM(dc, 0, *height); CGContextScaleCTM(dc, 1, -1);
    CGContextDrawImage(dc, CGRectMake(0, 0, *width, *height), image);
    CGContextRelease(dc); CGColorSpaceRelease(color); return size;
}
