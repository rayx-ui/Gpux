package dev.gpux.mobile;

import android.app.Activity;
import android.content.Context;
import android.graphics.Canvas;
import android.graphics.PixelFormat;
import android.os.Handler;
import android.os.IBinder;
import android.os.Looper;
import android.view.Gravity;
import android.view.KeyEvent;
import android.view.MotionEvent;
import android.view.Surface;
import android.view.SurfaceHolder;
import android.view.SurfaceView;
import android.view.View;
import android.view.ViewGroup;
import android.view.WindowManager;
import android.view.inputmethod.InputMethodManager;
import android.webkit.WebView;
import android.widget.FrameLayout;
import java.lang.ref.WeakReference;
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
 * <p>GPUI's coordinates are those of the surface it draws into. A NativeActivity's surface is
 * the activity's window. An embedding host whose SurfaceView does not fill the window names that
 * view with {@link #setHostView}, and panels, holes, the overlay and touches are offset by its
 * position in the window.
 *
 * <p>The overlay takes no touches while GPUI drew nothing on it, so they reach the native views
 * and the activity's window below; while it has content it takes them all and hands them to GPUI
 * as raw touches.
 *
 * <p>Where GPUI renders through GLES, whose surfaces wgpu presents only opaque, there is no
 * overlay window: GPUI draws its overlay content into the activity's window and each container
 * clips that content out of itself ({@link #setHoles}), handing touches in the holes to GPUI.
 *
 * <p>Panels are not focusable, so the activity's window keeps the focus and GPUI stays active. A
 * panel whose content takes text (a WebView, an EditText) becomes focusable when it is touched,
 * so the soft keyboard reaches it, and gives the focus back on a touch outside it or the back
 * key; GPUI is told ({@code nativePanelFocus}) and does not take that focus change for the
 * activity going inactive.
 *
 * <p>Rust calls every method from GPUI's thread; window changes are posted to the UI thread.
 * The native methods are registered by gpux when it loads this class. Consumers add these
 * sources to their build, as {@code gpux-testkit android build} does without Gradle.
 */
public final class Composition {
    /** The view GPUI draws into, when it is not the whole window. */
    private static WeakReference<View> hostView = new WeakReference<>(null);

    private final Activity activity;
    /** The Rust state the native callbacks reach. */
    private final long handle;
    /** Whether GPUI's overlay content goes into an overlay window, or through holes. */
    private final boolean overlayWindow;
    private final Handler ui = new Handler(Looper.getMainLooper());
    /** The panel windows mounted, bottom to top. */
    private final List<Container> mounted = new ArrayList<>();
    private FrameLayout overlay;
    private WindowManager.LayoutParams overlayParams;
    private boolean overlayTouchable;
    /** The panel that took the window focus for text input. */
    private Container focused;
    private boolean released;
    /** The host view whose layout changes re-place the panels. */
    private View watchedHost;
    private final View.OnLayoutChangeListener hostMoved =
            (view, left, top, right, bottom, oldLeft, oldTop, oldRight, oldBottom) -> placeAll();

    public Composition(Activity activity, long handle, boolean overlayWindow) {
        this.activity = activity;
        this.handle = handle;
        this.overlayWindow = overlayWindow;
    }

    /**
     * Names the view GPUI draws into, for an embedding host whose SurfaceView does not fill the
     * activity's window. Call it on the UI thread before GPUI composes; null means the window.
     */
    public static void setHostView(View view) {
        hostView = new WeakReference<>(view);
    }

    /** Where GPUI's surface starts in the activity's window, in pixels. */
    private static int[] origin() {
        int[] origin = new int[2];
        View host = hostView.get();
        if (host != null && host.isAttachedToWindow()) {
            host.getLocationInWindow(origin);
        }
        return origin;
    }

    /**
     * A native surface's container: it clips its holes (rectangles of GPUI content above it, in
     * GPUI's coordinates) out of what it draws, and hands touches that start in one to GPUI.
     */
    final class Container extends FrameLayout {
        /** The container's bounds in GPUI's coordinates. */
        private int x;
        private int y;
        private int width = 1;
        private int height = 1;
        private float[] holes = new float[0];
        private boolean forwarding;

        Container(Context context) {
            super(context);
            setClipChildren(true);
        }

        WindowManager.LayoutParams params() {
            return (WindowManager.LayoutParams) getLayoutParams();
        }

        void setHoles(float[] holes) {
            this.holes = holes;
            invalidate();
        }

        private boolean inHole(float px, float py) {
            for (int index = 0; index + 3 < holes.length; index += 4) {
                float left = holes[index] - x;
                float top = holes[index + 1] - y;
                if (px >= left
                        && py >= top
                        && px < left + holes[index + 2]
                        && py < top + holes[index + 3]) {
                    return true;
                }
            }
            return false;
        }

        @Override
        protected void dispatchDraw(Canvas canvas) {
            canvas.save();
            for (int index = 0; index + 3 < holes.length; index += 4) {
                float left = holes[index] - x;
                float top = holes[index + 1] - y;
                canvas.clipOutRect(left, top, left + holes[index + 2], top + holes[index + 3]);
            }
            super.dispatchDraw(canvas);
            canvas.restore();
        }

        @Override
        public boolean dispatchTouchEvent(MotionEvent event) {
            int action = event.getActionMasked();
            if (action == MotionEvent.ACTION_OUTSIDE) {
                // A touch elsewhere while this panel holds the focus: give it back.
                unfocus();
                return false;
            }
            if (action == MotionEvent.ACTION_DOWN) {
                forwarding = inHole(event.getX(), event.getY());
                if (!forwarding && wantsText(this)) {
                    focus(this);
                }
            }
            if (!forwarding) {
                return super.dispatchTouchEvent(event);
            }
            forwardTouches(handle, event, x, y);
            return true;
        }

        @Override
        public boolean dispatchKeyEvent(KeyEvent event) {
            if (super.dispatchKeyEvent(event)) {
                return true;
            }
            if (event.getKeyCode() == KeyEvent.KEYCODE_BACK && focused == this) {
                if (event.getAction() == KeyEvent.ACTION_UP) {
                    unfocus();
                }
                return true;
            }
            return false;
        }
    }

    /** Whether {@code view} or a view in it takes text input. */
    private static boolean wantsText(View view) {
        if (view instanceof WebView || view.onCheckIsTextEditor()) {
            return true;
        }
        if (view instanceof ViewGroup) {
            ViewGroup group = (ViewGroup) view;
            for (int index = 0; index < group.getChildCount(); index++) {
                if (wantsText(group.getChildAt(index))) {
                    return true;
                }
            }
        }
        return false;
    }

    /** Makes {@code container}'s panel focusable, so the soft keyboard reaches its views. */
    private void focus(Container container) {
        if (focused == container || !mounted.contains(container)) {
            return;
        }
        unfocus();
        focused = container;
        // Before the focus moves: GPUI must not take it for the activity going inactive.
        nativePanelFocus(handle, true);
        WindowManager.LayoutParams params = container.params();
        params.flags &= ~WindowManager.LayoutParams.FLAG_NOT_FOCUSABLE;
        params.flags |= WindowManager.LayoutParams.FLAG_WATCH_OUTSIDE_TOUCH;
        windowManager().updateViewLayout(container, params);
    }

    /** Gives the window focus back to the activity's window. */
    private void unfocus() {
        Container container = focused;
        if (container == null) {
            return;
        }
        focused = null;
        InputMethodManager keyboard =
                (InputMethodManager) activity.getSystemService(Context.INPUT_METHOD_SERVICE);
        if (keyboard != null && container.getWindowToken() != null) {
            keyboard.hideSoftInputFromWindow(container.getWindowToken(), 0);
        }
        WindowManager.LayoutParams params = container.params();
        params.flags |= WindowManager.LayoutParams.FLAG_NOT_FOCUSABLE;
        params.flags &= ~WindowManager.LayoutParams.FLAG_WATCH_OUTSIDE_TOUCH;
        if (mounted.contains(container)) {
            windowManager().updateViewLayout(container, params);
        }
        nativePanelFocus(handle, false);
    }

    private WindowManager windowManager() {
        return activity.getWindowManager();
    }

    private IBinder token() {
        return activity.getWindow().getDecorView().getWindowToken();
    }

    /** A container for native views, in a panel window once an order mounts it. */
    public FrameLayout createNativeSurface() {
        Container container = new Container(activity);
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

    /** Sets a container's window position and size from its bounds and GPUI's origin. */
    private void place(Container container, int[] origin) {
        WindowManager.LayoutParams params = container.params();
        params.x = origin[0] + container.x;
        params.y = origin[1] + container.y;
        params.width = container.width;
        params.height = container.height;
        if (mounted.contains(container)) {
            windowManager().updateViewLayout(container, params);
        } else {
            container.setLayoutParams(params);
        }
    }

    /** Places every panel and the overlay again: GPUI's surface moved in the window. */
    private void placeAll() {
        if (released) {
            return;
        }
        int[] origin = origin();
        for (Container container : mounted) {
            place(container, origin);
        }
        placeOverlay();
    }

    /** Follows the host view's layout, once there is one. */
    private void watchHost() {
        View host = hostView.get();
        if (host == watchedHost) {
            return;
        }
        if (watchedHost != null) {
            watchedHost.removeOnLayoutChangeListener(hostMoved);
        }
        watchedHost = host;
        if (host != null) {
            host.addOnLayoutChangeListener(hostMoved);
        }
    }

    /** Places a container at {@code x, y, width, height} pixels in GPUI's coordinates. */
    public void setBounds(FrameLayout container, int x, int y, int width, int height) {
        ui.post(
                () -> {
                    Container placed = (Container) container;
                    placed.x = x;
                    placed.y = y;
                    placed.width = Math.max(width, 1);
                    placed.height = Math.max(height, 1);
                    place(placed, origin());
                });
    }

    /**
     * Clips {@code holes}, rectangles in GPUI's coordinates as {@code x, y, width, height}
     * quadruples, out of a container, which hands touches in them to GPUI.
     */
    public void setHoles(FrameLayout container, float[] holes) {
        ui.post(() -> ((Container) container).setHoles(holes));
    }

    public void setVisible(FrameLayout container, boolean visible) {
        ui.post(
                () -> {
                    if (!visible && focused == container) {
                        unfocus();
                    }
                    container.setVisibility(visible ? View.VISIBLE : View.GONE);
                });
    }

    /**
     * Mounts {@code containers}' panel windows bottom to top, and the overlay above them. Panels
     * mounted later stack above earlier ones, so the panels from the first changed position on
     * are mounted again; the ones below it, and their views' windows, are left alone.
     */
    public void setOrder(FrameLayout[] containers) {
        ui.post(
                () -> {
                    if (released) {
                        return;
                    }
                    watchHost();
                    List<Container> order = new ArrayList<>();
                    for (FrameLayout container : containers) {
                        order.add((Container) container);
                    }
                    int kept = 0;
                    while (kept < mounted.size()
                            && kept < order.size()
                            && mounted.get(kept) == order.get(kept)) {
                        kept++;
                    }
                    for (int index = mounted.size() - 1; index >= kept; index--) {
                        Container container = mounted.get(index);
                        if (focused == container && !order.contains(container)) {
                            unfocus();
                        }
                        windowManager().removeViewImmediate(container);
                        mounted.remove(index);
                    }
                    int[] origin = origin();
                    for (int index = kept; index < order.size(); index++) {
                        Container container = order.get(index);
                        WindowManager.LayoutParams params = container.params();
                        params.token = token();
                        params.x = origin[0] + container.x;
                        params.y = origin[1] + container.y;
                        windowManager().addView(container, params);
                        mounted.add(container);
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
        sizeOverlay();
        windowManager().addView(overlay, overlayParams);
    }

    /** The overlay covers GPUI's surface: the host view when there is one, else the window. */
    private void sizeOverlay() {
        View host = hostView.get();
        if (host != null && host.isAttachedToWindow() && host.getWidth() > 0) {
            int[] origin = origin();
            overlayParams.x = origin[0];
            overlayParams.y = origin[1];
            overlayParams.width = host.getWidth();
            overlayParams.height = host.getHeight();
        } else {
            overlayParams.x = 0;
            overlayParams.y = 0;
            overlayParams.width = WindowManager.LayoutParams.MATCH_PARENT;
            overlayParams.height = WindowManager.LayoutParams.MATCH_PARENT;
        }
    }

    private void placeOverlay() {
        if (overlay == null) {
            return;
        }
        sizeOverlay();
        windowManager().updateViewLayout(overlay, overlayParams);
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

    /**
     * Hands each pointer of {@code event} to GPUI, offset by {@code dx, dy} into GPUI's
     * coordinates. A move's batched samples go first, each with its own time, so GPUI's pan and
     * fling see the whole motion of a fast gesture.
     */
    private static void forwardTouches(long handle, MotionEvent event, float dx, float dy) {
        int action = event.getActionMasked();
        int changed = event.getActionIndex();
        if (action == MotionEvent.ACTION_MOVE) {
            for (int sample = 0; sample < event.getHistorySize(); sample++) {
                long time = event.getHistoricalEventTime(sample);
                for (int index = 0; index < event.getPointerCount(); index++) {
                    nativeTouch(
                            handle,
                            MotionEvent.ACTION_MOVE,
                            event.getPointerId(index),
                            event.getHistoricalX(index, sample) + dx,
                            event.getHistoricalY(index, sample) + dy,
                            time);
                }
            }
        }
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
                    event.getY(index) + dy,
                    event.getEventTime());
        }
    }

    /** Removes every window: GPUI releases the composition. */
    public void release() {
        ui.post(
                () -> {
                    unfocus();
                    released = true;
                    if (watchedHost != null) {
                        watchedHost.removeOnLayoutChangeListener(hostMoved);
                        watchedHost = null;
                    }
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

    /**
     * One pointer of a touch on GPUI's content, at {@code time} on the {@code
     * SystemClock.uptimeMillis()} timeline; called on the UI thread.
     */
    private static native void nativeTouch(
            long handle, int action, int pointerId, float x, float y, long time);

    /** A panel took ({@code true}) or gave back the window focus; called on the UI thread. */
    private static native void nativePanelFocus(long handle, boolean focused);
}
