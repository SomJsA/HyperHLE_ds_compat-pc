package org.touchhle.android;

import android.content.Context;
import android.graphics.Canvas;
import android.graphics.LinearGradient;
import android.graphics.Paint;
import android.graphics.Path;
import android.graphics.Shader;
import android.view.View;

/** Lightweight Ishimura-inspired line-art backdrop; no bitmap dependency. */
final class LauncherBackdropView extends View {
    private final Paint paint = new Paint(Paint.ANTI_ALIAS_FLAG);
    private final Path hull = new Path();

    LauncherBackdropView(Context context) {
        super(context);
        setWillNotDraw(false);
    }

    @Override
    protected void onDraw(Canvas canvas) {
        super.onDraw(canvas);
        float width = getWidth();
        float height = getHeight();
        paint.setShader(new LinearGradient(
                0, 0, width, height,
                0xFF0A1119, 0xFF132833, Shader.TileMode.CLAMP));
        canvas.drawRect(0, 0, width, height, paint);
        paint.setShader(null);

        paint.setStyle(Paint.Style.STROKE);
        paint.setStrokeWidth(1f);
        paint.setColor(0x18395A68);
        for (int x = 0; x < width; x += 44) {
            canvas.drawLine(x, 0, x, height, paint);
        }
        for (int y = 0; y < height; y += 36) {
            canvas.drawLine(0, y, width, y, paint);
        }

        canvas.save();
        canvas.translate(width * 0.64f, height * 0.53f);
        paint.setColor(0x4A6FCFD4);
        paint.setStrokeWidth(1.5f);
        hull.reset();
        hull.moveTo(-width * 0.31f, 0);
        hull.lineTo(-width * 0.08f, -height * 0.15f);
        hull.lineTo(width * 0.30f, -height * 0.08f);
        hull.lineTo(width * 0.34f, 0);
        hull.lineTo(width * 0.12f, height * 0.13f);
        hull.lineTo(-width * 0.13f, height * 0.12f);
        hull.close();
        canvas.drawPath(hull, paint);
        for (int i = -4; i <= 4; i++) {
            float x = i * width * 0.035f;
            canvas.drawLine(x, -height * 0.10f, x, height * 0.10f, paint);
        }
        paint.setColor(0x8C52D2D9);
        paint.setStrokeWidth(2f);
        canvas.drawLine(-width * 0.05f, 0, width * 0.31f, 0, paint);
        paint.setColor(0x243F6C7A);
        paint.setStrokeWidth(1f);
        for (int i = 1; i <= 3; i++) {
            canvas.drawCircle(0, 0, width * (0.08f + i * 0.055f), paint);
        }
        canvas.restore();
    }
}
