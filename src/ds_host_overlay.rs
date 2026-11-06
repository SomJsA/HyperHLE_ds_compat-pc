//! Desktop text layers shared by CPU composition and final GPU presentation. The scene, timing
//! and input coordinate systems stay untouched. JSON is shared with Android.
use crate::font::{Font, TextAlignment, WrapMode};
#[path = "ds_overlay_blit.rs"]
mod blit;
use blit::CachedCard;
#[path = "ds_overlay_gpu.rs"]
mod gpu;
pub(crate) use gpu::State as GpuState;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};

const OBJECTIVE_SIZE_INSET_PT: f32 = 2.0;

struct Host {
    style: Value,
    texts: Vec<(String, String)>,
    english: HashMap<String,String>,
    started: Instant,
    signal_id: String,
    font: Option<Font>,
    title_font: Font,
    checked: Instant,
    stamp: Option<SystemTime>,
    cache: HashMap<String, Arc<CachedCard>>,
    panels: HashMap<(u32,u32,String,u8),Arc<CachedCard>>,
    entries: HashMap<String,(String,Instant,Instant)>,
    caption_plans:HashMap<String,crate::ds_caption_layout::Plan>,
    caption_event:Option<(u64,usize,bool)>,
    problem_notice:Option<(String,u32,u32,Arc<CachedCard>)>,
}
fn host() -> &'static Mutex<Host> {
    static HOST: OnceLock<Mutex<Host>> = OnceLock::new();
    HOST.get_or_init(|| {
        let map: Value = serde_json::from_str(include_str!("ds_overlay_texts.json")).unwrap();
        let texts = map["entries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| {
                (
                    e["id"].as_str().unwrap().to_owned(),
                    e["text"].as_str().unwrap().to_owned(),
                )
            })
            .collect();
        let font = Some(crate::ds_font_roles::load(crate::ds_font_roles::Role::Story));
        echo!(
            "[DSoverlay-host] CPU composition ready={} entries=995",
            font.is_some()
        );
        Mutex::new(Host {
            style: serde_json::from_str(include_str!("ds_overlay_default.json")).unwrap(),
            texts,
            english:map["entries"].as_array().unwrap().iter().map(|e|(e["id"].as_str().unwrap().to_owned(),e["english"].as_str().unwrap_or("").to_owned())).collect(),
            started:Instant::now(),
            signal_id:String::new(),
            font,
            title_font: crate::ds_font_roles::load(crate::ds_font_roles::Role::Frontend),
            checked: Instant::now() - Duration::from_secs(2),
            stamp: None,
            cache: HashMap::new(),
            panels:HashMap::new(),
            entries:HashMap::new(),
            caption_plans:HashMap::new(),
            caption_event:None,
            problem_notice:None,
        })
    })
}
pub(crate) fn prepare() {
    let _stutter_span=crate::ds_stutter_probe::Span::new(7,0);
    if std::env::var_os("TOUCHHLE_DS_CHAPTER_OVERLAY").is_none()
        && std::env::var_os("TOUCHHLE_DS_SUBTITLE_OVERLAY").is_none()
        && std::env::var_os("TOUCHHLE_DS_TUTORIAL_OVERLAY").is_none()
        && std::env::var_os("TOUCHHLE_DS_OBJECTIVE_OVERLAY").is_none()
        && std::env::var_os("TOUCHHLE_DS_QTE_OVERLAY").is_none()
    {
        return;
    }
    let Ok(mut h) = host().lock() else { return };
    if h.checked.elapsed() > Duration::from_millis(750) {
        h.checked = Instant::now();
        let path = std::env::var("TOUCHHLE_DS_OVERLAY_STYLE")
            .unwrap_or_else(|_| "overlay-style.json".to_owned());
        let stamp = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
        if stamp != h.stamp {
            if let Ok(bytes) = std::fs::read(&path) {
                if bytes.len() <= 1024 * 1024 {
                    if let Ok(value) = serde_json::from_slice::<Value>(&bytes) {
                        if value["schema"] == 1 && value["layers"].is_object() {
                            h.style = value;
                            h.cache.clear();
                            h.caption_plans.clear();
                            echo!("[DSoverlay-config] loaded {}", path);
                        }
                    }
                }
            }
            h.stamp = stamp;
        }
    }
    let ready = |layer: &str| {
        h.font.is_some()
            && h.style["layers"][layer]["enabled"]
                .as_bool()
                .unwrap_or(true)
    };
    crate::ds_chapter_overlay::host_ready(ready("chapter"));
    crate::ds_subtitle_overlay::host_ready(ready("subtitle"));
    crate::ds_objective_overlay::host_ready(ready("objective"));
    crate::ds_tutorial_overlay::host_ready(ready("tutorial"));
}

/// Whether the current frame needs the CPU-composition path. Frontend key
/// hints are drawn directly in the final present pass, so they must not force
/// a full readback and CA recomposition.
pub(crate) fn active() -> bool {
    visible() && !gpu::enabled()
}

pub(crate) fn observe_gpu_backend(fixed_function: bool) {
    gpu::observe_backend(fixed_function);
}
pub(crate) fn gpu_text_gate_enabled() -> bool { gpu::enabled() }

/// Both the EAGL presenter and the CA fullscreen-layer check must make the
/// same decision. Otherwise CPU text readback advances the guest without a
/// compositor draw or swap until the text disappears.
pub(crate) fn requires_cpu_composition(options: &crate::options::Options) -> bool {
    cpu_composition_policy(
        options.force_composition || std::env::var_os("TOUCHHLE_FORCE_COMPOSITE").is_some(),
        options.fast_present,
        active(),
    )
}

fn cpu_composition_policy(force: bool, fast: bool, cpu_text: bool) -> bool {
    cpu_text || (force && !fast)
}

