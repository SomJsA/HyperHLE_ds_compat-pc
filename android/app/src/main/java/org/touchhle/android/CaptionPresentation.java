package org.touchhle.android;

import android.graphics.Canvas;
import android.graphics.Color;
import android.graphics.Paint;
import android.graphics.Typeface;
import android.text.Layout;
import android.text.StaticLayout;
import android.text.TextPaint;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.regex.Matcher;
import java.util.regex.Pattern;

/** Plans only when text/style/viewport changes; page choice uses the guest clock. */
final class CaptionPresentation {
    private static final Pattern CLAUSE=Pattern.compile("[^。！？；，、]+[。！？；，、]*|[。！？；，、]+");
    private static final class Plan {final ArrayList<String> text=new ArrayList<>();final ArrayList<StaticLayout> pages=new ArrayList<>();float height;}
    private final LinkedHashMap<String,Plan> cache=new LinkedHashMap<>();
    private final Paint panel=new Paint(Paint.ANTI_ALIAS_FLAG);
    private String lastEvent;
    private Layout.Alignment alignment=Layout.Alignment.ALIGN_CENTER;
    private static String wrap(String value,TextPaint paint,int width){
        ArrayList<String> units=new ArrayList<>(),lines=new ArrayList<>();
        for(int i=0;i<value.length();){int next=value.offsetByCodePoints(i,1);String unit=value.substring(i,next);
            if("，。！？；：、…）》」』】,.!?;:)".contains(unit)&&!units.isEmpty()&&!"\n".equals(units.get(units.size()-1))){
                int last=units.size()-1;units.set(last,units.get(last)+unit);
            }else units.add(unit);i=next;
        }
        String line="";
        for(String unit:units){
            if("\n".equals(unit)){lines.add(line);line="";continue;}
            if(!line.isEmpty()&&paint.measureText(line+unit)>width){lines.add(line);line="";}
            line+=unit;
        }
        if(!line.isEmpty()||lines.isEmpty())lines.add(line);
        return android.text.TextUtils.join("\n",lines);
    }
    private StaticLayout layout(String value,TextPaint paint,int width,float spacing){
        return new StaticLayout(wrap(value,paint,width),new TextPaint(paint),width,alignment,spacing,0,false);
    }
    private void split(Plan p,String value,TextPaint paint,int width,float spacing){
        Matcher parts=CLAUSE.matcher(value);String page="";
        while(parts.find()){
            String part=parts.group();
            if(layout(page+part,paint,width,spacing).getLineCount()<=2){page+=part;continue;}
            if(!page.isEmpty()){p.text.add(page);page="";}
            ArrayList<String> units=new ArrayList<>();
            for(int i=0;i<part.length();){int next=part.offsetByCodePoints(i,1);String c=part.substring(i,next);
                if("，。！？；：、…）》」』】,.!?;:)".contains(c)&&!units.isEmpty()){int last=units.size()-1;units.set(last,units.get(last)+c);}else units.add(c);i=next;
            }
            for(String unit:units){
                if(!page.isEmpty()&&layout(page+unit,paint,width,spacing).getLineCount()>2){p.text.add(page);page="";}
                page+=unit;
            }
        }
        if(!page.isEmpty()||p.text.isEmpty())p.text.add(page);
    }
    private static int pageAt(Plan p,long elapsed,long total){
        if(total<=0||p.pages.size()<2)return 0;
        long weight=0;for(String s:p.text)weight+=s.codePointCount(0,s.length())+5;
        long target=Math.min(total,elapsed)*weight,end=0;
        for(int i=0;i<p.text.size();i++){String s=p.text.get(i);end+=s.codePointCount(0,s.length())+5;if(target<total*end)return i;}
        return p.pages.size()-1;
    }
    void draw(Canvas canvas,OverlayStyle.Layer style,String original,Typeface font,float left,float top,float cw,float ch,
              float density,String selected,long timing,long snapshot,boolean preview){
        if(!style.enabled()||original==null||original.isEmpty())return;
        long total=timing>>>32,elapsed=timing&0xffffffffL;boolean paged="paged".equals(selected)&&total>0;
        boolean signal=SignalText.marked(original)&&!Boolean.FALSE.equals(style.get("signalAnimation"));
        String value=SignalText.clean(original);float unit=ch/320f;
        float width=Math.min(cw-4*unit,cw*style.number("width",86,10,96)/100f),pad=style.number("padding",5,0,20)*unit;
        float spacing=style.number("lineHeight",1.12f,1,2),height=ch*style.number("maxHeight",32,10,80)/100f;
        float pixels=style.get("fontSizeSp") instanceof Number?style.number("fontSizeSp",22,14,44)*density
                :style.number("fontSize",11.5f,3,36)*unit*RoleFonts.heightToEm(font);
        if(paged)pixels*=1.25f;
        alignment="left".equals(style.string("align","center"))?Layout.Alignment.ALIGN_NORMAL
                :"right".equals(style.string("align","center"))?Layout.Alignment.ALIGN_OPPOSITE:Layout.Alignment.ALIGN_CENTER;
        String key=alignment+":"+System.identityHashCode(font)+":"+value+":"+width+":"+pad+":"+pixels+":"+height+":"+spacing+":"+paged;
        Plan plan=cache.get(key);int inner=Math.max(1,Math.round(width-2*pad));
        if(plan==null){
            TextPaint paint=new TextPaint(Paint.ANTI_ALIAS_FLAG);paint.setTypeface(font);paint.setTextSize(pixels);paint.setShadowLayer(unit*.5f,0,unit*.5f,Color.BLACK);
            plan=new Plan();
            if(paged){
                while(pixels>2&&layout("汉\n汉",paint,inner,spacing).getHeight()>height-2*pad){pixels-=.5f;paint.setTextSize(pixels);}
                split(plan,value,paint,inner,spacing);
                plan.height=layout("汉\n汉",paint,inner,spacing).getHeight();
            }else{
                float lo=1,hi=pixels;
                for(int i=0;i<16;i++){float mid=(lo+hi)*.5f;paint.setTextSize(mid);StaticLayout check=layout(value,paint,inner,spacing);
                    if(check.getLineCount()<=3&&check.getHeight()<=height-2*pad)lo=mid;else hi=mid;}
                paint.setTextSize(lo);plan.text.add(value);
            }
            for(String text:plan.text){StaticLayout page=layout(text,paint,inner,spacing);plan.pages.add(page);plan.height=Math.max(plan.height,page.getHeight());}
            if(cache.size()>=16)cache.remove(cache.keySet().iterator().next());cache.put(key,plan);
        }
        int index=pageAt(plan,elapsed,total);StaticLayout page=plan.pages.get(index);float box=plan.height+2*pad;
        float x=left+cw*style.number("x",50,0,100)/100f-width/2;
        float anchor=top+ch*style.number("y",93,0,100)/100f;
        float y=anchor-("bottom-center".equals(style.string("anchor","bottom-center"))?box:box/2);
        x=Math.max(left,Math.min(x,left+cw-width));y=Math.max(top,Math.min(y,top+ch-box));
        panel.setStyle(Paint.Style.FILL);panel.setColor(style.color("background","#000000"));panel.setAlpha(Math.round(255*style.number("opacity",.45f,0,1)));
        canvas.drawRoundRect(x,y,x+width,y+box,style.number("radius",2,0,20)*unit,style.number("radius",2,0,20)*unit,panel);
        float stroke=style.number("strokeOpacity",0,0,1);
        if(stroke>0){panel.setStyle(Paint.Style.STROKE);panel.setStrokeWidth(Math.max(1,unit*.35f));panel.setColor(style.color("stroke","#56dce8"));panel.setAlpha(Math.round(255*stroke));canvas.drawRoundRect(x,y,x+width,y+box,style.number("radius",2,0,20)*unit,style.number("radius",2,0,20)*unit,panel);}
        page.getPaint().setColor(style.color("color","#ffffff"));page.getPaint().setAlpha(255);
        canvas.save();canvas.translate(x+pad,y+pad+(plan.height-page.getHeight())*.5f);SignalText.draw(canvas,page,signal,unit);canvas.restore();
        String event=snapshot+":"+paged+":"+index;
        if(!preview&&!event.equals(lastEvent)){
            android.util.Log.i("touchHLE","[DScaption-page-view] serial="+(snapshot>>>32)+" mode="+(paged?"paged":"three")+" page="+(index+1)+"/"+plan.pages.size()+" elapsed="+elapsed+" duration="+total);
            lastEvent=event;
        }
    }
}
