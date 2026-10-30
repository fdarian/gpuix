// Test-only helper for the webview e2e: it runs inside the app's own process, on the main thread
// (bun calls it through bun:ffi), and touches only the app's own window. It captures that window
// with CGWindowListCreateImage (a process may capture its own windows without the screen
// recording permission) and hands NSEvents to the window's sendEvent:. Nothing is posted to the
// system, and no other window or app is addressed.
#import <AppKit/AppKit.h>
#import <CoreGraphics/CoreGraphics.h>
#import <ImageIO/ImageIO.h>
#import <dlfcn.h>
#import <objc/runtime.h>

static NSWindow *g_window;
static CGImageRef g_image;
static NSInteger g_event_number = 1;

typedef CGImageRef (*CaptureFn)(CGRect, uint32_t, uint32_t, uint32_t);

// The window of the app: the first visible one.
int64_t e2e_find_window(void) {
  for (NSWindow *window in [NSApp windows]) {
    if ([window isVisible]) {
      g_window = window;
      // The app is this process's own: a page only follows the pointer in an active app.
      [NSApp activateIgnoringOtherApps:YES];
      [window makeKeyAndOrderFront:nil];
      return [window windowNumber];
    }
  }
  return 0;
}

double e2e_scale(void) { return [g_window backingScaleFactor]; }

// The content size of the window in points.
double e2e_content_width(void) { return [[g_window contentView] bounds].size.width; }
double e2e_content_height(void) { return [[g_window contentView] bounds].size.height; }

void e2e_set_content_size(double width, double height) {
  [g_window setContentSize:NSMakeSize(width, height)];
}

// Captures the composited window, the way WindowServer holds it. Returns 0 on success.
int e2e_capture(void) {
  static CaptureFn capture;
  if (!capture) capture = (CaptureFn)dlsym(RTLD_DEFAULT, "CGWindowListCreateImage");
  if (!capture) return 1;
  if (g_image) CGImageRelease(g_image);
  // kCGWindowListOptionIncludingWindow = 8, kCGWindowImageBoundsIgnoreFraming = 1 | bestResolution = 8
  g_image = capture(CGRectNull, 8, (uint32_t)[g_window windowNumber], 1 | 8);
  return g_image ? 0 : 2;
}

int e2e_image_width(void) { return g_image ? (int)CGImageGetWidth(g_image) : 0; }
int e2e_image_height(void) { return g_image ? (int)CGImageGetHeight(g_image) : 0; }

// Copies the capture as sRGB RGBA, top row first. `length` is the size of `out` in bytes.
int e2e_image_rgba(uint8_t *out, size_t length) {
  if (!g_image) return 1;
  size_t width = CGImageGetWidth(g_image), height = CGImageGetHeight(g_image);
  if (length < width * height * 4) return 2;
  CGColorSpaceRef space = CGColorSpaceCreateWithName(kCGColorSpaceSRGB);
  CGContextRef context = CGBitmapContextCreate(out, width, height, 8, width * 4, space,
                                               (CGBitmapInfo)kCGImageAlphaPremultipliedLast);
  CGColorSpaceRelease(space);
  if (!context) return 3;
  CGContextDrawImage(context, CGRectMake(0, 0, width, height), g_image);
  CGContextRelease(context);
  return 0;
}

int e2e_save_png(const char *path) {
  if (!g_image) return 1;
  NSURL *url = [NSURL fileURLWithPath:[NSString stringWithUTF8String:path]];
  CGImageDestinationRef destination =
      CGImageDestinationCreateWithURL((__bridge CFURLRef)url, CFSTR("public.png"), 1, NULL);
  if (!destination) return 2;
  CGImageDestinationAddImage(destination, g_image, NULL);
  bool written = CGImageDestinationFinalize(destination);
  CFRelease(destination);
  return written ? 0 : 3;
}

static NSPoint window_point(double x, double y) {
  NSView *content = [g_window contentView];
  NSPoint local = NSMakePoint(x, [content bounds].size.height - y);
  return [content convertPoint:local toView:nil];
}

// The WKWebView under a point of the window, or nil.
static NSView *page_under(NSPoint point) {
  NSView *content = [g_window contentView];
  NSView *hit = [content hitTest:[[content superview] convertPoint:point fromView:nil]];
  for (NSView *view = hit; view; view = [view superview]) {
    if ([NSStringFromClass([view class]) containsString:@"WebView"]) return view;
  }
  return nil;
}

// kind: 0 move, 1 left down, 2 left up, 3 left drag. x and y are from the top left of the content.
void e2e_mouse(int kind, double x, double y, int clicks) {
  NSEventType types[] = {NSEventTypeMouseMoved, NSEventTypeLeftMouseDown, NSEventTypeLeftMouseUp,
                         NSEventTypeLeftMouseDragged};
  NSPoint point = window_point(x, y);
  NSEvent *event = [NSEvent mouseEventWithType:types[kind]
                                      location:point
                                 modifierFlags:0
                                     timestamp:[NSProcessInfo processInfo].systemUptime
                                  windowNumber:[g_window windowNumber]
                                       context:nil
                                   eventNumber:g_event_number++
                                    clickCount:clicks
                                      pressure:kind == 1 || kind == 3 ? 1.0f : 0.0f];
  [g_window sendEvent:event];
  if (kind == 0) {
    // WebKit hears moves through its tracking area, which AppKit delivers to the view directly
    // and not through the window.
    NSView *page = page_under(point);
    if (page) [page mouseMoved:event];
  }
}

