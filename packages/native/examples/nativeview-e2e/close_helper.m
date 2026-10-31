// Test-only helper for the nativeview e2e, next to the webview e2e's `e2e_helper.m`: what that one does
// not do. It runs in the app's own process and touches only the app's own window.
//
// The "later" functions queue their work on the main queue, so that it runs while the app's run loop is
// pumped inside `tick()`, as input and a window close from the user do. Called straight from JS through
// bun:ffi, the same work runs outside `tick()`, where gpuix cannot call JS synchronously: a pointer handler
// could not run before the native view gets the press, and `onDetach` could not be delivered inside the
// close.
#import <AppKit/AppKit.h>

static NSInteger g_event_number = 1000000;

static NSWindow *app_window(void) {
  for (NSWindow *window in [NSApp windows]) {
    if ([window isVisible]) return window;
  }
  return nil;
}

static NSEvent *mouse_event(NSWindow *window, NSEventType type, double x, double y, int clicks) {
  NSView *content = [window contentView];
  NSPoint local = NSMakePoint(x, [content bounds].size.height - y);
  return [NSEvent mouseEventWithType:type
                            location:[content convertPoint:local toView:nil]
                       modifierFlags:0
                           timestamp:[NSProcessInfo processInfo].systemUptime
                        windowNumber:[window windowNumber]
                             context:nil
                         eventNumber:g_event_number++
                          clickCount:clicks
                            pressure:type == NSEventTypeLeftMouseDown ? 1.0f : 0.0f];
}

// How many visible windows the app has.
int nv_visible_windows(void) {
  int count = 0;
  for (NSWindow *window in [NSApp windows]) {
    if ([window isVisible]) count++;
  }
  return count;
}

// Closes the app's visible window the way a program does ([NSWindow close]: no delegate veto, the window
// is released and its views go), on the main queue. Returns 1 if there was a window to close.
int nv_close_window_later(void) {
  NSWindow *window = app_window();
  if (!window) return 0;
  dispatch_async(dispatch_get_main_queue(), ^{
    [window close];
  });
  return 1;
}

// Moves the pointer to a point of the content (points from the top left), on the main queue.
void nv_move_later(double x, double y) {
  dispatch_async(dispatch_get_main_queue(), ^{
    NSWindow *window = app_window();
    [window sendEvent:mouse_event(window, NSEventTypeMouseMoved, x, y, 0)];
  });
}

// A left click at a point of the content, on the main queue: move, press, release.
void nv_click_later(double x, double y) {
  dispatch_async(dispatch_get_main_queue(), ^{
    NSWindow *window = app_window();
    [window sendEvent:mouse_event(window, NSEventTypeMouseMoved, x, y, 0)];
    [window sendEvent:mouse_event(window, NSEventTypeLeftMouseDown, x, y, 1)];
    [window sendEvent:mouse_event(window, NSEventTypeLeftMouseUp, x, y, 1)];
  });
}

static int count_views(NSView *view) {
  int count = 1;
  for (NSView *subview in [view subviews]) count += count_views(subview);
  return count;
}

// How many views the app's window holds in all, the content view included: a native surface is among them
// from the frame that makes it.
int nv_view_count(void) {
  NSWindow *window = app_window();
  return window ? count_views([window contentView]) : 0;
}
