// Lane F's UIKit shell: starts gpui_mobile with the conformance host registered, and forwards the
// application lifecycle. gpui_mobile's windows create their own UIWindow, Metal view and display
// link, and receive their touches directly.

#import <UIKit/UIKit.h>

#import "gpux_ios_host.h"

@interface GpuxAppDelegate : UIResponder <UIApplicationDelegate>
@property(nonatomic, strong) UIWindow *window;
@end

@implementation GpuxAppDelegate

- (BOOL)application:(UIApplication *)application
    didFinishLaunchingWithOptions:(NSDictionary *)launchOptions {
    gpux_ios_host_register();
    gpui_ios_run_demo();
    return YES;
}

- (void)applicationWillEnterForeground:(UIApplication *)application {
    gpui_ios_will_enter_foreground(NULL);
}

- (void)applicationDidBecomeActive:(UIApplication *)application {
    gpui_ios_did_become_active(NULL);
}

- (void)applicationWillResignActive:(UIApplication *)application {
    gpui_ios_will_resign_active(NULL);
}

- (void)applicationDidEnterBackground:(UIApplication *)application {
    gpui_ios_did_enter_background(NULL);
}

- (void)applicationWillTerminate:(UIApplication *)application {
    gpui_ios_will_terminate(NULL);
}

@end

int main(int argc, char *argv[]) {
    @autoreleasepool {
        return UIApplicationMain(argc, argv, nil, NSStringFromClass([GpuxAppDelegate class]));
    }
}
