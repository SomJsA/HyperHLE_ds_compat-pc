/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. https://mozilla.org/MPL/2.0/ */
package org.touchhle.android;

import android.app.AlertDialog;
import android.content.Context;
import android.content.SharedPreferences;
import android.graphics.Canvas;
import android.graphics.Color;
import android.graphics.Paint;
import android.text.Layout;
import android.text.StaticLayout;
import android.text.TextPaint;
import android.graphics.Typeface;
import android.view.View;
import android.widget.CheckBox;
import android.widget.EditText;
import android.widget.LinearLayout;
import android.widget.ScrollView;
import android.widget.SeekBar;
import android.widget.TextView;
import org.json.JSONObject;

/** Native-timed captions and live display preferences; no input interception. */
final class SubtitleOverlay {
    private static native long nativeSnapshot();
    private static native long nativeTiming(long snapshot);
    private static native void nativeSetReady(boolean value);
    private static final String PREVIEW = "字幕显示预览：位置、字号、文字颜色与底板透明度。\n第二行会按可用宽度自动换行。";
    private final Context context;
    private final View owner;
    private final SharedPreferences prefs;
    private final TextPaint text = new TextPaint(Paint.ANTI_ALIAS_FLAG);
    private final Paint panel = new Paint(Paint.ANTI_ALIAS_FLAG);
    private final String[] captions;
    private final String[] ids;
    private final Typeface font;
    private boolean bridgeReady;
    private boolean preview;
    private long lastSnapshot = Long.MIN_VALUE;
    private long layoutKey = Long.MIN_VALUE;
    private String cachedText;
    private StaticLayout layout;
    private float layoutWidth;
    private boolean enabled;
    private int size, width, x, bottom, opacity, color;
    private String longMode;
    private JSONObject localOverrides;
    private final CaptionPresentation presentation=new CaptionPresentation();

    SubtitleOverlay(Context context, View owner, Typeface font) throws Exception {
        this.context = context;
        this.owner = owner;
        this.font = font;
        prefs = context.getSharedPreferences("overlay_display", Context.MODE_PRIVATE);
        text.setTypeface(font);
        text.setShadowLayer(2f, 1f, 2f, Color.BLACK);
        OverlayTexts.load(context);
        captions = OverlayTexts.text;
        ids = OverlayTexts.ids;
        reload();
    }

    void reload() {
        OverlayStyle.Layer defaults=OverlayStyle.layer("subtitle", "");
        enabled = prefs.getBoolean("enabled", defaults.enabled());
        size = Math.max(14, Math.min(44, prefs.getInt("size", 22)));
        width = Math.max(50, Math.min(96, prefs.getInt("width", 86)));
        x = Math.max(-12, Math.min(12, prefs.getInt("x", 0)));
        bottom = Math.max(2, Math.min(30, prefs.getInt("bottom", 7)));
        opacity = Math.max(0, Math.min(100, prefs.getInt("opacity", 45)));
        color = prefs.getInt("color", Color.WHITE) | 0xff000000;
        longMode=prefs.getString("long_mode",defaults.string("longTextMode","three"));
        if(!"three".equals(longMode))longMode="paged";
        try{
            localOverrides=new JSONObject();
            if(prefs.contains("enabled"))localOverrides.put("enabled",enabled);
            if(prefs.contains("width"))localOverrides.put("width",width);
            if(prefs.contains("x"))localOverrides.put("x",50+x);
            if(prefs.contains("bottom"))localOverrides.put("y",100-bottom);
            if(prefs.contains("opacity"))localOverrides.put("opacity",opacity/100f);
            if(prefs.contains("color"))localOverrides.put("color",String.format(java.util.Locale.ROOT,"#%06X",color&0xffffff));
            if(prefs.contains("size_reference"))localOverrides.put("fontSize",Math.max(3f,Math.min(36f,prefs.getFloat("size_reference",11.5f))));
            else if(prefs.contains("size"))localOverrides.put("fontSizeSp",size); // Preserve an explicit legacy user choice.
            if(prefs.contains("long_mode"))localOverrides.put("longTextMode",longMode);
        }catch(org.json.JSONException error){throw new IllegalStateException(error);}
        layout = null;
        owner.invalidate();
    }