fn visible() -> bool {
    // Shared ownership gate: hidden gameplay cards must not force a full
    // readback/CA pass while a native Store, Bench, menu or dialog is open.
    if cfg!(target_os="windows") && (crate::ds_frontend_controller::capture_menu()
        || crate::ds_frontend_controller::capture_scene_dialog()) { return false; }
    (crate::ds_chapter_overlay::host_snapshot() & (1 << 16) != 0)
        || crate::ds_objective_overlay::host_snapshot() > 0
        || crate::ds_subtitle_overlay::host_snapshot() > 0
        || crate::ds_tutorial_overlay::host_snapshot().is_some()
        || crate::ds_qte_overlay::snapshot().is_some()
}
fn number(v: &Value, key: &str, default: f32, min: f32, max: f32) -> f32 {
    v[key]
        .as_f64()
        .filter(|n| n.is_finite())
        .map(|n| (n as f32).clamp(min, max))
        .unwrap_or(default)
}
fn color(v: &Value, key: &str, default: &str) -> [u8; 3] {
    let s = v[key].as_str().unwrap_or(default);
    let n = s
        .strip_prefix('#')
        .filter(|s| s.len() == 6)
        .and_then(|s| u32::from_str_radix(s, 16).ok())
        .unwrap_or(0xffffff);
    [(n >> 16) as u8, (n >> 8) as u8, n as u8]
}
fn lines(font: &Font, size: f32, text: &str, width: f32) -> Vec<String> {
    let mut result = Vec::new();
    for paragraph in text.split('\n') {
        let mut line = String::new();
        for ch in paragraph.chars() {
            let candidate = format!("{line}{ch}");
            if !line.is_empty() && font.calculate_text_size(size, &candidate, None).0 > width {
                result.push(std::mem::take(&mut line));
                if !ch.is_whitespace() { line.push(ch); }
            } else { line.push(ch); }
        }
        result.push(line);
    }
    result
}

fn semantic_break(ch: char) -> bool {
    matches!(
        ch,
        '，' | '。' | '！' | '？' | '；' | '：' | '、' | ',' | '.' | '!' | '?' | ';' | ':'
            | '个' | '的' | '和' | '与' | '并' | '及' | '在' | '为' | '到'
            | '务' | '道' | '点' | '部' | '板' | '统' | '间' | '房' | '罐'
            | '机' | '车' | '站' | '堆' | '心' | '室' | '区' | '台' | '端'
            | '池' | '库' | '线' | '舍' | '闸' | '纽' | '器' | '域'
    )
}

const OBJECTIVE_WORDS: &[&str] = &[
    "服务器", "机房", "隧道", "装置", "维护", "枢纽", "控制", "面板", "系统",
    "房间", "区域", "接入点", "外部", "反应堆", "升降机", "缆车", "列车站",
    "供电", "电源箱", "电池", "净化罐", "密封", "检测", "蓄水池", "仓库",
    "处理区", "清理区", "行政区", "办公室", "终端", "平台", "路线", "上层",
    "下层", "内部", "交通", "跨区", "封锁", "气闸", "食品", "培育舱",
    "岩石取芯器", "静滞模块", "动态牵引模块", "等离子切割器", "切割锯",
    "线性枪", "泰坦", "碎片", "尸变体", "进入", "前往", "乘车", "操作",
    "找到", "寻找", "取得", "摧毁", "破坏", "解锁", "恢复", "重置",
    "穿过", "通过", "逃离", "击败", "直面", "杀死", "清除", "调查",
    "绕过", "绕行", "继续", "返回", "启动", "打开", "使用", "完成",
];

fn objective_word_mask(chars: &[char]) -> Vec<u8> {
    let mut mask = vec![0; chars.len() + 1];
    for word in OBJECTIVE_WORDS {
        let word: Vec<char> = word.chars().collect();
        if word.len() < 2 || word.len() > chars.len() {
            continue;
        }
        for start in 0..=chars.len() - word.len() {
            if chars[start..start + word.len()] != word {
                continue;
            }
            mask[start] |= 1;
            mask[start + word.len()] |= 1;
            for boundary in start + 1..start + word.len() {
                mask[boundary] |= 2;
            }
        }
    }
    mask
}

fn balanced_split(
    chars: &[char],
    width: f32,
    measure: impl Fn(&str) -> f32,
) -> Option<usize> {
    if chars.len() < 2 {
        return None;
    }
    let word_mask = objective_word_mask(chars);
    let mut best: Option<(f32, usize)> = None;
    for index in 1..chars.len() {
        if chars[index - 1].is_ascii_alphanumeric() && chars[index].is_ascii_alphanumeric() {
            continue;
        }
        if matches!(
            chars[index],
            '，' | '。' | '！' | '？' | '；' | '：' | '、' | ',' | '.' | '!' | '?' | ';' | ':'
        ) {
            continue;
        }
        let first: String = chars[..index].iter().collect();
        let second: String = chars[index..].iter().collect();
        let first_width = measure(&first);
        let second_width = measure(&second);
        if first_width > width || second_width > width {
            continue;
        }
        let semantic_bonus = if semantic_break(chars[index - 1]) {
            width * 0.25
        } else if word_mask[index] & 1 != 0 {
            width * 0.08
        } else {
            0.0
        };
        let word_break_penalty = if word_mask[index] & 2 != 0 {
            width * 0.5
        } else {
            0.0
        };
        let score =
            (first_width - second_width).abs() + word_break_penalty - semantic_bonus;
        if best.is_none_or(|(best_score, _)| score < best_score) {
            best = Some((score, index));
        }
    }
    best.map(|(_, index)| index)
}

fn objective_lines(font: &Font, size: f32, text: &str, width: f32) -> Vec<String> {
    let text = text.trim();
    if text.is_empty() {
        return vec![String::new()];
    }
    let measure = |value: &str| font.calculate_text_size(size, value, None).0;
    if measure(text) <= width {
        return vec![text.to_owned()];
    }
    let chars: Vec<char> = text.chars().collect();
    if let Some(index) = balanced_split(&chars, width, measure) {
        let first: String = chars[..index].iter().collect();
        let second: String = chars[index..].iter().collect();
        return vec![first, second];
    }
    lines(font, size, text, width)
}

