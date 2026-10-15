// The C functions the benchmark's iOS shell calls: the benchmark's registration and
// gpui_mobile's entry point and lifecycle hooks, which exist in every gpux version it runs on.

#ifndef GPUX_BENCH_H
#define GPUX_BENCH_H

#ifdef __cplusplus
extern "C" {
#endif

void gpux_bench_register(void);

void gpui_ios_run_demo(void);

void gpui_ios_will_enter_foreground(void *app_ptr);
void gpui_ios_did_become_active(void *app_ptr);
void gpui_ios_will_resign_active(void *app_ptr);
void gpui_ios_did_enter_background(void *app_ptr);
void gpui_ios_will_terminate(void *app_ptr);

#ifdef __cplusplus
}
#endif

#endif
