/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. https://mozilla.org/MPL/2.0/ */
package org.touchhle.android;

import android.content.Context;
import android.graphics.Canvas;
import android.graphics.Color;
import android.graphics.Paint;
import android.graphics.Typeface;
import android.os.SystemClock;
import android.text.Layout;
import android.text.StaticLayout;
import android.text.TextPaint;
import org.json.JSONObject;
import java.io.ByteArrayOutputStream;
import java.io.File;
import java.io.FileInputStream;
import java.io.InputStream;
import java.nio.charset.StandardCharsets;

/** Shared schema used by the desktop previewer and all three Android layers. */
final class OverlayStyle {
    private static JSONObject defaults, active;
    private static long lastCheck, stamp = Long.MIN_VALUE, revision;
    private static Context context;
    static void init(Context ctx) throws Exception {
        context = ctx.getApplicationContext();
        if (defaults == null) {
            try (InputStream in = ctx.getAssets().open("overlay/default-style.json")) { defaults = read(in); }
            active = defaults;
        }
    }
    private static JSONObject read(InputStream in) throws Exception {
        ByteArrayOutputStream out = new ByteArrayOutputStream(); byte[] buffer = new byte[16384]; int n;
        while ((n = in.read(buffer)) != -1) {
            if (out.size() + n > 1024 * 1024) { throw new IllegalArgumentException("Style too large"); }
            out.write(buffer, 0, n);
        }
        JSONObject value = new JSONObject(new String(out.toByteArray(), StandardCharsets.UTF_8));
        if (value.getInt("schema") != 1 || value.optJSONObject("layers") == null) { throw new IllegalArgumentException("Style schema"); }
        return value;
    }
    static void poll() {
        if (context == null || SystemClock.uptimeMillis() - lastCheck < 750) { return; }
        lastCheck = SystemClock.uptimeMillis();
        File file = new File(context.getFilesDir(), "overlay-style.json");
        long current = file.isFile() ? file.lastModified() : 0;
        if (current == stamp) { return; }
        try {
            JSONObject next = defaults;
            if (file.isFile()) { try (InputStream in = new FileInputStream(file)) { next = read(in); } }
            active = next; stamp = current; revision++;
            android.util.Log.i("touchHLE", "[DSoverlay-config] loaded revision=" + revision
                    + " overrides=" + (active.optJSONObject("overrides") == null ? 0 : active.optJSONObject("overrides").length()));
        } catch (Exception error) {
            stamp = current;
            android.util.Log.e("touchHLE", "[DSoverlay-config] rejected; retaining last valid style", error);
        }
    }
    static long revision() { return revision; }
    static boolean external() { return active != defaults; }
    static String text(String id, String fallback, boolean secondary) {
        JSONObject overrides = active.optJSONObject("overrides");
        JSONObject entry = overrides == null ? null : overrides.optJSONObject(id);
        String value = entry == null ? fallback : entry.optString(secondary ? "secondaryText" : "text", fallback);
        return value == null || value.trim().isEmpty() ? fallback : value;
    }
    static Layer layer(String name, String id) {
        JSONObject base = defaults.optJSONObject("layers").optJSONObject(name);
        JSONObject set = active.optJSONObject("layers").optJSONObject(name);
        JSONObject overrides = active.optJSONObject("overrides");
        JSONObject entry = overrides == null ? null : overrides.optJSONObject(id);
        return new Layer(base, set, entry == null ? null : entry.optJSONObject("style"));
    }
    static final class Layer {
        final JSONObject[] sources;
        Layer(JSONObject... sources) { this.sources = sources; }
        Object get(String key) {
            for (int i = sources.length - 1; i >= 0; i--) {
                if (sources[i] != null && sources[i].has(key)) { return sources[i].opt(key); }
            }
            return null;
        }
        float number(String key, float fallback, float low, float high) {
            Object raw = get(key); double value = raw instanceof Number ? ((Number)raw).doubleValue() : fallback;
            return Double.isNaN(value) || Double.isInfinite(value) ? fallback : (float)Math.max(low, Math.min(high, value));
        }
        boolean enabled() { return !Boolean.FALSE.equals(get("enabled")); }
        String string(String key, String fallback) { Object raw = get(key); return raw instanceof String ? (String)raw : fallback; }
        int color(String key, String fallback) {
            String value = string(key, fallback);
            try { return value.matches("#[0-9a-fA-F]{6}") ? Color.parseColor(value) : Color.parseColor(fallback); }
            catch (RuntimeException error) { return Color.parseColor(fallback); }
        }
    }

