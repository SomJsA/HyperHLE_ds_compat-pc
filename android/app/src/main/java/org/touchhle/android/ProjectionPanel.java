package org.touchhle.android;
import android.graphics.Canvas;
import android.graphics.Color;
import android.graphics.Paint;
import android.os.SystemClock;
import java.util.WeakHashMap;
/** A local Canvas panel, with no bitmap allocation or scene readback. */
final class ProjectionPanel {
    private static final Paint paint=new Paint(Paint.ANTI_ALIAS_FLAG);
    private static final class Clock {long start,last;Clock(long now){start=last=now;}}
    private static final WeakHashMap<Object,Clock> clocks=new WeakHashMap<>();
    static float draw(Canvas c,Object key,OverlayStyle.Layer style,float width,float height,float opacity){
        String mode=style.string("projection","");if(mode.isEmpty())return 1;
        long now=SystemClock.uptimeMillis();Clock clock=clocks.get(key);
        if(clock==null||now-clock.last>200){clock=new Clock(now);clocks.put(key,clock);}clock.last=now;
        long elapsed=now-clock.start;boolean animate=!Boolean.FALSE.equals(style.get("projectionAnimation"));
        float p=animate?Math.min(6,elapsed/50)/6f:1f;
        if(!Boolean.FALSE.equals(style.get("projectionBackground"))&&p>0){
            float unit=Math.max(1,Math.min(4,height/160));
            c.save();
            if("scan".equals(mode)){c.clipRect(width*(1-p)*.5f,0,width*(1+p)*.5f,height);}
            else if("segmented".equals(mode)){c.clipRect(0,0,width,height*p);}
            paint.setStyle(Paint.Style.FILL);paint.setColor(Color.rgb(5,27,38));
            paint.setAlpha(Math.round(128*opacity*("minimal".equals(mode)?p:1)));c.drawRect(0,0,width,height,paint);
            paint.setColor(Color.rgb(110,211,228));paint.setAlpha(Math.round(148*opacity));paint.setStrokeWidth(unit);
            c.drawLine(0,unit*.5f,width,unit*.5f,paint);c.drawLine(0,height-unit*.5f,width,height-unit*.5f,paint);
            c.drawLine(unit,0,unit,height/6,paint);c.drawLine(width-unit,0,width-unit,height/6,paint);
            c.drawLine(unit,height*5/6,unit,height,paint);c.drawLine(width-unit,height*5/6,width-unit,height,paint);
            if(!"minimal".equals(mode)){
                paint.setAlpha(Math.round(12*opacity));
                for(float y=0;y<height;y+=unit*4)c.drawLine(0,y,width,y,paint);
                if(p<1){paint.setAlpha(Math.round(164*opacity));c.drawLine(0,height*p,width,height*p,paint);}
            }c.restore();
        }
        return animate?Math.max(0,Math.min(1,(elapsed-220)/160f)):1;
    }
}
