/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. https://mozilla.org/MPL/2.0/ */
package org.touchhle.android;

import android.content.Context;
import android.app.AlertDialog;
import android.graphics.Canvas;
import android.graphics.Color;
import android.graphics.Paint;
import android.graphics.RectF;
import android.graphics.Typeface;
import android.os.SystemClock;
import android.util.Log;
import android.view.View;
import android.text.StaticLayout;
import android.text.Layout;
import android.text.TextPaint;

/** Output-only chapter layer: never changes the SDL viewport or touch routing. */
final class ChapterOverlayView extends View {
    private final ChapterTypography typography;
    private static native long nativeSnapshot();
    private static native void nativeSetReady(boolean ready);

    private final Paint paint = new Paint(Paint.ANTI_ALIAS_FLAG);
    private final RectF panel = new RectF();
    private final Typeface font;
    private final boolean stretch;
    private boolean running;
    private boolean bridgeReady;
    private long lastSnapshot;
    private long phaseStarted;
    private ChapterOverlayGeometry geometry;
    private SubtitleOverlay subtitles;
    private ObjectiveOverlay objectives;
    private StaticLayout chapterChinese, chapterEnglish;
    private int cachedChapter;
    private final TextPaint chapterPaint = new TextPaint(Paint.ANTI_ALIAS_FLAG);

    @Override protected void onSizeChanged(int width, int height, int oldWidth, int oldHeight) {
        super.onSizeChanged(width, height, oldWidth, oldHeight);
        geometry = width > 0 && height > 0 ? new ChapterOverlayGeometry(width, height, stretch) : null;
        cachedChapter = 0;
    }

    ChapterOverlayView(Context context, boolean stretch, String language) {
        super(context);
        this.stretch = stretch;
        try {
            OverlayTexts.load(context, language);
            typography=new ChapterTypography(context);
        } catch(Exception e) {throw new IllegalStateException("Chapter fonts unavailable",e);}
        // Failure throws before the view is attached/acknowledged; native drawing
        // remains enabled. Use a dedicated font, not the guest font substitution.
        font = RoleFonts.get(context).story;
        try { OverlayStyle.init(context); objectives = new ObjectiveOverlay(context, font); }
        catch (Exception error) { Log.e("touchHLE", "[DSobjective-view] initialization failed; native display retained", error); }
        setClickable(false);
        setLongClickable(false);
        setFocusable(false);
        setImportantForAccessibility(IMPORTANT_FOR_ACCESSIBILITY_NO);
        paint.setTypeface(font);
        paint.setTextAlign(Paint.Align.CENTER);
        chapterPaint.setTypeface(font);
        try {
            subtitles = new SubtitleOverlay(context, this, font);
        } catch (Exception error) {
            Log.e("touchHLE", "[DSsubtitle-view] map unavailable; retaining native subtitles", error);
        }
    }

    void setRunning(boolean value) {
        running = value;
        if (!value) {
            if (subtitles != null) { subtitles.stop(); }
            if (objectives != null) { objectives.stop(); }
            nativeSetReady(false);
            bridgeReady = false;
            lastSnapshot = 0;
        }
        invalidate();
    }

    @Override protected void onDetachedFromWindow() {
        setRunning(false);
        super.onDetachedFromWindow();
    }

    @Override protected void onDraw(Canvas canvas) {
        super.onDraw(canvas);
        if (getWidth() == 0 || getHeight() == 0) { return; }
        OverlayStyle.poll();
        float cw = getWidth(), ch = getHeight();
        if (!stretch) {
            if (cw / ch > 1.5f) { cw = ch * 1.5f; }
            else { ch = cw / 1.5f; }
        }
        if (subtitles != null) {
            subtitles.draw(canvas, running, (getWidth() - cw) / 2f, (getHeight() - ch) / 2f, cw, ch);
        }
        if (objectives != null) { objectives.draw(canvas, running, (getWidth()-cw)/2f, (getHeight()-ch)/2f, cw, ch); }
        if (!running) { return; }
        boolean chapterEnabled=OverlayStyle.layer("chapter", "").enabled();
        if (bridgeReady != chapterEnabled) {
            nativeSetReady(chapterEnabled);
            bridgeReady = chapterEnabled;
            Log.i("touchHLE", "[DSchapter-view] ready; Canvas font loaded; touch passthrough");
        }
        long snapshot = nativeSnapshot();
        if (snapshot != lastSnapshot) {
            if (snapshot != 0 && (lastSnapshot == 0 || (snapshot >>> 32) != (lastSnapshot >>> 32))) {
                typography.beginPlayback();
            }
            phaseStarted = SystemClock.uptimeMillis();
            lastSnapshot = snapshot;
            if (snapshot != 0) {
                Log.i("touchHLE", "[DSchapter-view] serial=" + (snapshot >>> 32)
                        + " chapter=" + (snapshot & 255) + " phase=" + ((snapshot >>> 8) & 255));
            }
        }
        int chapter = (int)(snapshot & 255);
        int phase = (int)((snapshot >>> 8) & 255);
        if ((snapshot & (1L << 16)) != 0 && chapter >= 1 && chapter <= 12) {
            // State transitions are driven by the guest. These presentation-only
            // fades use the researched 400/500 ms windows; native timing is intact.
            long elapsed = SystemClock.uptimeMillis() - phaseStarted;
            float opacity = phase == 1 ? Math.min(1f, elapsed / 400f)
                    : phase == 3 ? Math.max(0f, 1f - elapsed / 500f) : 1f;
            drawChapter(canvas, chapter, opacity);
        }
        // Native text state needs a poll, not a 120 Hz fullscreen View redraw.
        // 30 Hz still resolves the 50/80 ms projection and signal phases.
        postInvalidateDelayed(33);
    }

    private void drawChapter(Canvas canvas, int chapter, float opacity) {
        String id=String.format(java.util.Locale.ROOT,"CHAPTER_%02d_GAME_TITLE",chapter);
        float cw=getWidth(),ch=getHeight();
        if (!stretch) { if(cw/ch>1.5f){cw=ch*1.5f;}else{ch=cw/1.5f;} }
        typography.draw(canvas,chapter,opacity,(getWidth()-cw)/2f,(getHeight()-ch)/2f,cw,ch);
    }

    AlertDialog subtitleSettings(Runnable closed) {
        if (subtitles == null) { return null; }
        return subtitles.settings(closed);
    }

}