// A key press for the window, through the application as a typed key goes. `flags` are
// NSEventModifierFlags.
void e2e_key(int down, unsigned short key_code, const char *characters, const char *ignoring, uint64_t flags) {
  NSEvent *event = [NSEvent keyEventWithType:down ? NSEventTypeKeyDown : NSEventTypeKeyUp
                                    location:NSZeroPoint
                               modifierFlags:flags
                                   timestamp:[NSProcessInfo processInfo].systemUptime
                                windowNumber:[g_window windowNumber]
                                     context:nil
                                  characters:[NSString stringWithUTF8String:characters]
                 charactersIgnoringModifiers:[NSString stringWithUTF8String:ignoring]
                                   isARepeat:NO
                                     keyCode:key_code];
  [NSApp sendEvent:event];
}

// -- A wheel event made from a CGEvent belongs to no window and reports its location on the screen, so
// for the one event this helper sends, the three reads that place it in a window answer for it.
static NSEvent *g_wheel;
static NSPoint g_wheel_point;
static IMP g_location_in_window, g_window_number, g_window_of;

static NSPoint wheel_location(id self, SEL command) {
  if (self == g_wheel) return g_wheel_point;
  return ((NSPoint (*)(id, SEL))g_location_in_window)(self, command);
}
static NSInteger wheel_window_number(id self, SEL command) {
  if (self == g_wheel) return [g_window windowNumber];
  return ((NSInteger (*)(id, SEL))g_window_number)(self, command);
}
static NSWindow *wheel_window(id self, SEL command) {
  if (self == g_wheel) return g_window;
  return ((NSWindow * (*)(id, SEL))g_window_of)(self, command);
}

static void install_wheel_overrides(void) {
  if (g_location_in_window) return;
  g_location_in_window = method_setImplementation(class_getInstanceMethod([NSEvent class], @selector(locationInWindow)), (IMP)wheel_location);
  g_window_number = method_setImplementation(class_getInstanceMethod([NSEvent class], @selector(windowNumber)), (IMP)wheel_window_number);
  g_window_of = method_setImplementation(class_getInstanceMethod([NSEvent class], @selector(window)), (IMP)wheel_window);
}

// A pixel-unit scroll wheel event at a point of the content, as the system makes one: from a CGEvent.
// `reported` gets the location the event gives in its window (x, then y from the top) and its window
// number.
int e2e_wheel(double x, double y, double dy, double *reported) {
  install_wheel_overrides();
  CGEventRef cg = CGEventCreateScrollWheelEvent2(NULL, kCGScrollEventUnitPixel, 1, (int32_t)dy, 0, 0);
  NSEvent *event = [NSEvent eventWithCGEvent:cg];
  CFRelease(cg);
  if (!event) return 1;
  g_wheel = event;
  g_wheel_point = window_point(x, y);
  NSPoint reported_point = [event locationInWindow];
  reported[0] = reported_point.x;
  reported[1] = [[g_window contentView] bounds].size.height - ([[g_window contentView] convertPoint:reported_point fromView:nil]).y;
  reported[2] = (double)[event windowNumber];
  [g_window sendEvent:event];
  g_wheel = nil;
  return 0;
}

// 0 arrow, 1 I-beam, 2 pointing hand, 3 other.
int e2e_cursor(void) {
  NSCursor *cursor = [NSCursor currentCursor];
  if (cursor == [NSCursor arrowCursor]) return 0;
  if (cursor == [NSCursor IBeamCursor]) return 1;
  if (cursor == [NSCursor pointingHandCursor]) return 2;
  return 3;
}

// The system's cursor, as WindowServer holds it. 0 arrow, 1 I-beam, 2 pointing hand, 3 other.
int e2e_system_cursor(void) {
  NSCursor *cursor = [NSCursor currentSystemCursor];
  if (!cursor) return -1;
  if ([cursor isEqual:[NSCursor arrowCursor]]) return 0;
  if ([cursor isEqual:[NSCursor IBeamCursor]]) return 1;
  if ([cursor isEqual:[NSCursor pointingHandCursor]]) return 2;
  return 3;
}

// The class of the window's first responder.
int e2e_first_responder(char *out, size_t length) {
  id responder = [g_window firstResponder];
  const char *name = responder ? [NSStringFromClass([responder class]) UTF8String] : "(nil)";
  strncpy(out, name, length - 1);
  out[length - 1] = 0;
  return 0;
}

// Whether the window's first responder is the page of the webview whose rect (points from the top left of
// the content) contains the point, or a view inside it.
int e2e_page_is_first_responder(double x, double y) {
  NSView *page = page_under(window_point(x, y));
  if (!page) return -1;
  id responder = [g_window firstResponder];
  if (![responder isKindOfClass:[NSView class]]) return 0;
  return [(NSView *)responder isDescendantOf:page] ? 1 : 0;
}

// Whether the app is active and the window is key, for the log.
int e2e_active(void) { return ([NSApp isActive] ? 1 : 0) | ([g_window isKeyWindow] ? 2 : 0); }