fn objective_layout(
    font: &Font,
    requested_size: f32,
    text: &str,
    width: f32,
    height: f32,
    line_height: f32,
    max_size: f32,
    size_inset: f32,
) -> (f32, Vec<String>) {
    let fits = |lines: &[String], size: f32| {
        !lines.is_empty()
            && lines.len() <= 2
            && lines
                .iter()
                .all(|line| font.calculate_text_size(size, line, None).0 <= width)
            && lines.len() as f32 * size * 1.125 * line_height <= height
    };
    let requested_lines = objective_lines(font, requested_size, text, width);
    let mut lo = if fits(&requested_lines, requested_size) {
        requested_size
    } else {
        1.0
    };
    let mut hi = max_size.max(lo);
    for _ in 0..18 {
        let mid = (lo + hi) * 0.5;
        let lines = objective_lines(font, mid, text, width);
        if fits(&lines, mid) {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let fitted = (lo - size_inset).max(requested_size);
    let lines = objective_lines(font, fitted, text, width);
    (fitted, lines)
}
fn rounded_inside(x: f32, y: f32, w: f32, h: f32, radius: f32) -> bool {
    if x < 0.0 || y < 0.0 || x > w || y > h { return false; }
    let r = radius.min(w / 2.0).min(h / 2.0).max(0.0);
    let dx = (r - x).max(x - (w - r)).max(0.0);
    let dy = (r - y).max(y - (h - r)).max(0.0);
    dx * dx + dy * dy <= r * r
}

struct Card {
    image: Arc<CachedCard>,
    left: u32,
    top: u32,
    scale: f32,
    opacity: f32,
}

pub(crate) fn compose(pixels: &mut [u8], raw_w: u32, raw_h: u32) {
    if pixels.len() != raw_w as usize * raw_h as usize * 4 {
        return;
    }
    for card in cards(raw_w, raw_h) {
        card.image.composite(pixels, raw_w, raw_h, card.left, card.top, card.scale, card.opacity);
        crate::ds_perf::text_card_submission(false);
    }
}

// Permanent UI ownership repair: gameplay text cannot survive over native
// station/menu/dialog controls. Keep this gate shared by CPU and GPU cards;
// changing present paths must not revive a subtitle behind a workbench/shop.
fn problem_notice_card(raw_w:u32,raw_h:u32)->Option<Card>{
    let text=crate::ds_diagnostics::problem_notice()?;
    let (fw,fh)=(raw_h,raw_w);let unit=fh as f32/320.0;
    let width=((text.chars().count() as f32*8.5+18.0)*unit).ceil().min(fw as f32).max(1.0) as u32;
    let height=(22.0*unit).ceil().max(1.0) as u32;
    let mut h=host().lock().ok()?;
    let changed=h.problem_notice.as_ref().is_none_or(|(old,w,hh,_)|old!=&text||*w!=width||*hh!=height);
    if changed {
        let font=h.font.as_ref()?;let mut pixels=vec![0u8;width as usize*height as usize*4];
        for p in pixels.chunks_exact_mut(4){p.copy_from_slice(&[8,24,32,224]);}
        draw_text_centered(&mut pixels,width,height,font,7.8*unit,&text,5.0*unit,[220,248,255]);
        h.problem_notice=Some((text,width,height,Arc::new(CachedCard::new(width,height,0.0,&pixels))));
    }
    let image=Arc::clone(&h.problem_notice.as_ref()?.3);
    Some(Card{image,left:fw.saturating_sub(width)/2,top:(12.0*unit) as u32,scale:1.0,opacity:1.0})
}
fn cards(raw_w: u32, raw_h: u32) -> Vec<Card> {
    let _stutter_span=crate::ds_stutter_probe::Span::new(8,((raw_w as u64)<<32)|raw_h as u64);
    if raw_w==0 || raw_h==0{return Vec::new();}
    let notice=problem_notice_card(raw_w,raw_h);
    if !visible(){return notice.into_iter().collect();}
    let Ok(mut h) = host().lock() else { return Vec::new() };
    if h.font.is_none() {
        return Vec::new();
    }
    let mut cards = Vec::with_capacity(4);
    if let Some(card)=notice{cards.push(card);}
    let mut requests: Vec<(&str, String, String, Option<String>, f32)> = Vec::new();
    let chapter = crate::ds_chapter_overlay::host_snapshot();
    if chapter & (1 << 16) != 0 {
        let n = (chapter & 255) as usize;
        if (1..=12).contains(&n) {
            let id=format!("CHAPTER_{n:02}_GAME_TITLE");
            if let Some((_,text))=h.texts.iter().find(|(key,_)|key==&id) {
                let en=h.english.get(&id).cloned();
                requests.push(("chapter",id,text.clone(),en,if (chapter>>8)&255==3 {0.35}else{1.}));
            }
        }
    }
    if let Some(tutorial)=crate::ds_tutorial_overlay::host_snapshot() {
        let title=h.texts.iter().find(|(id,_)|id==&tutorial.title).map(|(_,t)|t.clone());
        let body=h.texts.iter().find(|(id,_)|id==&tutorial.body).map(|(_,t)|t.clone());
        if let (Some(title),Some(body))=(title,body) {
            let (body,_)=crate::ds_tutorial_text::content(&tutorial.body,&body);
            requests.push(("tutorial",tutorial.body,title,Some(body),1.));
        }
    }
    for (layer, value) in [
        ("objective", crate::ds_objective_overlay::host_snapshot()),
        ("subtitle", crate::ds_subtitle_overlay::host_snapshot()),
    ] {
        let index = (value & 0xffff) as usize;
        if index > 0 {
            if let Some((id, text)) = h.texts.get(index - 1) {
                requests.push((layer, id.clone(), text.clone(), None, 1.0));
            }
        }
    }
    let (fw, fh) = (raw_h, raw_w);
    let unit = fh as f32 / 320.0;
    for (layer, id, original, secondary, opacity) in requests {
        let mut style = h.style["layers"][layer].clone();
        let overrides = h.style["overrides"][&id].clone();
        if let (Some(dest), Some(src)) = (style.as_object_mut(), overrides["style"].as_object()) {
            for (k, v) in src {
                dest.insert(k.clone(), v.clone());
            }
        }
        let text = overrides["text"]
            .as_str()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(&original)
            .to_owned();
        let second = overrides["secondaryText"]
            .as_str()
            .filter(|s| !s.trim().is_empty())
            .map(str::to_owned)
            .or(secondary);
        let projection=matches!(layer,"chapter"|"objective") && style["projection"].as_str().is_some();
        let projection_mode=style["projection"].as_str().unwrap_or("scan").to_owned();
        let projection_background=style["projectionBackground"].as_bool().unwrap_or(true);
        let projection_animation=style["projectionAnimation"].as_bool().unwrap_or(true);
        let elapsed=if projection {
            let now=Instant::now();let entry=h.entries.entry(layer.to_owned()).or_insert((id.clone(),now,now));
            if entry.0!=id||now.duration_since(entry.2)>Duration::from_millis(200){*entry=(id.clone(),now,now);}
            entry.2=now;now.duration_since(entry.1).as_millis()
        }else{0};
        if projection {style["opacity"]=json!(0);style["strokeOpacity"]=json!(0);}
        let (mut text,marked)=crate::ds_text_presentation::clean_interference(&text);
        if layer=="subtitle" {
            let snapshot=crate::ds_subtitle_overlay::host_snapshot();
            let timing=crate::ds_subtitle_overlay::host_timing(snapshot);
            let (total,elapsed)=((timing>>32) as u32,timing as u32);
            let configured=overrides["style"]["longTextMode"].as_str().map(str::to_owned)
                .or_else(||std::env::var("TOUCHHLE_DS_SUBTITLE_MODE").ok())
                .unwrap_or_else(||style["longTextMode"].as_str().unwrap_or("three").to_owned());
            let paged=configured=="paged"&&total>0;
            let width=fw as f32*number(&style,"width",86.,10.,96.)/100.;
            let padding=number(&style,"padding",5.,0.,20.)*unit;
            let requested=number(&style,"fontSize",11.5,3.,36.)*unit/1.125*if paged{1.25}else{1.};
            let line_height=1.125*number(&style,"lineHeight",1.12,1.,2.);
            let height=(fh as f32*number(&style,"maxHeight",32.,10.,80.)/100.-2.*padding).max(1.);
            let plan_key=json!([text,width,padding,requested,line_height,height,paged]).to_string();
            if !h.caption_plans.contains_key(&plan_key){
                let font=h.font.as_ref().unwrap();
                let plan=crate::ds_caption_layout::plan(&text,(width-2.*padding).max(1.),requested,height,line_height,paged,|size,s|font.calculate_text_size(size,s,None).0);
                if h.caption_plans.len()>=16{h.caption_plans.clear();}
                h.caption_plans.insert(plan_key.clone(),plan);
            }
            let plan=&h.caption_plans[&plan_key];
            let page=crate::ds_caption_layout::page_at(&plan.pages,elapsed,total);
            let count=plan.pages.len();let size=plan.size;
            text=plan.pages[page].clone();style["fontSize"]=json!(size*1.125/unit);style["captionMinLines"]=json!(if paged{2}else{0});
            let event=(snapshot,page,paged);
            if h.caption_event!=Some(event){
                echo!("[DScaption-page] serial={} mode={} page={}/{} elapsed={} duration={}",snapshot>>32,if paged{"paged"}else{"three"},page+1,count,elapsed,total);
                h.caption_event=Some(event);
            }
        }
        if layer=="subtitle" && h.signal_id!=id {h.signal_id=id.clone();h.started=Instant::now();}
        let effect_phase=if marked && style["signalAnimation"].as_bool().unwrap_or(true) {crate::ds_text_presentation::signal_phase(h.started.elapsed().as_millis())}else{0};
        let key = json!([style, text, second, fw, fh,effect_phase]).to_string();
        if layer=="chapter" && !h.cache.contains_key(&key) {
            let mut cn=Vec::new();let mut en=Vec::new();
            for n in 1..=12 {
                let id=format!("CHAPTER_{n:02}_GAME_TITLE");
                let fallback=h.texts.iter().find(|(k,_)|k==&id).map(|(_,t)|t.as_str()).unwrap_or("");
                cn.push(h.style["overrides"][&id]["text"].as_str().unwrap_or(fallback));
                en.push(h.style["overrides"][&id]["secondaryText"].as_str().unwrap_or_else(||h.english.get(&id).map(|s|s.as_str()).unwrap_or("")));
            }
            let width=(fw as f32*number(&style,"width",70.,10.,96.)/100.) as u32;
            let height=(fh as f32*number(&style,"maxHeight",35.,10.,80.)/100.) as u32;
            let image=crate::ds_text_presentation::chapter((chapter&255) as usize,&text,second.as_deref().unwrap_or(""),&cn,&en,width,height);
            if h.cache.len()>24 || h.cache.values().map(|c|c.pixels.len()).sum::<usize>()+image.pixels.len()>64*1024*1024 {h.cache.clear();}
            h.cache.insert(key.clone(),Arc::new(CachedCard::new(image.width,image.height,image.first_center,&image.pixels)));
        }
        if !h.cache.contains_key(&key) {
            let width = (fw as f32 * number(&style, "width", 46.0, 10.0, 96.0) / 100.0)
                .round()
                .max(1.0) as u32;
            let pad = number(&style, "padding", 5.0, 0.0, 20.0) * unit;
            let requested_font_size = number(&style, "fontSize", 11.5, 3.0, 36.0);
            let mut size = requested_font_size * unit / 1.125;
            let max_font_size = number(
                &style,
                "maxFontSize",
                (requested_font_size * 2.5).min(24.0).max(requested_font_size),
                requested_font_size,
                36.0,
            );
            let sub_size = number(&style, "secondaryFontSize", 8.5, 3.0, 30.0) * unit / 1.125;
            let gap = number(&style, "gap", 3.0, 0.0, 15.0) * unit;
            let inner = (width as f32 - 2.0 * pad).max(1.0);
            let line_height = number(&style, "lineHeight", 1.12, 1.0, 2.0);
            let objective_label = style["label"].as_str().unwrap_or("目标：");
            let objective_label_size =
                number(&style, "labelFontSize", 18.0, 3.0, 18.0) * unit / 1.125;
            let objective_label_step = objective_label_size * 1.125 * line_height;
            let objective_label_gap = number(&style, "labelGap", 1.5, 0.0, 8.0) * unit;
            let objective_label_h = if objective_label.trim().is_empty() {
                0.0
            } else {
                objective_label_step + objective_label_gap
            };
            let objective_height = if layer == "objective" {
                (fh as f32 * number(&style, "maxHeight", 35.0, 10.0, 80.0) / 100.0)
                    .round()
                    .max(1.0)
            } else {
                0.0
            };
            let first_lines = if layer=="subtitle" {
                crate::ds_caption_layout::wrap(&text,inner,|s|h.font.as_ref().unwrap().calculate_text_size(size,s,None).0)
            } else if layer=="objective" {
                let (fitted, lines) = objective_layout(
                    h.font.as_ref().unwrap(),
                    size,
                    &text,
                    inner,
                    (objective_height - 2.0 * pad - objective_label_h).max(1.0),
                    line_height,
                    max_font_size * unit / 1.125,
                    OBJECTIVE_SIZE_INSET_PT * unit / 1.125,
                );
                size = fitted;
                lines
            } else {
                lines(if layer == "tutorial" { &h.title_font } else { h.font.as_ref().unwrap() }, size, &text, inner)
            };
            let second_lines = second.as_ref().map(|s| lines(h.font.as_ref().unwrap(), sub_size, s, inner)).unwrap_or_default();
            let first_step = size * 1.125 * line_height;
            let second_step = sub_size * 1.125 * line_height;
            let first_h = first_lines.len().max(style["captionMinLines"].as_u64().unwrap_or(0) as usize) as f32 * first_step;
            let font = h.font.as_ref().unwrap();
            let second_h = second_lines.len() as f32 * second_step;
            let icons=if layer=="tutorial" {crate::ds_tutorial_text::content(&id,"").1}else{Vec::new()};
            let icon_h=if icons.is_empty(){0.}else{24.*unit};
            let mut height = (icon_h + 2.0 * pad + first_h + if second_h > 0.0 { gap + second_h } else { 0.0 })
                .ceil()
                .max(1.0) as u32;
            if layer == "objective" {
                // Objective and chapter share the same total card area even
                // when the objective text is only one or two lines.
                height = objective_height as u32;
            }
            if width as u64 * height as u64 > 8_000_000 {
                continue;
            }
            let mut bitmap = vec![0u8; width as usize * height as usize * 4];
            let bg = color(&style, "background", "#031016");
            let bg_a = number(&style, "opacity", 0.76, 0.0, 1.0);
            let radius = number(&style, "radius", 2.0, 0.0, 20.0) * unit;
            let border = (unit * 0.35).max(1.0);
            let stroke = color(&style, "stroke", "#56dce8");
            let stroke_a = number(&style, "strokeOpacity", 0.6, 0.0, 1.0);
            for y in 0..height {
                for x in 0..width {
                    let px = x as f32 + 0.5;
                    let py = y as f32 + 0.5;
                    if !rounded_inside(px, py, width as f32, height as f32, radius) { continue; }
                    let edge = !rounded_inside(px-border, py-border, width as f32-2.0*border, height as f32-2.0*border, (radius-border).max(0.0));
                    let a = if edge { stroke_a + bg_a * (1.0-stroke_a) } else { bg_a };
                    let at = ((y * width + x) * 4) as usize;
                    for c in 0..3 {
                        bitmap[at+c] = if edge && a > 0.0 { ((stroke[c] as f32 * stroke_a + bg[c] as f32 * bg_a * (1.0-stroke_a))/a) as u8 } else { bg[c] };
                    }
                    bitmap[at+3] = (a*255.0) as u8;
                }
            }
            let background=if marked {let b=bitmap.clone();bitmap.fill(0);Some(b)}else{None};
            let align = match style["align"].as_str() {
                Some("left") => TextAlignment::Left,
                Some("right") => TextAlignment::Right,
                _ => TextAlignment::Center,
            };
            let origin_x = match align {
                TextAlignment::Left => pad,
                TextAlignment::Right => width as f32 - pad,
                _ => width as f32 / 2.0,
            };
            let mut draw = |face: &Font, s: &str, size: f32, y: f32, rgb: [u8; 3]| {
                face.draw(
                    size,
                    s,
                    (origin_x, y),
                    Some((inner, WrapMode::Char)),
                    match style["align"].as_str() { Some("left")=>TextAlignment::Left,Some("right")=>TextAlignment::Right,_=>TextAlignment::Center },
                    |glyph| {
                        let (gx, gy) = glyph.origin();
                        let (gw, gh) = glyph.dimensions();
                        for py in 0..gh {
                            for px in 0..gw {
                                let x = gx.round() as i32 + px;
                                let y = gy.round() as i32 + py;
                                if x < 0 || y < 0 || x >= width as i32 || y >= height as i32 {
                                    continue;
                                }
                                let at = (y as usize * width as usize + x as usize) * 4;
                                let a = glyph.pixel_at((px, py));
                                if a > 0.0 {
                                    let old_a = bitmap[at + 3] as f32 / 255.0;
                                    let out = a + old_a * (1.0 - a);
                                    for c in 0..3 {
                                        bitmap[at + c] = ((rgb[c] as f32 * a
                                            + bitmap[at + c] as f32 * old_a * (1.0 - a))
                                            / out.max(0.001))
                                            as u8;
                                    }
                                    bitmap[at + 3] = (out * 255.0) as u8;
                                }
                            }
                        }
                    },
                );
            };
            let first_top = if layer == "objective" {
                let group_h = objective_label_h + first_lines.len() as f32 * first_step;
                let label_top = ((height as f32 - group_h) * 0.5).max(pad);
                label_top + objective_label_h
            } else {
                pad + (first_h - first_lines.len() as f32 * first_step) * 0.5
            };
            if layer == "objective" && objective_label_h > 0.0 {
                let group_h = objective_label_h + first_lines.len() as f32 * first_step;
                let label_top = ((height as f32 - group_h) * 0.5).max(pad);
                draw(
                    &h.title_font,
                    objective_label,
                    objective_label_size,
                    label_top,
                    color(&style, "labelColor", "#56dce8"),
                );
            }
            for (index, line) in first_lines.iter().enumerate() {
                draw(if layer == "tutorial" { &h.title_font } else { font }, &crate::ds_text_presentation::signal_text(line,effect_phase), size, first_top + index as f32 * first_step, color(&style, "color", "#ebf4f7"));
            }
            for (index, line) in second_lines.iter().enumerate() {
                draw(font, line, sub_size, pad + first_h + gap + index as f32 * second_step, color(&style, "secondaryColor", "#56dce8"));
            }
            drop(draw);
            if !icons.is_empty() {
                draw_keycaps(&mut bitmap,width,height,&id,&icons,font,unit,height as f32-icon_h);
            }
            if marked {crate::ds_text_presentation::interference(&mut bitmap,width,height,effect_phase);}
            if let Some(mut background)=background {
                for y in 0..height {for x in 0..width {let at=(y as usize*width as usize+x as usize)*4;
                    crate::ds_text_presentation::blend(&mut background,width,height,x as i32,y as i32,[bitmap[at],bitmap[at+1],bitmap[at+2]],bitmap[at+3] as f32/255.);
                }}
                bitmap=background;
            }
            if h.cache.len() > 24 || h.cache.values().map(|c|c.pixels.len()).sum::<usize>()+bitmap.len()>64*1024*1024 {
                h.cache.clear();
            }
            h.cache.insert(key.clone(), Arc::new(CachedCard::new(width, height, pad + first_h / 2.0, &bitmap)));
        }
        let card = Arc::clone(&h.cache[&key]);
        let (bw, bh) = (card.width, card.height);
        let scale =
            (fh as f32 * number(&style, "maxHeight", 35.0, 10.0, 80.0) / 100.0 / bh as f32)
                .min(1.0);
        let dw = (bw as f32 * scale).max(1.0) as u32;
        let dh = (bh as f32 * scale).max(1.0) as u32;
        let left = (fw as f32 * number(&style, "x", 50.0, 0.0, 100.0) / 100.0 - dw as f32 / 2.0)
            .clamp(0.0, fw.saturating_sub(dw) as f32) as u32;
        let anchor = fh as f32 * number(&style, "y", 50.0, 0.0, 100.0) / 100.0;
        let offset = match style["anchor"].as_str() {
            Some("bottom-center") => dh as f32,
            Some("first-center") => card.first_center * scale,
            _ => dh as f32 / 2.0,
        };
        let top = (anchor - offset).clamp(0.0, fh.saturating_sub(dh) as f32) as u32;
        if projection && projection_background {
            let phase=crate::ds_panel_motion::panel_phase(elapsed,projection_animation);
            let panel_key=(bw,bh,projection_mode.clone(),phase);
            if !h.panels.contains_key(&panel_key) {
                // Entry textures stop being useful once the panel is stable.
                if phase==6 {h.panels.retain(|k,_|k.3==6);}
                if h.panels.len()>20 || h.panels.values().map(|c|c.pixels.len()).sum::<usize>()+(bw as usize*bh as usize*4)>32*1024*1024 {h.panels.clear();}
                let pixels=crate::ds_panel_motion::raster(bw,bh,&projection_mode,phase);
                h.panels.insert(panel_key.clone(),Arc::new(CachedCard::new(bw,bh,0.,&pixels)));
            }
            cards.push(Card{image:Arc::clone(&h.panels[&panel_key]),left,top,scale,opacity});
        }
        let opacity=opacity*if projection{crate::ds_panel_motion::text_opacity(elapsed,projection_animation)}else{1.};
        cards.push(Card { image: card, left, top, scale, opacity });
    }
    if let Some(prompt) = crate::ds_qte_overlay::snapshot() {
        cards.extend(qte_cards(&mut h, prompt, raw_w, raw_h));
    }
    cards
}

fn draw_text_centered(
    bitmap: &mut [u8],
    width: u32,
    height: u32,
    font: &Font,
    size: f32,
    text: &str,
    top: f32,
    rgb: [u8; 3],
) {
    font.draw(
        size,
        text,
        (width as f32 * 0.5, top),
        None,
        TextAlignment::Center,
        |glyph| {
            let (gx, gy) = glyph.origin();
            let (gw, gh) = glyph.dimensions();
            for py in 0..gh {
                for px in 0..gw {
                    let x = gx.round() as i32 + px;
                    let y = gy.round() as i32 + py;
                    if x < 0 || y < 0 || x >= width as i32 || y >= height as i32 {
                        continue;
                    }
                    let at = (y as usize * width as usize + x as usize) * 4;
                    let a = glyph.pixel_at((px, py));
                    if a <= 0.0 {
                        continue;
                    }
                    let old_a = bitmap[at + 3] as f32 / 255.0;
                    let out = a + old_a * (1.0 - a);
                    for c in 0..3 {
                        bitmap[at + c] = ((rgb[c] as f32 * a
                            + bitmap[at + c] as f32 * old_a * (1.0 - a))
                            / out.max(0.001)) as u8;
                    }
                    bitmap[at + 3] = (out * 255.0) as u8;
                }
            }
        },
    );
}

fn qte_cards(
    h: &mut Host,
    prompt: crate::ds_qte_overlay::Prompt,
    raw_w: u32,
    raw_h: u32,
) -> Vec<Card> {
    use crate::ds_qte_overlay::PromptKind;

    if prompt.kind == PromptKind::Death {
        if let Some([x, y, width, height]) = prompt.secondary {
            let cancel = crate::ds_qte_overlay::Prompt {
                x,
                y,
                width,
                height,
                kind: PromptKind::DeathCancel,
                secondary: None,
            };
            return [
                qte_card(h, prompt, raw_w, raw_h),
                qte_card(h, cancel, raw_w, raw_h),
            ]
            .into_iter()
            .flatten()
            .collect();
        }
    }
    qte_card(h, prompt, raw_w, raw_h).into_iter().collect()
}

fn qte_card(
    h: &mut Host,
    prompt: crate::ds_qte_overlay::Prompt,
    raw_w: u32,
    raw_h: u32,
) -> Option<Card> {
    use crate::ds_qte_overlay::{CombatAction, PromptKind};

    let fw = raw_h as f32;
    let fh = raw_w as f32;
    let unit = fh / 320.0;
    let combat = matches!(prompt.kind, PromptKind::Combat(_));
    let labeled = combat
        || matches!(prompt.kind, PromptKind::Death | PromptKind::DeathCancel);
    let (bw, bh) = if labeled {
        (
            (66.0 * unit).min(fw).round() as u32,
            (44.0 * unit).min(fh).round() as u32,
        )
    } else {
        // Native texture bounds include transparent artwork margins and the
        // rotated arrow's full canvas. They locate the hint, not the E/A plate.
        // Keep the existing minimum keycap size instead of covering gameplay
        // with a plate as large as that padded native texture.
        (
            (52.0 * unit).min(fw).round() as u32,
            (32.0 * unit).min(fh).round() as u32,
        )
    };
    if bw < 8 || bh < 8 {
        return None;
    }
    let kind_tag = match prompt.kind {
        PromptKind::Tap => "tap",
        PromptKind::Swipe => "swipe",
        PromptKind::Combat(CombatAction::Slash) => "slash",
        PromptKind::Combat(CombatAction::Stomp) => "stomp",
        PromptKind::Death => "death",
        PromptKind::DeathCancel => "death-cancel",
    };
    let key = format!("qte:{kind_tag}:{bw}:{bh}");
    if !h.cache.contains_key(&key) {
        let mut bitmap = vec![0u8; bw as usize * bh as usize * 4];
        let accent = if matches!(prompt.kind, PromptKind::Death | PromptKind::DeathCancel) {
            [235, 55, 55]
        } else {
            [86, 220, 232]
        };
        let radius = (5.0 * unit).min(bh as f32 * 0.5);
        let border = (unit * 0.55).max(1.0);
        for y in 0..bh {
            for x in 0..bw {
                let px = x as f32 + 0.5;
                let py = y as f32 + 0.5;
                if !rounded_inside(px, py, bw as f32, bh as f32, radius) {
                    continue;
                }
                let edge = !rounded_inside(
                    px - border,
                    py - border,
                    bw as f32 - 2.0 * border,
                    bh as f32 - 2.0 * border,
                    (radius - border).max(0.0),
                );
                let at = ((y * bw + x) * 4) as usize;
                let rgb = if edge { accent } else { [4, 17, 23] };
                bitmap[at..at + 3].copy_from_slice(&rgb);
                bitmap[at + 3] = if edge { 245 } else { 235 };
            }
        }
        let labels = if prompt.kind == PromptKind::DeathCancel {
            vec!["Q".to_owned(), "B".to_owned()]
        } else {
            vec!["E".to_owned(), "A".to_owned()]
        };
        draw_keycaps(
            &mut bitmap,
            bw,
            bh,
            "TUTORIAL_MELEE_TEXT",
            &labels,
            h.font.as_ref()?,
            unit,
            if labeled {
                4.0 * unit
            } else {
                (bh as f32 - 18.0 * unit) * 0.5
            },
        );
        let label = match prompt.kind {
            PromptKind::Combat(CombatAction::Slash) => Some("切割"),
            PromptKind::Combat(CombatAction::Stomp) => Some("踩踏"),
            PromptKind::Death => Some("继续"),
            PromptKind::DeathCancel => Some("返回"),
            _ => None,
        };
        if let Some(label) = label {
            draw_text_centered(
                &mut bitmap,
                bw,
                bh,
                h.font.as_ref()?,
                8.0 * unit / 1.125,
                label,
                bh as f32 - 14.0 * unit,
                [235, 244, 247],
            );
        }
        if h.cache.len() > 32
            || h.cache.values().map(|c| c.pixels.len()).sum::<usize>() + bitmap.len()
                > 64 * 1024 * 1024
        {
            h.cache.clear();
        }
        h.cache.insert(key.clone(), Arc::new(CachedCard::new(bw, bh, 0.0, &bitmap)));
    }
    let card = Arc::clone(&h.cache[&key]);
    if combat {
        return Some(Card {
            image: card,
            left: (4.0 * unit).round() as u32,
            top: (4.0 * unit).round() as u32,
            scale: 1.0,
            opacity: 0.96,
        });
    }
    let frame = crate::window::DS_FRAME_COUNTER.load(std::sync::atomic::Ordering::Relaxed);
    let pressed = (frame / 10) % 4 == 2;
    let scale = if pressed { 0.92 } else { 1.0 };
    let opacity = if pressed { 1.0 } else { 0.78 };
    let press_offset = if pressed { 2.0 * unit } else { 0.0 };
    let dw = bw as f32 * scale;
    let dh = bh as f32 * scale;
    let left = (prompt.x / 480.0 * fw - dw * 0.5)
        .clamp(0.0, (fw - dw).max(0.0)) as u32;
    let top = (prompt.y / 320.0 * fh - dh * 0.5 + press_offset)
        .clamp(0.0, (fh - dh).max(0.0)) as u32;
    Some(Card {
        image: card,
        left,
        top,
        scale,
        opacity,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        balanced_split, cpu_composition_policy, objective_layout, OBJECTIVE_SIZE_INSET_PT,
    };

    #[test]
    fn cpu_text_overrides_fast_present_until_it_disappears() {
        assert!(!cpu_composition_policy(true, true, false));
        assert!(cpu_composition_policy(true, true, true));
        assert!(!cpu_composition_policy(true, true, false));
        assert!(cpu_composition_policy(true, false, false));
        assert!(cpu_composition_policy(true, false, true));
        // A requested host caption still needs CPU fallback without an
        // explicit --force-composition flag or when fast-present is selected.
        assert!(cpu_composition_policy(false, true, true));
        assert!(cpu_composition_policy(false, false, true));
        assert!(!cpu_composition_policy(false, false, false));
    }

    #[test]
    fn objective_break_prefers_a_semantic_boundary() {
        let chars: Vec<char> = "找到并摧毁3个排水保护装置的电源箱".chars().collect();
        let index = balanced_split(&chars, 120.0, |value| value.chars().count() as f32 * 10.0)
            .unwrap();
        assert_eq!(chars[..index].iter().collect::<String>(), "找到并摧毁3个");
    }

    #[test]
    fn every_objective_scales_and_fits_within_two_lines() {
        let font = crate::font::Font::from_vec(
            crate::ds_font_roles::STORY.to_vec(),
        )
        .unwrap();
        let entries: serde_json::Value =
            serde_json::from_str(include_str!("ds_overlay_texts.json")).unwrap();
        let unit = 1080.0 / 320.0;
        let width = 1920.0 * 20.4 / 100.0 - 2.0 * 2.5 * unit;
        let label_size = 6.0 * unit / 1.125;
        let label_height = label_size * 1.125 * 1.12 + 1.5 * unit;
        let height = 1080.0 * 17.0 / 100.0 - 2.0 * 2.5 * unit - label_height;
        let requested = 7.2 * unit / 1.125;
        let max = 18.0 * unit / 1.125;
        let mut objectives = 0;
        for entry in entries["entries"].as_array().unwrap() {
            let id = entry["id"].as_str().unwrap();
            if !id.starts_with("OBJ_") {
                continue;
            }
            let text = entry["text"].as_str().unwrap();
            let (size, lines) = objective_layout(
                &font,
                requested,
                text,
                width,
                height,
                1.12,
                max,
                OBJECTIVE_SIZE_INSET_PT * unit / 1.125,
            );
            assert!(size > requested, "{id} did not scale up");
            assert!(
                size <= max - OBJECTIVE_SIZE_INSET_PT * unit / 1.125 + 0.01,
                "{id} exceeded the requested size inset"
            );
            assert!((1..=2).contains(&lines.len()), "{id} produced too many lines");
            for line in &lines {
                assert!(
                    font.calculate_text_size(size, line, None).0 <= width,
                    "{id} overflowed horizontally"
                );
            }
            assert!(
                lines.len() as f32 * size * 1.125 * 1.12 <= height,
                "{id} overflowed vertically"
            );
            objectives += 1;
        }
        assert_eq!(objectives, 85);
    }
}

// Reuse the accepted Kenney input textures. Draw a labeled fallback only for
// unusual user bindings which have no supplied sprite; never redraw defaults.
fn draw_keycaps(bitmap:&mut [u8],w:u32,h:u32,id:&str,labels:&[String],font:&Font,unit:f32,top:f32) {
    let gap=4.*unit;let size=8.*unit/1.125;let cap_h=18.*unit;
    let sprites:Vec<_>=labels.iter().enumerate().map(|(i,s)|crate::ds_tutorial_prompts::sprite(crate::ds_tutorial_text::icon_kind(id,i),s)).collect();
    let widths:Vec<f32>=labels.iter().zip(&sprites).map(|(s,r)|r.map(|r|cap_h*r[2]as f32/r[3]as f32).unwrap_or_else(||(font.calculate_text_size(size,s,None).0+10.*unit).max(cap_h))).collect();
    let total=widths.iter().sum::<f32>()+gap*(labels.len().saturating_sub(1) as f32);
    let scale=((w as f32-8.*unit)/total).min(1.);let mut left=(w as f32-total*scale)*0.5;
    for (index,(label,width)) in labels.iter().zip(widths).enumerate() {
        let kind=crate::ds_tutorial_text::icon_kind(id,index);
        let width=width*scale;let height=cap_h*scale;let round=if kind=="pad" || kind=="stick" {height*0.5}else{3.*unit*scale};
        if let Some(sprite)=sprites[index] {
            crate::ds_tutorial_prompts::draw(bitmap,w,h,sprite,left,top,height);
            left+=width+gap*scale;continue;
        }
        for y in 0..height.ceil() as u32 {for x in 0..width.ceil() as u32 {
            if rounded_inside(x as f32,y as f32,width,height,round) {
                let edge=!rounded_inside(x as f32-1.,y as f32-1.,width-2.,height-2.,(round-1.).max(0.));
                let stroke=if kind=="pad" {[158,208,249]}else{[210,220,226]};
                crate::ds_text_presentation::blend(bitmap,w,h,(left+x as f32) as i32,(top+y as f32) as i32,if edge {stroke}else{[16,30,38]},0.95);
            }
        }}
        font.draw(size*scale,label,(left+width*0.5,top+3.*unit*scale),None,TextAlignment::Center,|glyph| {
            let(gx,gy)=glyph.origin();let(gw,gh)=glyph.dimensions();
            for y in 0..gh {for x in 0..gw {crate::ds_text_presentation::blend(bitmap,w,h,gx.round() as i32+x,gy.round() as i32+y,[255,255,255],glyph.pixel_at((x,y)));}}
        });left+=width+gap*scale;
    }
}