    // A small bounded cache avoids re-laying out long subtitles on every frame.
    private static final java.util.LinkedHashMap<String, Card> cache = new java.util.LinkedHashMap<>();
    private static final class Card { StaticLayout label, first, second; float width, pad, gap, firstStep, labelStep, secondStep; }
    static void draw(Canvas canvas, Layer style, String first, String second, Typeface font,
            float left, float top, float cw, float ch, float opacity) {
        draw(canvas, style, first, second, null, font, font, left, top, cw, ch, opacity);
    }
    static void drawObjective(Canvas canvas, Layer style, String text, Typeface font, Typeface titleFont,
            float left, float top, float cw, float ch) {
        draw(canvas, style, text, null, style.string("label", "目标："), font, titleFont, left, top, cw, ch, 1f);
    }
    private static void draw(Canvas canvas, Layer style, String first, String second, String label, Typeface font, Typeface headingFont,
            float left, float top, float cw, float ch, float opacity) {
        if (!style.enabled() || first == null || first.isEmpty()) { return; }
        boolean interference=SignalText.marked(first)&&!Boolean.FALSE.equals(style.get("signalAnimation"));
        first=SignalText.clean(first);
        float sy = ch / 320f;
        float fontEmScale = RoleFonts.heightToEm(font);
        float labelEmScale = RoleFonts.heightToEm(headingFont);
        float pad = style.number("padding", 5, 0, 20) * sy;
        float width = Math.min(cw - 4 * sy, cw * style.number("width", 46, 10, 96) / 100f);
        float fontSize = style.number("fontSize", 11.5f, 3, 36) * sy * fontEmScale;
        float secondarySize = style.number("secondaryFontSize", 8.5f, 3, 30) * sy * fontEmScale;
        float labelSize = style.number("labelFontSize", 18, 3, 18) * sy * labelEmScale;
        float labelGap = style.number("labelGap", 1.5f, 0, 8) * sy;
        float gap = style.number("gap", 3, 0, 15) * sy;
        float lineHeight = style.number("lineHeight", 1.12f, 1, 2);
        Layout.Alignment align = "left".equals(style.string("align", "center")) ? Layout.Alignment.ALIGN_NORMAL
                : "right".equals(style.string("align", "center")) ? Layout.Alignment.ALIGN_OPPOSITE : Layout.Alignment.ALIGN_CENTER;
        float maxHeight = ch * style.number("maxHeight", 35, 10, 80) / 100f;
        float maxFont=style.number("maxFontSize",18,3,36)*sy*fontEmScale;
        String key = revision + ":" + System.identityHashCode(font) + ":" + System.identityHashCode(headingFont) + ":" + width + ":" + pad + ":" + fontSize + ":" + secondarySize + ":" + labelSize + ":" + maxHeight + ":" + maxFont + ":" + lineHeight + ":" + align + ":" + first + ":" + second + ":" + label;
        Card card = cache.get(key);
        if (card == null) {
            card = new Card(); card.width = width; card.pad = pad; card.gap = gap;
            int inner = Math.max(1, Math.round(width - 2 * pad));
            TextPaint paint = new TextPaint(Paint.ANTI_ALIAS_FLAG); paint.setTypeface(font);
            paint.setTextSize(fontSize); paint.setShadowLayer(sy * .5f, 0, sy * .5f, Color.BLACK);
            if (label != null) {
                float available=Math.max(1,maxHeight-2*pad-labelSize/labelEmScale*lineHeight-labelGap);
                float lo=fontSize,hi=Math.max(lo,maxFont);
                for(int i=0;i<18;i++) {
                    float mid=(lo+hi)*.5f;paint.setTextSize(mid);
                    StaticLayout candidate=new StaticLayout(ObjectiveLayout.wrap(first,paint,inner),paint,inner,align,lineHeight,0,false);
                    if(candidate.getLineCount()<=2&&candidate.getLineCount()*mid/fontEmScale*lineHeight<=available)lo=mid;else hi=mid;
                }
                fontSize=Math.max(fontSize,lo-2*sy*fontEmScale);paint.setTextSize(fontSize);
                first=ObjectiveLayout.wrap(first,paint,inner);
            }
            card.first = new StaticLayout(first, paint, inner, align, lineHeight, 0, false);
            card.firstStep=fontSize/fontEmScale*lineHeight;
            card.labelStep=labelSize/labelEmScale*lineHeight;card.secondStep=secondarySize/fontEmScale*lineHeight;
            if (label != null && !label.isEmpty()) {
                TextPaint heading = new TextPaint(paint); heading.setTypeface(headingFont); heading.setTextSize(labelSize);
                card.label = new StaticLayout(label, heading, inner, align, lineHeight, 0, false);
            }
            if (second != null && !second.isEmpty()) {
                TextPaint sub = new TextPaint(paint); sub.setTextSize(secondarySize);
                card.second = new StaticLayout(second, sub, inner, align, lineHeight, 0, false);
            }
            if (cache.size() > 12) { cache.clear(); }
            cache.put(key, card);
        }
        float labelHeight = card.label == null ? 0 : Math.max(card.label.getHeight(),card.label.getLineCount()*card.labelStep) + labelGap;
        float firstHeight = Math.max(card.first.getHeight(),card.first.getLineCount()*card.firstStep), height = labelHeight + firstHeight + 2 * pad;
        if (card.second != null) { height += Math.max(card.second.getHeight(),card.second.getLineCount()*card.secondStep) + gap; }
        if (label != null) height=maxHeight;
        float scale = Math.min(1f, maxHeight / height);
        float bx = left + cw * style.number("x", 50, 0, 100) / 100f - width * scale / 2f;
        float anchor = top + ch * style.number("y", 50, 0, 100) / 100f;
        String mode = style.string("anchor", "center");
        float by = "bottom-center".equals(mode) ? anchor - height * scale
                : "first-center".equals(mode) ? anchor - (pad + labelHeight + firstHeight / 2) * scale : anchor - height * scale / 2;
        bx = Math.max(left, Math.min(bx, left + cw - width * scale));
        by = Math.max(top, Math.min(by, top + ch - height * scale));
        Paint background = new Paint(Paint.ANTI_ALIAS_FLAG); background.setColor(style.color("background", "#031016"));
        background.setAlpha(Math.round(255 * opacity * style.number("opacity", .76f, 0, 1)));
        float radius = style.number("radius", 2, 0, 20) * sy;
        canvas.save(); canvas.translate(bx, by); canvas.scale(scale, scale);
        boolean projection=!style.string("projection","").isEmpty();
        if(projection){opacity*=ProjectionPanel.draw(canvas,card,style,width,height,opacity);background.setAlpha(0);}
        canvas.drawRoundRect(0, 0, width, height, radius, radius, background);
        background.setStyle(Paint.Style.STROKE); background.setStrokeWidth(Math.max(1, sy * .35f));
        background.setColor(style.color("stroke", "#56dce8"));
        background.setAlpha(Math.round(255 * opacity * style.number("strokeOpacity", .6f, 0, 1)));
        if(projection){background.setAlpha(0);}
        canvas.drawRoundRect(0, 0, width, height, radius, radius, background);
        card.first.getPaint().setColor(style.color("color", "#ebf4f7")); card.first.getPaint().setAlpha(Math.round(255 * opacity));
        canvas.translate(pad, label==null?pad:Math.max(pad,(height-labelHeight-firstHeight)*.5f));
        if (card.label != null) {
            card.label.getPaint().setColor(style.color("labelColor", "#56dce8"));
            card.label.getPaint().setAlpha(Math.round(255 * opacity)); card.label.draw(canvas);
            canvas.translate(0, labelHeight);
        }
        SignalText.draw(canvas,card.first,interference,sy);
        if (card.second != null) {
            card.second.getPaint().setColor(style.color("secondaryColor", "#56dce8")); card.second.getPaint().setAlpha(Math.round(255 * opacity));
            canvas.translate(0, firstHeight + gap); card.second.draw(canvas);
        }
        canvas.restore();
    }
}
