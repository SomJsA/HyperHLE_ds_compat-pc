package org.touchhle.android;

import android.content.Context;
import android.graphics.Paint;
import android.graphics.Typeface;
import android.util.Log;
import java.util.IdentityHashMap;

/** The user's exact faces; logical heights use the actual platform metrics. */
final class RoleFonts {
    final Typeface title;
    final Typeface story;
    private static RoleFonts shared;
    private static final IdentityHashMap<Typeface, Float> scales = new IdentityHashMap<>();

    private RoleFonts(Context context) {
        String language = OverlayTexts.language();
        if (language.startsWith("ja")) {
            title = Typeface.createFromAsset(context.getAssets(), "touchHLE_fonts/NotoSansJP-Bold.otf");
            story = Typeface.createFromAsset(context.getAssets(), "touchHLE_fonts/NotoSansJP-Regular.otf");
        } else if (language.startsWith("ko")) {
            title = Typeface.createFromAsset(context.getAssets(), "touchHLE_fonts/NotoSansKR-Regular.otf");
            story = title;
        } else if (language.startsWith("zh")) {
            // The Android package already carries the OFL Noto CJK faces used
            // by UIKit; keep the Canvas layer from adding a second 12 MB font
            // pair just for the decorative desktop role faces.
            title = Typeface.createFromAsset(context.getAssets(), "touchHLE_fonts/NotoSansSC-Bold.otf");
            story = Typeface.createFromAsset(context.getAssets(), "touchHLE_fonts/NotoSansSC-Regular.otf");
        } else {
            title = Typeface.createFromAsset(context.getAssets(), "touchHLE_fonts/LiberationSans-Bold.ttf");
            story = Typeface.createFromAsset(context.getAssets(), "touchHLE_fonts/LiberationSans-Regular.ttf");
        }
        Log.i("touchHLE", "[DSfont-role] language=" + language + " titleHeightToEm=" + heightToEm(title)
                + " storyHeightToEm=" + heightToEm(story));
    }

    static synchronized RoleFonts get(Context context) {
        if (shared == null) { shared = new RoleFonts(context); }
        return shared;
    }

    static synchronized float heightToEm(Typeface face) {
        Float cached = scales.get(face);
        if (cached != null) { return cached; }
        Paint paint = new Paint(Paint.ANTI_ALIAS_FLAG);
        paint.setTypeface(face);
        paint.setTextSize(1000f);
        Paint.FontMetrics metrics = paint.getFontMetrics();
        float extent = metrics.descent - metrics.ascent;
        if (Float.isNaN(extent) || Float.isInfinite(extent) || extent <= 0) {
            throw new IllegalStateException("Invalid role font height");
        }
        float scale = 1000f / extent;
        if (scales.size() >= 16) { scales.clear(); }
        scales.put(face, scale);
        return scale;
    }

    static void setTextHeight(Paint paint, float height) {
        paint.setTextSize(height * heightToEm(paint.getTypeface()));
    }
}
