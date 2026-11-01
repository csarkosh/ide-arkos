// A display for a machine that has none worth drawing on: a virtual display at two pixels a
// point, made the main display for as long as this runs. arkos-shoot builds it with clang once
// a run and starts it on a machine whose main display draws one pixel a point (a GitHub
// runner's 1024 by 768 has no HiDPI mode), so the window, the stills and the recording come out
// at a Retina Mac's density.
//
//   vdisplay <width> <height>        points; the display is 2*width by 2*height pixels
//
// It prints one line once the display is up and main:
//
//   ready <display id> <width> <height> <left> <top> <right> <bottom>
//
// the last four the visible area's edges in points from the display's top-left corner (the
// menu bar and the Dock leave it; arkos-shoot centres the window inside it), then waits. SIGINT
// or SIGTERM ends it, and the display goes away with the process; macOS puts the other displays
// back as they were.
//
// CGVirtualDisplay is CoreGraphics' private API for exactly this (DeskPad and BetterDisplay use
// it); outside the App Sandbox it needs no entitlement. The declarations below are the parts
// used, as the headers reverse-engineered for those apps give them.
#import <Cocoa/Cocoa.h>
#import <CoreGraphics/CoreGraphics.h>
#import <signal.h>

@interface CGVirtualDisplayMode : NSObject
- (instancetype)initWithWidth:(NSUInteger)width height:(NSUInteger)height refreshRate:(CGFloat)refreshRate;
@end

@interface CGVirtualDisplaySettings : NSObject
@property(retain, nonatomic) NSArray<CGVirtualDisplayMode *> *modes;
@property(nonatomic) unsigned int hiDPI;
@end

@class CGVirtualDisplay;

@interface CGVirtualDisplayDescriptor : NSObject
@property(retain, nonatomic) dispatch_queue_t queue;
@property(retain, nonatomic) NSString *name;
@property(nonatomic) unsigned int maxPixelsHigh;
@property(nonatomic) unsigned int maxPixelsWide;
@property(nonatomic) CGSize sizeInMillimeters;
@property(nonatomic) unsigned int serialNum;
@property(nonatomic) unsigned int productID;
@property(nonatomic) unsigned int vendorID;
@property(copy, nonatomic) void (^terminationHandler)(id, CGVirtualDisplay *);
@end

@interface CGVirtualDisplay : NSObject
@property(readonly, nonatomic) CGDirectDisplayID displayID;
- (instancetype)initWithDescriptor:(CGVirtualDisplayDescriptor *)descriptor;
- (BOOL)applySettings:(CGVirtualDisplaySettings *)settings;
@end

static void fail(NSString *message) {
    fprintf(stderr, "vdisplay: %s\n", message.UTF8String);
    exit(2);
}

static volatile sig_atomic_t stopping = 0;
static void onSignal(int sig) { (void)sig; stopping = 1; }

/// The display's visible area (what the menu bar and the Dock leave), in points from its
/// top-left corner, once AppKit lists it; the whole display when AppKit does not yet.
static BOOL visibleArea(CGDirectDisplayID display, CGFloat *left, CGFloat *top, CGFloat *right, CGFloat *bottom) {
    for (NSScreen *screen in NSScreen.screens) {
        NSNumber *number = screen.deviceDescription[@"NSScreenNumber"];
        if (number.unsignedIntValue != display) continue;
        NSRect f = screen.frame, v = screen.visibleFrame;
        // AppKit's origin is the bottom-left corner.
        *left = v.origin.x - f.origin.x;
        *top = (f.origin.y + f.size.height) - (v.origin.y + v.size.height);
        *right = *left + v.size.width;
        *bottom = *top + v.size.height;
        return YES;
    }
    return NO;
}

