package org.touchhle.android;
import android.content.Context;
import android.graphics.Canvas;
import android.graphics.Typeface;
final class ObjectiveOverlay {
    private static native long nativeSnapshot();
    private static native void nativeSetReady(boolean value);
    private final Typeface font;
    private final Typeface titleFont;
    private boolean ready;
    ObjectiveOverlay(Context context, Typeface font) throws Exception { OverlayTexts.load(context); this.font=font; titleFont=RoleFonts.get(context).title; }
    void stop() { nativeSetReady(false); ready=false; }
    void draw(Canvas canvas,boolean running,float left,float top,float cw,float ch) {
        boolean desired=running && OverlayStyle.layer("objective","").enabled();
        if (desired!=ready) { nativeSetReady(desired); ready=desired; }
        long value=desired ? nativeSnapshot():0; int index=(int)(value&0xffff);
        if (index<=0 || index>=OverlayTexts.ids.length) { return; }
        String id=OverlayTexts.ids[index];
        OverlayStyle.drawObjective(canvas,OverlayStyle.layer("objective",id),OverlayStyle.text(id,OverlayTexts.text[index],false),
                font,titleFont,left,top,cw,ch);
    }
}
