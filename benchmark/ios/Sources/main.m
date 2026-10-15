// The benchmark's UIKit shell: registers the benchmark with gpui_mobile, starts it, and forwards
// the application lifecycle. The benchmark reads its run from the launch arguments, prints its
// report on stdout and exits.

#import <UIKit/UIKit.h>

#import "gpux_bench.h"

@interface GpuxBenchDelegate : UIResponder <UIApplicationDelegate>
@property(nonatomic, strong) UIWindow *window;
@end

@implementation GpuxBenchDelegate

- (BOOL)application:(UIApplication *)application
    didFinishLaunchingWithOptions:(NSDictionary *)launchOptions {
    gpux_bench_register();
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
        return UIApplicationMain(argc, argv, nil, NSStringFromClass([GpuxBenchDelegate class]));
    }
}
