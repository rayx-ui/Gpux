package dev.gpux.mobile;

import android.app.Activity;
import android.content.Context;
import android.graphics.Canvas;
import android.graphics.PixelFormat;
import android.os.Handler;
import android.os.IBinder;
import android.os.Looper;
import android.view.Gravity;
import android.view.MotionEvent;
import android.view.Surface;
import android.view.SurfaceHolder;
import android.view.SurfaceView;
import android.view.View;
import android.view.ViewGroup;
import android.view.WindowManager;
import android.widget.FrameLayout;
import java.util.ArrayList;
import java.util.List;

/**
 * gpux window composition on Android (zed-industries/zed#62379): native views between GPUI's
 * base content and its overlay.
 *
 * <p>GPUI draws its base content into the activity's own window (a NativeActivity's surface, or
 * the embedding host's SurfaceView), where Android views cannot draw. So each native surface is a
 * container in a panel window attached to the activity's window, and the overlay is a translucent
 * sub-panel window above every panel, holding a SurfaceView that GPUI renders into. This serves
 * the NativeActivity host and the embedded host alike.
 *
 * <p>The overlay takes no touches while GPUI drew nothing on it, so they reach the native views
 * and the activity's window below; while it has content it takes them all and hands them to GPUI
 * as raw touches.
 *
 * <p>Where GPUI renders through GLES, whose surfaces wgpu presents only opaque, there is no
 * overlay window: GPUI draws its overlay content into the activity's window and each container
 * clips that content out of itself ({@link #setHoles}), handing touches in the holes to GPUI.
 *
 * <p>Rust calls every method from GPUI's thread; window changes are posted to the UI thread.
 * The native methods are registered by gpux when it loads this class. Consumers add these
 * sources to their build, as {@code gpux-testkit android build} does without Gradle.
 */
public final class Composition {
    private final Activity activity;
    /** The Rust state the native callbacks reach. */
    private final long handle;
    /** Whether GPUI's overlay content goes into an overlay window, or through holes. */
    private final boolean overlayWindow;
    private final Handler ui = new Handler(Looper.getMainLooper());
    /** The panel windows mounted, bottom to top. */
    private final List<FrameLayout> mounted = new ArrayList<>();
    private FrameLayout overlay;
    private WindowManager.LayoutParams overlayParams;
    private boolean overlayTouchable;
    private boolean released;

    public Composition(Activity activity, long handle, boolean overlayWindow) {
        this.activity = activity;
        this.handle = handle;
        this.overlayWindow = overlayWindow;
    }

    /**
     * A native surface's container: it clips its holes (window rectangles of GPUI content above
     * it) out of what it draws, and hands touches that start in one to GPUI.
     */
    static final class Container extends FrameLayout {
        private final long handle;
        private float[] holes = new float[0];
        private boolean forwarding;

        Container(Context context, long handle) {
            super(context);
            this.handle = handle;
            setClipChildren(true);
        }

        private int left() {
            return ((WindowManager.LayoutParams) getLayoutParams()).x;
        }

        private int top() {
            return ((WindowManager.LayoutParams) getLayoutParams()).y;
        }

        void setHoles(float[] holes) {
            this.holes = holes;
            invalidate();
        }

        private boolean inHole(float x, float y) {
            for (int index = 0; index + 3 < holes.length; index += 4) {
                float left = holes[index] - left();
                float top = holes[index + 1] - top();
                if (x >= left && y >= top && x < left + holes[index + 2] && y < top + holes[index + 3]) {
                    return true;
                }
            }
            return false;
        }

        @Override
        protected void dispatchDraw(Canvas canvas) {
            canvas.save();
            for (int index = 0; index + 3 < holes.length; index += 4) {
                float left = holes[index] - left();
                float top = holes[index + 1] - top();
                canvas.clipOutRect(left, top, left + holes[index + 2], top + holes[index + 3]);
            }
            super.dispatchDraw(canvas);
            canvas.restore();
        }

