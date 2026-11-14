package org.touchhle.android;

import android.text.TextPaint;

/** Same two-line balancing and word boundaries as PC ds_host_overlay. */
final class ObjectiveLayout {
    private static final String PUNCT="，。！？；：、,.!?;:";
    private static final String SEMANTIC=PUNCT+"个的和与并及在为到务道点部板统间房罐机车站堆心室区台端池库线舍闸纽器域";
    private static final String[] WORDS={"服务器","机房","隧道","装置","维护","枢纽","控制","面板","系统","房间","区域","接入点","外部","反应堆","升降机","缆车","列车站","供电","电源箱","电池","净化罐","密封","检测","蓄水池","仓库","处理区","清理区","行政区","办公室","终端","平台","路线","上层","下层","内部","交通","跨区","封锁","气闸","食品","培育舱","岩石取芯器","静滞模块","动态牵引模块","等离子切割器","切割锯","线性枪","泰坦","碎片","尸变体","进入","前往","乘车","操作","找到","寻找","取得","摧毁","破坏","解锁","恢复","重置","穿过","通过","逃离","击败","直面","杀死","清除","调查","绕过","绕行","继续","返回","启动","打开","使用","完成"};
    static String wrap(String source,TextPaint paint,float width) {
        String text=source.trim();if(paint.measureText(text)<=width)return text;
        byte[] mask=new byte[text.length()+1];
        for(String word:WORDS)for(int at=text.indexOf(word);at>=0;at=text.indexOf(word,at+1)){
            mask[at]|=1;mask[at+word.length()]|=1;for(int p=at+1;p<at+word.length();p++)mask[p]|=2;
        }
        int best=-1;float score=Float.MAX_VALUE;
        for(int at=1;at<text.length();at++) {
            char left=text.charAt(at-1),right=text.charAt(at);
            if(Character.isLowSurrogate(right)||ascii(left)&&ascii(right)||PUNCT.indexOf(right)>=0)continue;
            float a=paint.measureText(text,0,at),b=paint.measureText(text,at,text.length());if(a>width||b>width)continue;
            float bonus=SEMANTIC.indexOf(left)>=0?width*.25f:(mask[at]&1)!=0?width*.08f:0;
            float value=Math.abs(a-b)+((mask[at]&2)!=0?width*.5f:0)-bonus;
            if(value<score){score=value;best=at;}
        }
        return best<0?text:text.substring(0,best)+"\n"+text.substring(best);
    }
    private static boolean ascii(char c){return c>='0'&&c<='9'||c>='A'&&c<='Z'||c>='a'&&c<='z';}
}
