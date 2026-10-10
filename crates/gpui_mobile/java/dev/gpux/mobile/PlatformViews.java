package dev.gpux.mobile;

import android.app.Activity;
import android.os.Handler;
import android.os.Looper;
import android.util.Log;
import android.view.View;
import android.view.ViewGroup;
import android.webkit.WebChromeClient;
import android.webkit.WebSettings;
import android.webkit.WebView;
import android.webkit.WebViewClient;
import android.widget.FrameLayout;
import java.util.HashMap;
import java.util.Map;

/**
 * gpui-mobile's platform views on Android: native {@code View}s that GPUI places through window
 * composition, each in the container of a native surface ({@link Composition}).
 *
 * <p>gpux creates the view for a type through a registered {@link Factory}; it builds in {@code
 * container} (an empty layout, with an optional {@code background} colour such as {@code
 * #ff0000}) and {@code webview} ({@code url} or {@code html}, {@code javascript_enabled}, {@code
 * dom_storage_enabled}, {@code zoom_enabled}). An app registers its own types, such as a video
 * surface, with {@link #register} before GPUI creates them. Composition sizes and places a
 * view's container at the visible part of its element, and {@link #setFrame} the view in it.
 *
 * <p>Rust calls every method from GPUI's thread; view work runs on the UI thread.
 */
public final class PlatformViews {
    private static final String TAG = "gpux-platform-views";

    /** Creates the native view of one platform view. */
    public interface Factory {
        View create(Activity activity, long viewId, Map<String, String> params);

        default void pause(View view) {}

        default void resume(View view) {}

        default void dispose(View view) {}
    }

    private static final Handler ui = new Handler(Looper.getMainLooper());
    private static final Map<String, Factory> factories = new HashMap<>();
    private static final Map<Long, View> views = new HashMap<>();
    private static final Map<Long, Factory> viewFactories = new HashMap<>();
    /** Containers given before their view was made. */
    private static final Map<Long, FrameLayout> pendingContainers = new HashMap<>();

    static {
        factories.put("container", new ContainerFactory());
        factories.put("webview", new WebViewFactory());
    }

    private PlatformViews() {}

    /** Registers the factory of {@code viewType}, replacing a built-in one of that name. */
    public static synchronized void register(String viewType, Factory factory) {
        factories.put(viewType, factory);
    }

    /** Makes the view {@code viewId} of {@code viewType}; it shows once attached. */
    public static boolean createView(
            Activity activity, String viewType, long viewId, String creationParams) {
        Map<String, String> params = parse(creationParams);
        Factory factory;
        synchronized (PlatformViews.class) {
            factory = factories.get(viewType);
        }
        if (factory == null) {
            Log.w(TAG, "no factory for platform view type " + viewType);
            return false;
        }
        Factory chosen = factory;
        ui.post(
                () -> {
                    View view = chosen.create(activity, viewId, params);
                    if (view == null) {
                        Log.w(TAG, "the " + viewType + " factory made no view");
                        return;
                    }
                    views.put(viewId, view);
                    viewFactories.put(viewId, chosen);
                    FrameLayout container = pendingContainers.remove(viewId);
                    if (container != null) {
                        fill(container, view);
                    }
                });
        return true;
    }

    /** Puts the view {@code viewId} in {@code container}, a native surface's container. */
    public static void attach(long viewId, FrameLayout container) {
        ui.post(
                () -> {
                    View view = views.get(viewId);
                    if (view == null) {
                        pendingContainers.put(viewId, container);
                    } else {
                        fill(container, view);
                    }
                });
    }

    private static void fill(FrameLayout container, View view) {
        if (view.getParent() instanceof ViewGroup) {
            ((ViewGroup) view.getParent()).removeView(view);
        }
        container.addView(
                view,
                new FrameLayout.LayoutParams(
                        ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT));
    }

    /**
     * Places the view {@code viewId} at {@code x, y, width, height} pixels in its container: its
     * full size, offset where the container shows only part of it.
     */
    public static void setFrame(long viewId, int x, int y, int width, int height) {
        ui.post(
                () -> {
                    View view = views.get(viewId);
                    if (view == null) {
                        return;
                    }
                    ViewGroup.LayoutParams params = view.getLayoutParams();
                    if (params == null) {
                        params = new FrameLayout.LayoutParams(width, height);
                    }
                    params.width = Math.max(width, 1);
                    params.height = Math.max(height, 1);
                    view.setLayoutParams(params);
                    view.setTranslationX(x);
                    view.setTranslationY(y);
                });
    }