        @Override
        public boolean dispatchTouchEvent(MotionEvent event) {
            if (event.getActionMasked() == MotionEvent.ACTION_DOWN) {
                forwarding = inHole(event.getX(), event.getY());
            }
            if (!forwarding) {
                return super.dispatchTouchEvent(event);
            }
            forwardTouches(handle, event, left(), top());
            return true;
        }
    }

    private WindowManager windowManager() {
        return activity.getWindowManager();
    }

    private IBinder token() {
        return activity.getWindow().getDecorView().getWindowToken();
    }

    /** A container for native views, in a panel window once an order mounts it. */
    public FrameLayout createNativeSurface() {
        Container container = new Container(activity, handle);
        WindowManager.LayoutParams params =
                new WindowManager.LayoutParams(
                        1,
                        1,
                        WindowManager.LayoutParams.TYPE_APPLICATION_PANEL,
                        // Not focusable: the activity's window keeps the focus, so GPUI stays
                        // active; touches still reach the native views.
                        WindowManager.LayoutParams.FLAG_NOT_FOCUSABLE
                                | WindowManager.LayoutParams.FLAG_LAYOUT_IN_SCREEN
                                | WindowManager.LayoutParams.FLAG_LAYOUT_NO_LIMITS
                                | WindowManager.LayoutParams.FLAG_NOT_TOUCH_MODAL,
                        PixelFormat.TRANSLUCENT);
        params.gravity = Gravity.TOP | Gravity.LEFT;
        container.setLayoutParams(params);
        return container;
    }

    /** Places a container at {@code x, y, width, height} in window pixels. */
    public void setBounds(FrameLayout container, int x, int y, int width, int height) {
        ui.post(
                () -> {
                    WindowManager.LayoutParams params =
                            (WindowManager.LayoutParams) container.getLayoutParams();
                    params.x = x;
                    params.y = y;
                    params.width = Math.max(width, 1);
                    params.height = Math.max(height, 1);
                    if (mounted.contains(container)) {
                        windowManager().updateViewLayout(container, params);
                    } else {
                        container.setLayoutParams(params);
                    }
                });
    }

    /**
     * Clips {@code holes}, window rectangles as {@code x, y, width, height} quadruples, out of a
     * container, which hands touches in them to GPUI.
     */
    public void setHoles(FrameLayout container, float[] holes) {
        ui.post(() -> ((Container) container).setHoles(holes));
    }

    public void setVisible(FrameLayout container, boolean visible) {
        ui.post(() -> container.setVisibility(visible ? View.VISIBLE : View.GONE));
    }

    /**
     * Mounts {@code containers}' panel windows bottom to top, and the overlay above them: panels
     * mounted later stack above earlier ones, so a changed order mounts them again.
     */
    public void setOrder(FrameLayout[] containers) {
        ui.post(
                () -> {
                    if (released) {
                        return;
                    }
                    List<FrameLayout> order = new ArrayList<>();
                    for (FrameLayout container : containers) {
                        order.add(container);
                    }
                    if (!order.equals(mounted)) {
                        for (FrameLayout container : mounted) {
                            windowManager().removeViewImmediate(container);
                        }
                        mounted.clear();
                        for (FrameLayout container : order) {
                            WindowManager.LayoutParams params =
                                    (WindowManager.LayoutParams) container.getLayoutParams();
                            params.token = token();
                            windowManager().addView(container, params);
                            mounted.add(container);
                        }
                    }
                    ensureOverlay();
                });
    }