int main(int argc, const char *argv[]) {
    @autoreleasepool {
        if (argc != 3) fail(@"usage: vdisplay <width> <height>");
        unsigned width = (unsigned)atoi(argv[1]), height = (unsigned)atoi(argv[2]);
        if (width < 800 || width > 3840 || height < 600 || height > 2400) fail(@"width 800 to 3840 and height 600 to 2400, in points");

        CGVirtualDisplayDescriptor *descriptor = [[CGVirtualDisplayDescriptor alloc] init];
        descriptor.name = @"Arkos Shoot";
        descriptor.queue = dispatch_get_main_queue();
        descriptor.maxPixelsWide = width * 2;
        descriptor.maxPixelsHigh = height * 2;
        // About a 14-inch panel's size at this many points, so macOS treats it as a laptop display.
        descriptor.sizeInMillimeters = CGSizeMake(width * 0.2146, height * 0.2146);
        descriptor.productID = 0x4152;
        descriptor.vendorID = 0x4b4f;
        descriptor.serialNum = 1;
        descriptor.terminationHandler = ^(id sender, CGVirtualDisplay *display) {
            (void)sender; (void)display;
            fprintf(stderr, "vdisplay: the display was ended by the system\n");
            exit(3);
        };
        CGVirtualDisplay *display = [[CGVirtualDisplay alloc] initWithDescriptor:descriptor];
        if (!display) fail(@"could not create a virtual display");
        CGVirtualDisplaySettings *settings = [[CGVirtualDisplaySettings alloc] init];
        settings.hiDPI = 1;
        settings.modes = @[[[CGVirtualDisplayMode alloc] initWithWidth:width height:height refreshRate:60]];
        if (![display applySettings:settings]) fail(@"the display refused its mode");
        CGDirectDisplayID id = display.displayID;

        // The display takes a moment to come online; then it becomes main by taking the origin,
        // every other display moved to its right (the main display is the one at the origin).
        NSDate *deadline = [NSDate dateWithTimeIntervalSinceNow:15];
        for (;;) {
            if (CGDisplayIsOnline(id) && CGDisplayPixelsWide(id) == width) break;
            if ([deadline timeIntervalSinceNow] < 0) fail(@"the virtual display did not come online");
            [NSRunLoop.mainRunLoop runUntilDate:[NSDate dateWithTimeIntervalSinceNow:0.2]];
        }
        CGDirectDisplayID others[16];
        uint32_t count = 0;
        CGGetOnlineDisplayList(16, others, &count);
        CGDisplayConfigRef config = NULL;
        if (CGBeginDisplayConfiguration(&config) != kCGErrorSuccess) fail(@"could not begin a display configuration");
        CGConfigureDisplayOrigin(config, id, 0, 0);
        int32_t x = (int32_t)width;
        for (uint32_t i = 0; i < count; i++) {
            if (others[i] == id) continue;
            CGConfigureDisplayOrigin(config, others[i], x, 0);
            x += (int32_t)CGDisplayPixelsWide(others[i]);
        }
        CGError err = CGCompleteDisplayConfiguration(config, kCGConfigureForSession);
        if (err != kCGErrorSuccess) fail([NSString stringWithFormat:@"could not make the virtual display main (%d)", err]);
        deadline = [NSDate dateWithTimeIntervalSinceNow:10];
        while (CGMainDisplayID() != id) {
            if ([deadline timeIntervalSinceNow] < 0) fail(@"the virtual display did not become main");
            [NSRunLoop.mainRunLoop runUntilDate:[NSDate dateWithTimeIntervalSinceNow:0.2]];
        }
        // AppKit learns the new arrangement through the run loop; give it a moment so the
        // visible area (the menu bar, the Dock) is the new display's.
        [NSRunLoop.mainRunLoop runUntilDate:[NSDate dateWithTimeIntervalSinceNow:1.0]];
        CGFloat left = 0, top = 25, right = width, bottom = height;
        visibleArea(id, &left, &top, &right, &bottom);
        printf("ready %u %u %u %.0f %.0f %.0f %.0f\n", id, width, height, left, top, right, bottom);
        fflush(stdout);

        signal(SIGINT, onSignal);
        signal(SIGTERM, onSignal);
        while (!stopping) {
            [NSRunLoop.mainRunLoop runUntilDate:[NSDate dateWithTimeIntervalSinceNow:0.25]];
        }
        // `display` is released on exit, and the display goes with it.
        (void)display;
    }
    return 0;
}
