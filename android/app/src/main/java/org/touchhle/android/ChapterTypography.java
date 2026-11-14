package org.touchhle.android;

import android.content.Context;
import android.graphics.Canvas;
import android.graphics.Color;
import android.graphics.Paint;
import android.graphics.Typeface;

/** The twelve titles share the largest single-line size in each language. */
final class ChapterTypography {
    private static final float TRACKING_EM=.10f;
    private final Paint cn = new Paint(Paint.ANTI_ALIAS_FLAG);
    private final Paint en = new Paint(Paint.ANTI_ALIAS_FLAG);
    private final String[] chinese = new String[12], english = new String[12];
    private final String[] a=new String[12],b=new String[12],numbers=new String[12];
    private long measuredRevision=Long.MIN_VALUE;
    private float measuredWidth=-1,measuredMax=-1,cs,es;
    ChapterTypography(Context context) throws Exception {
        OverlayTexts.load(context);
        cn.setTypeface(RoleFonts.get(context).title);
        en.setTypeface(RoleFonts.get(context).title);
        for (int n=1;n<=12;n++) {
            String id=String.format(java.util.Locale.ROOT,"CHAPTER_%02d_GAME_TITLE",n);
            chinese[n-1]=OverlayTexts.value(id,false);english[n-1]=OverlayTexts.value(id,true);
            numbers[n-1]=Integer.toString(n);
        }
    }
    private static float measure(Paint p,String text,float size) {
        RoleFonts.setTextHeight(p,size);p.setFakeBoldText(false);
        int first=text.offsetByCodePoints(0,Math.min(1,text.codePointCount(0,text.length())));
        float tail=p.measureText(text,first,text.length());
        p.setFakeBoldText(true);float head=p.measureText(text,0,first);p.setFakeBoldText(false);
        return head+tail+Math.max(0,text.codePointCount(0,text.length())-1)*size*TRACKING_EM+Math.max(1,size*.025f);
    }
    private static float fit(Paint p,String[] titles,float width,float max) {
        float lo=1,hi=Math.max(1,max);
        for(int i=0;i<18;i++) {float mid=(lo+hi)*.5f;boolean ok=true;
            for(String title:titles) if(measure(p,title,mid)>width){ok=false;break;}
            if(ok){lo=mid;}else{hi=mid;}
        }return lo;
    }
    private static void line(Canvas c,Paint p,String text,float size,float width,float top,float opacity,boolean highlight) {
        if(text.isEmpty())return;
        float measured=measure(p,text,size),x=(width-measured)*.5f;
        RoleFonts.setTextHeight(p,size);float baseline=top-p.ascent();p.setAlpha(Math.round(opacity*255));
        for(int i=0,n=0;i<text.length();n++){
            int next=text.offsetByCodePoints(i,1);boolean first=highlight&&n==0;
            p.setFakeBoldText(first);p.setColor(first?Color.rgb(235,42,48):Color.WHITE);p.setAlpha(Math.round(opacity*255));
            c.drawText(text,i,next,x,baseline,p);x+=p.measureText(text,i,next)+size*TRACKING_EM;i=next;
        }p.setFakeBoldText(false);
    }
    // One identity per native playback, retained across per-frame Layer objects.
    // A fresh Layer as the clock key restarted the 220 ms text gate every draw.
    private Object projectionKey = new Object();
    void beginPlayback() { projectionKey = new Object(); }

    void draw(Canvas canvas,int chapter,float opacity,float left,float top,float cw,float ch) {
        String id=String.format(java.util.Locale.ROOT,"CHAPTER_%02d_GAME_TITLE",chapter);
        OverlayStyle.Layer style=OverlayStyle.layer("chapter",id);if(!style.enabled())return;
        float width=cw*style.number("width",70,10,96)/100f,max=ch*style.number("maxHeight",35,10,80)/100f;
        float pad=Math.max(2,width*.018f),label=Math.max(16,max*.189f),gap=max*.035f;
        float room=Math.max(2,max-pad*2-label-gap*2);
        if(measuredRevision!=OverlayStyle.revision()||measuredWidth!=width||measuredMax!=max){
            for(int i=0;i<12;i++){String key=String.format(java.util.Locale.ROOT,"CHAPTER_%02d_GAME_TITLE",i+1);a[i]=OverlayStyle.text(key,chinese[i],false);b[i]=OverlayStyle.text(key,english[i],true);}
            cs=fit(cn,a,width-pad*2,room*.57f);es=fit(en,b,width-pad*2,room*.43f);
            measuredRevision=OverlayStyle.revision();measuredWidth=width;measuredMax=max;
        }
        float height=pad*2+label+gap*2+cs+es;
        float x=Math.max(left,Math.min(left+cw-width,left+cw*style.number("x",50,0,100)/100f-width*.5f));
        float y=Math.max(top,Math.min(top+ch-height,top+ch*style.number("y",39,0,100)/100f-height*.5f));
        canvas.save();canvas.translate(x,y);
        opacity*=ProjectionPanel.draw(canvas,projectionKey,style,width,height,opacity);
        String number=numbers[chapter-1];
        cn.setFakeBoldText(false);RoleFonts.setTextHeight(cn,label*.70f);float lw=cn.measureText("第"),rw=cn.measureText("章");
        en.setFakeBoldText(false);RoleFonts.setTextHeight(en,label);float nw=en.measureText(number),gapX=label*.22f;
        float hx=(width-lw-gapX-nw-gapX-rw)*.5f;
        en.setColor(Color.WHITE);en.setAlpha(Math.round(opacity*255));
        canvas.drawText(number,hx+lw+gapX,pad-en.ascent(),en);
        cn.setColor(Color.WHITE);cn.setAlpha(Math.round(opacity*255));
        canvas.drawText("第",hx,pad+label*.15f-cn.ascent(),cn);
        canvas.drawText("章",hx+lw+gapX+nw+gapX,pad+label*.15f-cn.ascent(),cn);
        line(canvas,cn,a[chapter-1],cs,width,pad+label+gap,opacity,true);
        line(canvas,en,b[chapter-1],es,width,pad+label+gap*2+cs,opacity,true);
        canvas.restore();
    }
}