    /** The overlay window, made the first time an order is set. */
    private void ensureOverlay() {
        if (overlay != null || !overlayWindow) {
            return;
        }
        overlay = new FrameLayout(activity);
        SurfaceView surface = new SurfaceView(activity);
        surface.setZOrderOnTop(true);
        surface.getHolder().setFormat(PixelFormat.TRANSLUCENT);
        surface.getHolder()
                .addCallback(
                        new SurfaceHolder.Callback() {
                            @Override
                            public void surfaceCreated(SurfaceHolder holder) {}

                            @Override
                            public void surfaceChanged(
                                    SurfaceHolder holder, int format, int width, int height) {
                                nativeSurfaceChanged(handle, holder.getSurface(), width, height);
                            }

                            @Override
                            public void surfaceDestroyed(SurfaceHolder holder) {
                                nativeSurfaceChanged(handle, null, 0, 0);
                            }
                        });
        overlay.addView(
                surface,
                new FrameLayout.LayoutParams(
                        ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT));
        overlay.setOnTouchListener(
                (view, event) -> {
                    forwardTouches(handle, event, 0, 0);
                    return true;
                });
        overlayParams =
                new WindowManager.LayoutParams(
                        WindowManager.LayoutParams.MATCH_PARENT,
                        WindowManager.LayoutParams.MATCH_PARENT,
                        WindowManager.LayoutParams.TYPE_APPLICATION_SUB_PANEL,
                        WindowManager.LayoutParams.FLAG_NOT_FOCUSABLE
                                | WindowManager.LayoutParams.FLAG_LAYOUT_IN_SCREEN
                                | WindowManager.LayoutParams.FLAG_LAYOUT_NO_LIMITS
                                | WindowManager.LayoutParams.FLAG_NOT_TOUCHABLE,
                        PixelFormat.TRANSLUCENT);
        overlayParams.gravity = Gravity.TOP | Gravity.LEFT;
        overlayParams.token = token();
        windowManager().addView(overlay, overlayParams);
    }

    /** Takes touches while GPUI drew something on the overlay, and lets them through otherwise. */
    public void setOverlayTouchable(boolean touchable) {
        ui.post(
                () -> {
                    if (overlay == null || touchable == overlayTouchable) {
                        return;
                    }
                    overlayTouchable = touchable;
                    if (touchable) {
                        overlayParams.flags &= ~WindowManager.LayoutParams.FLAG_NOT_TOUCHABLE;
                    } else {
                        overlayParams.flags |= WindowManager.LayoutParams.FLAG_NOT_TOUCHABLE;
                    }
                    windowManager().updateViewLayout(overlay, overlayParams);
                });
    }

    /** Hands each pointer of {@code event} to GPUI, offset to window coordinates. */
    private static void forwardTouches(long handle, MotionEvent event, float dx, float dy) {
        int action = event.getActionMasked();
        int changed = event.getActionIndex();
        for (int index = 0; index < event.getPointerCount(); index++) {
            int pointerAction;
            switch (action) {
                case MotionEvent.ACTION_DOWN:
                case MotionEvent.ACTION_UP:
                case MotionEvent.ACTION_CANCEL:
                    pointerAction = action;
                    break;
                case MotionEvent.ACTION_POINTER_DOWN:
                    pointerAction = index == changed ? MotionEvent.ACTION_DOWN : MotionEvent.ACTION_MOVE;
                    break;
                case MotionEvent.ACTION_POINTER_UP:
                    pointerAction = index == changed ? MotionEvent.ACTION_UP : MotionEvent.ACTION_MOVE;
                    break;
                default:
                    pointerAction = MotionEvent.ACTION_MOVE;
            }
            nativeTouch(
                    handle,
                    pointerAction,
                    event.getPointerId(index),
                    event.getX(index) + dx,
                    event.getY(index) + dy);
        }
    }

    /** Removes every window: GPUI releases the composition. */
    public void release() {
        ui.post(
                () -> {
                    released = true;
                    for (FrameLayout container : mounted) {
                        windowManager().removeViewImmediate(container);
                    }
                    mounted.clear();
                    if (overlay != null) {
                        windowManager().removeViewImmediate(overlay);
                        overlay = null;
                    }
                });
    }

    /** The overlay's surface, or null once it is destroyed; called on the UI thread. */
    private static native void nativeSurfaceChanged(
            long handle, Surface surface, int width, int height);

    /** One pointer of a touch on the overlay; called on the UI thread. */
    private static native void nativeTouch(long handle, int action, int pointerId, float x, float y);
}
