package dev.gpux.mobile;

import android.app.Activity;
import android.content.ClipData;
import android.content.ClipboardManager;
import android.content.Context;
import android.os.Handler;
import android.os.Looper;
import android.util.Log;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicReference;

/**
 * gpui-mobile's clipboard on Android: the system {@link ClipboardManager}, with plain text and,
 * beside it, the same content as HTML ({@link ClipData#newHtmlText}).
 *
 * <p>Rust calls every method from GPUI's thread; the clipboard work runs on the UI thread, which
 * the call waits for (briefly), so a read after a write sees it.
 */
public final class Clipboard {
    private static final String TAG = "gpux-clipboard";
    private static final String LABEL = "gpux";
    private static final long TIMEOUT_MS = 1000;
    private static final Handler ui = new Handler(Looper.getMainLooper());

    private Clipboard() {}

    /** Writes {@code text}, with {@code html} beside it unless it is null. */
    public static boolean write(Activity activity, String text, String html) {
        Boolean written =
                onUiThread(
                        () -> {
                            ClipboardManager manager = manager(activity);
                            if (manager == null) {
                                return false;
                            }
                            ClipData clip =
                                    html != null
                                            ? ClipData.newHtmlText(LABEL, text, html)
                                            : ClipData.newPlainText(LABEL, text);
                            manager.setPrimaryClip(clip);
                            return true;
                        });
        return written != null && written;
    }

    /**
     * The clipboard's text and HTML, read from one clip: {@code {text, html}}, either null when
     * the clip holds none; null when the clipboard is empty or unreachable.
     */
    public static String[] read(Activity activity) {
        return onUiThread(
                () -> {
                    ClipData.Item item = firstItem(activity);
                    if (item == null) {
                        return null;
                    }
                    CharSequence text = item.getText();
                    if (text == null) {
                        text = item.coerceToText(activity);
                    }
                    return new String[] {text == null ? null : text.toString(), item.getHtmlText()};
                });
    }

    private static ClipboardManager manager(Activity activity) {
        return (ClipboardManager) activity.getSystemService(Context.CLIPBOARD_SERVICE);
    }

    private static ClipData.Item firstItem(Activity activity) {
        ClipboardManager manager = manager(activity);
        if (manager == null || !manager.hasPrimaryClip()) {
            return null;
        }
        ClipData clip = manager.getPrimaryClip();
        if (clip == null || clip.getItemCount() == 0) {
            return null;
        }
        return clip.getItemAt(0);
    }

    private interface Work<T> {
        T run();
    }

    /** Runs {@code work} on the UI thread and returns its result, or null after a timeout. */
    private static <T> T onUiThread(Work<T> work) {
        if (Looper.myLooper() == Looper.getMainLooper()) {
            return work.run();
        }
        AtomicReference<T> result = new AtomicReference<>();
        CountDownLatch done = new CountDownLatch(1);
        ui.post(
                () -> {
                    try {
                        result.set(work.run());
                    } catch (RuntimeException error) {
                        Log.w(TAG, "clipboard access failed", error);
                    } finally {
                        done.countDown();
                    }
                });
        try {
            if (!done.await(TIMEOUT_MS, TimeUnit.MILLISECONDS)) {
                Log.w(TAG, "clipboard access timed out");
            }
        } catch (InterruptedException interrupted) {
            Thread.currentThread().interrupt();
        }
        return result.get();
    }
}
