package org.touchhle.android;
import android.graphics.Canvas;
import android.graphics.Color;
import android.text.StaticLayout;
import android.text.TextPaint;
import android.os.SystemClock;
import java.util.WeakHashMap;
final class SignalText {
    private static final class Clock {long start,last;Clock(long now){start=last=now;}}
    private static final WeakHashMap<StaticLayout,Clock> clocks=new WeakHashMap<>();
    static boolean marked(String text){return text!=null&&(text.contains("（干扰）")||text.contains("(干扰)")||text.contains("[干扰]")||text.contains("(static)")||text.contains("(Static)"));}
    static String clean(String text){return text==null?null:text.replace("（干扰）"," ▒▒ ").replace("(干扰)"," ▒▒ ").replace("[干扰]"," ▒▒ ").replace("(static)"," ▒▒ ").replace("(Static)"," ▒▒ ");}
    static int phase(long elapsed){int p=(int)(elapsed/80%32);return (p>=12&&p<=17)||p==24||p>=29?p:0;}
    static void draw(Canvas canvas,StaticLayout layout,boolean marked,float unit){
        if(!marked){layout.draw(canvas);return;}
        long now=SystemClock.uptimeMillis();Clock clock=clocks.get(layout);
        if(clock==null||now-clock.last>200){clock=new Clock(now);clocks.put(layout,clock);}clock.last=now;
        int phase=phase(now-clock.start);if(phase==24)return;
        if(phase>=12&&phase<=14){canvas.save();canvas.translate((phase%2==0?2:-2)*unit,phase==13?unit:0);layout.draw(canvas);canvas.restore();return;}
        TextPaint p=layout.getPaint();int color=p.getColor(),alpha=p.getAlpha();
        if(phase>=15&&phase<=17){
            // Preserve original glyph advances and line baselines while garbling.
            CharSequence text=layout.getText();String[] wide={"▧","□","◇","▒"},ascii={"#","%","?","@"};
            for(int line=0;line<layout.getLineCount();line++){
                for(int i=layout.getLineStart(line);i<layout.getLineEnd(line);){
                    int cp=Character.codePointAt(text,i),next=i+Character.charCount(cp);
                    String glyph=new String(Character.toChars(cp));
                    if(!Character.isWhitespace(cp)&&Character.isLetterOrDigit(cp)){glyph=(cp<128?ascii:wide)[(i+phase)%4];}
                    if(!Character.isWhitespace(cp)){
                        float x=layout.getPrimaryHorizontal(i),end=layout.getPrimaryHorizontal(next);
                        if(next==layout.getLineEnd(line))end=x+p.measureText(text,i,next);
                        canvas.drawText(glyph,x+(end-x-p.measureText(glyph))*.5f,layout.getLineBaseline(line),p);
                    }i=next;
                }
            }return;
        }
        layout.draw(canvas);
        if(phase>=29){
            float direction=phase%2==0?1:-1;
            p.setColor(Color.rgb(100,230,255));p.setAlpha(Math.round(alpha*.38f));
            canvas.save();canvas.translate(4*unit*direction,0);layout.draw(canvas);canvas.restore();
            p.setColor(Color.rgb(255,100,110));p.setAlpha(Math.round(alpha*.26f));
            canvas.save();canvas.translate(-2*unit*direction,unit);layout.draw(canvas);canvas.restore();
            p.setColor(color);p.setAlpha(alpha);
        }
    }
}