    public static void setVisible(long viewId, boolean visible) {
        ui.post(
                () -> {
                    View view = views.get(viewId);
                    if (view != null) {
                        view.setVisibility(visible ? View.VISIBLE : View.GONE);
                    }
                });
    }

    public static void disposeView(long viewId) {
        ui.post(
                () -> {
                    pendingContainers.remove(viewId);
                    View view = views.remove(viewId);
                    Factory factory = viewFactories.remove(viewId);
                    if (view == null) {
                        return;
                    }
                    if (view.getParent() instanceof ViewGroup) {
                        ((ViewGroup) view.getParent()).removeView(view);
                    }
                    if (factory != null) {
                        factory.dispose(view);
                    }
                });
    }

    public static void pauseAll() {
        ui.post(
                () -> {
                    for (Map.Entry<Long, View> entry : views.entrySet()) {
                        Factory factory = viewFactories.get(entry.getKey());
                        if (factory != null) {
                            factory.pause(entry.getValue());
                        }
                    }
                });
    }

    public static void resumeAll() {
        ui.post(
                () -> {
                    for (Map.Entry<Long, View> entry : views.entrySet()) {
                        Factory factory = viewFactories.get(entry.getKey());
                        if (factory != null) {
                            factory.resume(entry.getValue());
                        }
                    }
                });
    }

    public static void disposeAll() {
        ui.post(
                () -> {
                    for (Long viewId : views.keySet().toArray(new Long[0])) {
                        disposeView(viewId);
                    }
                });
    }

    /** Parses {@code key1=value1|key2=value2}. */
    private static Map<String, String> parse(String params) {
        Map<String, String> map = new HashMap<>();
        if (params == null || params.isEmpty()) {
            return map;
        }
        for (String pair : params.split("\\|")) {
            int equals = pair.indexOf('=');
            if (equals > 0) {
                map.put(pair.substring(0, equals), pair.substring(equals + 1));
            }
        }
        return map;
    }

    /** An empty layout, with an optional background colour. */
    static final class ContainerFactory implements Factory {
        @Override
        public View create(Activity activity, long viewId, Map<String, String> params) {
            FrameLayout frame = new FrameLayout(activity);
            int color = 0x00000000;
            String background = params.get("background");
            if (background != null) {
                try {
                    color = android.graphics.Color.parseColor(background);
                } catch (IllegalArgumentException e) {
                    Log.w(TAG, "bad background colour " + background);
                }
            }
            frame.setBackgroundColor(color);
            return frame;
        }
    }

    /** A WebView loading {@code url} or {@code html}. */
    static final class WebViewFactory implements Factory {
        @Override
        public View create(Activity activity, long viewId, Map<String, String> params) {
            WebView view = new WebView(activity);
            WebSettings settings = view.getSettings();
            settings.setJavaScriptEnabled(flag(params, "javascript_enabled"));
            settings.setDomStorageEnabled(flag(params, "dom_storage_enabled"));
            boolean zoom = flag(params, "zoom_enabled");
            settings.setBuiltInZoomControls(zoom);
            settings.setDisplayZoomControls(false);
            settings.setLoadWithOverviewMode(true);
            settings.setUseWideViewPort(true);
            view.setWebViewClient(new WebViewClient());
            view.setWebChromeClient(new WebChromeClient());
            String html = params.getOrDefault("html", "");
            String url = params.getOrDefault("url", "");
            if (!html.isEmpty()) {
                view.loadDataWithBaseURL(null, html, "text/html", "UTF-8", null);
            } else if (!url.isEmpty()) {
                view.loadUrl(url);
            }
            return view;
        }

        private static boolean flag(Map<String, String> params, String name) {
            return Boolean.parseBoolean(params.getOrDefault(name, "true"));
        }

        @Override
        public void pause(View view) {
            ((WebView) view).onPause();
        }

        @Override
        public void resume(View view) {
            ((WebView) view).onResume();
        }

        @Override
        public void dispose(View view) {
            ((WebView) view).destroy();
        }
    }
}
