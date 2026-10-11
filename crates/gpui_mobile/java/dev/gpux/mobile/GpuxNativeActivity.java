/*
 * Copyright 2026 gpux contributors.
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *     http://www.apache.org/licenses/LICENSE-2.0
 */
package dev.gpux.mobile;

import android.app.NativeActivity;

/**
 * A {@code NativeActivity} that tells gpux about {@code onTrimMemory}.
 *
 * <p>{@code android-activity} forwards {@code onLowMemory} only, and {@code onTrimMemory} has no
 * native callback of its own, so a NativeActivity host that wants its windows to give their
 * scroll-layer tiles back on every trim level names this class as its activity
 * ({@code android:name="dev.gpux.mobile.GpuxNativeActivity"}). A host that is not built with these
 * helpers keeps {@code onLowMemory}, pause and surface loss, which release the tiles as well.
 *
 * <p>The native method is registered by gpux when its platform starts, which is after the
 * activity could first be trimmed: until then the call is skipped.
 */
public class GpuxNativeActivity extends NativeActivity {
    /** Set by gpux once {@link #nativeTrimMemory} is registered. */
    private static volatile boolean nativesReady;

    @Override
    public void onTrimMemory(int level) {
        super.onTrimMemory(level);
        if (nativesReady) {
            nativeTrimMemory(level);
        }
    }

    private static native void nativeTrimMemory(int level);
}