    void stop() {
        nativeSetReady(false);
        bridgeReady = false;
        lastSnapshot = Long.MIN_VALUE;
    }

    void draw(Canvas canvas, boolean running, float left, float top, float cw, float ch) {
        boolean ready = running && (OverlayStyle.external() ? OverlayStyle.layer("subtitle", "").enabled() : enabled) && captions.length > 1;
        if (ready != bridgeReady) { nativeSetReady(ready); bridgeReady = ready; }
        long snapshot = ready ? nativeSnapshot() : 0;
        int index = (int)snapshot;
        String value = preview ? PREVIEW : index > 0 && index < captions.length ? captions[index] : null;
        if (snapshot != lastSnapshot) {
            if (snapshot != 0) { android.util.Log.i("touchHLE", "[DSsubtitle-view] serial=" + (snapshot >>> 32) + " index=" + index); }
            lastSnapshot = snapshot;
        }
        if (value == null || (!preview && !running)) { return; }
        String id=index>0&&index<ids.length?ids[index]:"";
        OverlayStyle.Layer style=OverlayStyle.layer("subtitle",id);
        if(!OverlayStyle.external())style=new OverlayStyle.Layer(style.sources[0],style.sources[1],style.sources[2],localOverrides);
        String mode=prefs.contains("long_mode")?longMode:style.string("longTextMode","three");
        String caption=preview?value:OverlayStyle.text(id,value,false);
        presentation.draw(canvas,style,caption,font,left,top,cw,ch,context.getResources().getDisplayMetrics().scaledDensity,
                mode,preview?(12_000L<<32):nativeTiming(snapshot),snapshot,preview);
    }

