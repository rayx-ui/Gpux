// The C entry points the shell calls: gpux-tests-conformance's `ios_host` example and gpui_mobile's
// iOS FFI (crates/gpui_mobile/src/ios/ffi.rs).

#ifndef GPUX_IOS_HOST_H
#define GPUX_IOS_HOST_H

#ifdef __cplusplus
extern "C" {
#endif

// Registers the conformance host as the app gpui_mobile runs.
void gpux_ios_host_register(void);

// Starts gpui_mobile's platform and application, which runs the registered host.
void gpui_ios_run_demo(void);

// Application lifecycle, forwarded to gpui_mobile's windows.
void gpui_ios_will_enter_foreground(void *app_ptr);
void gpui_ios_did_become_active(void *app_ptr);
void gpui_ios_will_resign_active(void *app_ptr);
void gpui_ios_did_enter_background(void *app_ptr);
void gpui_ios_did_receive_memory_warning(void *app_ptr);
void gpui_ios_will_terminate(void *app_ptr);

#ifdef __cplusplus
}
#endif

#endif