    AlertDialog settings(Runnable closed) {
        preview = true;
        owner.invalidate();
        LinearLayout controls = new LinearLayout(context);
        controls.setOrientation(LinearLayout.VERTICAL);
        controls.setPadding(24, 8, 24, 8);
        TextView note = new TextView(context);
        note.setText("对白使用当前校订稿。设置实时预览并自动保存。长句可选大字分段或整段三行，按原对白片段时长显示。");
        controls.addView(note);
        CheckBox toggle = new CheckBox(context);
        toggle.setText("启用高清字幕接管（关闭后恢复原字幕）"); toggle.setChecked(enabled);
        toggle.setOnCheckedChangeListener((button, checked) -> { prefs.edit().putBoolean("enabled", checked).apply(); reload(); });
        controls.addView(toggle);
        TextView modeLabel=new TextView(context);modeLabel.setText("长句显示方式");controls.addView(modeLabel);
        android.widget.Spinner modePicker=new android.widget.Spinner(context);
        android.widget.ArrayAdapter<String> modes=new android.widget.ArrayAdapter<>(context,android.R.layout.simple_spinner_item,new String[]{"大字分段","整段三行"});
        modes.setDropDownViewResource(android.R.layout.simple_spinner_dropdown_item);modePicker.setAdapter(modes);modePicker.setSelection("three".equals(longMode)?1:0);
        modePicker.setOnItemSelectedListener(new android.widget.AdapterView.OnItemSelectedListener(){
            public void onItemSelected(android.widget.AdapterView<?> parent,View view,int position,long id){
                String mode=position==1?"three":"paged";
                if(!mode.equals(longMode)){prefs.edit().putString("long_mode",mode).apply();reload();}
            }
            public void onNothingSelected(android.widget.AdapterView<?> parent){}
        });controls.addView(modePicker);
        TextView sizeLabel=new TextView(context);controls.addView(sizeLabel);
        float reference=prefs.getFloat("size_reference",OverlayStyle.layer("subtitle","").number("fontSize",11.5f,3,36));
        sizeLabel.setText(prefs.contains("size")?"保留原字号："+size+" sp；调整后使用 PC 参考字号":"字号（与 PC 共用参考值）："+reference);
        SeekBar sizeSlider=new SeekBar(context);sizeSlider.setMax(66);sizeSlider.setProgress(Math.round((reference-3f)*2));
        sizeSlider.setOnSeekBarChangeListener(new SeekBar.OnSeekBarChangeListener(){
            public void onProgressChanged(SeekBar bar,int progress,boolean user){if(!user)return;float value=3f+progress*.5f;
                sizeLabel.setText("字号（与 PC 共用参考值）："+value);prefs.edit().remove("size").putFloat("size_reference",value).apply();reload();}
            public void onStartTrackingTouch(SeekBar bar){}
            public void onStopTrackingTouch(SeekBar bar){}
        });controls.addView(sizeSlider);
        slider(controls, "文字区域宽度", "width", 50, 96, width, "%");
        slider(controls, "左右偏移", "x", -12, 12, x, "%");
        slider(controls, "距画面底部", "bottom", 2, 30, bottom, "%");
        slider(controls, "黑色底板不透明度", "opacity", 0, 100, opacity, "%");
        EditText hex = new EditText(context);
        hex.setSingleLine(true); hex.setHint("文字颜色 #RRGGBB");
        hex.setText(String.format(java.util.Locale.ROOT, "#%06X", color & 0xffffff));
        hex.addTextChangedListener(new android.text.TextWatcher() {
            public void beforeTextChanged(CharSequence s, int start, int count, int after) {}
            public void onTextChanged(CharSequence s, int start, int before, int count) {
                if (s.toString().matches("#[0-9a-fA-F]{6}")) {
                    prefs.edit().putInt("color", Color.parseColor(s.toString())).apply(); reload();
                }
            }
            public void afterTextChanged(android.text.Editable s) {}
        });
        controls.addView(hex);
        controls.setFocusableInTouchMode(true);controls.requestFocus();
        ScrollView scroll = new ScrollView(context); scroll.addView(controls);
        // Keep the lower screen visible so the actual game view shows the preview.
        Runnable finish = () -> { preview = false; owner.invalidate(); closed.run(); };
        AlertDialog dialog = new AlertDialog.Builder(context).setTitle("字幕显示 · 实时预览")
                .setView(scroll).setPositiveButton("完成", (d, w) -> finish.run())
                .setNegativeButton("恢复默认", (d, w) -> { prefs.edit().clear().apply(); reload(); finish.run(); })
                .setOnCancelListener(d -> finish.run()).create();
        dialog.setOnShowListener(d -> {
            scroll.scrollTo(0,0);
            android.view.Window window = dialog.getWindow();
            if (window != null) {
                window.setGravity(android.view.Gravity.TOP | android.view.Gravity.CENTER_HORIZONTAL);
                window.setLayout((int)(owner.getWidth() * .62f), (int)(owner.getHeight() * .58f));
                window.clearFlags(android.view.WindowManager.LayoutParams.FLAG_DIM_BEHIND);
            }
        });
        return dialog;
    }

    private void slider(LinearLayout parent, String title, String key, int min, int max, int value, String unit) {
        TextView label = new TextView(context); label.setText(title + "：" + value + unit); parent.addView(label);
        SeekBar slider = new SeekBar(context); slider.setMax(max - min); slider.setProgress(value - min);
        slider.setOnSeekBarChangeListener(new SeekBar.OnSeekBarChangeListener() {
            public void onProgressChanged(SeekBar bar, int progress, boolean user) {
                int selected = min + progress; label.setText(title + "：" + selected + unit);
                prefs.edit().putInt(key, selected).apply(); reload();
            }
            public void onStartTrackingTouch(SeekBar bar) {}
            public void onStopTrackingTouch(SeekBar bar) {}
        });
        parent.addView(slider);
    }
}
