//! Opt-in scene activity for long full-chapter diagnostic sessions. No pixel,
//! path, key, GL query, guest getter or synchronous IO collection. Counts are
//! submissions/code fetches; scope wall times are inclusive and may overlap.
use std::sync::{OnceLock, atomic::{AtomicU64, Ordering::Relaxed}};
use std::time::Instant;
use serde_json::{json, Value};

#[derive(Copy, Clone)]
pub(crate) enum Kind { FileRead, IpaMiss, IpaHit, PvrtcDecode, HdDecode, HdCacheHit,
    MipStep, TexImage, TexCompressed, TexSubImage, BufferUpload, DrawArrays,
    DrawElements, ShaderCompile, ShaderLink, JitCodeRead, ReadPixels, Finish, ImageDecode, RenderKey, RenderKeyOverflow, Flush }
const N:usize=Kind::Flush as usize+1;
const NAMES:[&str;N]=["guest_file_read", "ipa_cache_miss_decode", "ipa_cache_hit", "pvrtc_decode", "hd_lookup_decode", "hd_cache_hit",
    "rgba_mip_step", "guest_tex_image_pixels", "guest_compressed_tex_bytes", "guest_tex_subimage_pixels", "guest_buffer_payload_bytes", "guest_draw_arrays_vertices",
    "guest_draw_elements_indices", "explicit_shader_compile", "explicit_shader_link", "jit_code_read_bytes", "host_readback_pixels", "host_gl_finish", "image_decode_input_bytes", "partial_render_key_draws", "partial_render_key_capacity_overflow", "host_gl_flush"];
struct Counter {calls:AtomicU64,units:AtomicU64,wall:AtomicU64,max:AtomicU64,last_tag:AtomicU64}
impl Counter {
    const fn new()->Self {Self{calls:AtomicU64::new(0),units:AtomicU64::new(0),wall:AtomicU64::new(0),max:AtomicU64::new(0),last_tag:AtomicU64::new(0)}}
    fn push(&self,units:u64,us:u64,tag:u64) {self.calls.fetch_add(1,Relaxed);self.units.fetch_add(units,Relaxed);self.wall.fetch_add(us,Relaxed);self.max.fetch_max(us,Relaxed);self.last_tag.store(tag,Relaxed);}
    fn take(&self)->Value {json!({"calls":self.calls.swap(0,Relaxed),"units":self.units.swap(0,Relaxed),"inclusive_wall_us":self.wall.swap(0,Relaxed),"max_wall_us":self.max.swap(0,Relaxed),"last_tag":self.last_tag.load(Relaxed)})}
}
static COUNTERS:[Counter;N]=[const{Counter::new()};N];
pub(crate) fn requested()->bool {static ON:OnceLock<bool>=OnceLock::new();*ON.get_or_init(||std::env::var("TOUCHHLE_DS_SCENE_TRACE").as_deref()==Ok("1"))}
pub(crate) fn enabled()->bool {requested() && crate::ds_diagnostics::detail_enabled()}
pub(crate) fn count(kind:Kind,units:u64,tag:u64) {
    if !enabled(){return;}let cost=Instant::now();COUNTERS[kind as usize].push(units,0,tag);crate::ds_diagnostics::collector_cost(cost);
}
pub(crate) struct Scope {start:Option<Instant>,kind:Kind,units:u64,tag:u64}
impl Scope {
    pub(crate) fn new(kind:Kind,units:u64,tag:u64)->Self {let start=enabled().then(Instant::now);if let Some(cost)=start {crate::ds_diagnostics::collector_cost(cost);}Self{start,kind,units,tag}}
    /// Frequent draw calls use counts in enhanced mode, wall spans in super mode.
    pub(crate) fn draw(kind:Kind,units:u64,tag:u64)->Self {
        if crate::ds_stutter_probe::enabled(){Self::new(kind,units,tag)}
        else{count(kind,units,tag);Self{start:None,kind,units,tag}}
    }
    pub(crate) fn units(&mut self,units:u64){self.units=units;}
}
impl Drop for Scope {
    fn drop(&mut self) {
        let Some(start)=self.start else{return;};let cost=Instant::now();
        let us=cost.duration_since(start).as_micros().min(u64::MAX as u128) as u64;
        COUNTERS[self.kind as usize].push(self.units,us,self.tag);
        crate::ds_diagnostics::collector_cost(cost);
        if us>=2000 && crate::ds_diagnostics::detail_enabled(){crate::ds_diagnostics::event("scene_slow_scope",[self.kind as u64,us,crate::window::DS_FRAME_COUNTER.load(Relaxed),self.tag]);}
    }
}
pub(crate) fn snapshot()->Value {
    if !requested(){return Value::Null;}
    Value::Object(NAMES.into_iter().enumerate().map(|(i,n)|(n.to_owned(),COUNTERS[i].take())).collect())
}
pub(crate) fn contract()->Value {json!({"schema":1,"requested":requested(),"kinds":NAMES,
    "scope":"CPU inclusive call wall time and submission/code-read counts, not GPU completion or exclusive function time; nested intervals overlap; atomic interval boundaries approximate",
    "jit":"code fetch callbacks while translating, including rereads; not executed instruction count, compiled-block count or measured compiler time; tag is last guest code address",
    "texture":"guest upload requests/areas or payload bytes, not total VRAM; HD actual-submission observer separate; no additional GL query",
    "tags":"IPA archive member index; HD content key; texture format; buffer target; draw primitive; explicit shader/program object ID; guest file class1=host,2=IPA-memory,3=resource,4=embedded,0=other",
    "readback":"host ReadPixels CPU wall time/area, never initiated by this collector",
    "render_key":"partial per-context fixed-function setter state only; no texture IDs/content, matrices, light/material values, full combiner or direct GL helper state. First-use key is not driver compilation proof. 4096 retained keys/context, overflow draw count disclosed",
    "render_key_fields":["caps/depth/blend/alpha/fog/lighting/cull/scissor/normalize/rescale/palette/color-material/stencil and texture2D units0,1","blend factors","alpha function/reference bits","depth function","shade model","texture env mode0","texture env mode1","explicit program object","client arrays","vertex size/type","draw primitive","index type"],
    "units":"file/image/PVRTC/compressed texture/buffer/HDcacheclone: bytes; teximage/subimage/mip: pixels; HDlookup: attempts including absent file; draw: submitted vertices/indices; JIT: 4-byte code fetches; compile/link/finish/flush: calls; IPAhit: calls with0units; null upload payload0, allocation/driver acceptance not inferred",
    "self_cost":"counter, scope setup/drop and event enqueue tracked; operation wall time is not charged as collector overhead; clock cost imperfect, real A/B still required"})}
#[cfg(test)] mod tests {
    use super::*;
    #[test] fn counters_preserve_large_stalls_and_payloads_and_reset_windows(){let c=Counter::new();c.push(5,750000,42);c.push(7,9,17);let s=c.take();assert_eq!(s["calls"],2);assert_eq!(s["units"],12);assert_eq!(s["max_wall_us"],750000);assert_eq!(s["inclusive_wall_us"],750009);assert_eq!(s["last_tag"],17);assert_eq!(c.take()["calls"],0);}
    #[test] fn kinds_and_units_have_stable_one_to_one_schema(){assert_eq!(NAMES.len(),N);assert_eq!(NAMES[Kind::JitCodeRead as usize],"jit_code_read_bytes");let mut names=std::collections::HashSet::new();for name in NAMES{assert!(names.insert(name));}}
}

/// Per-context, explicitly partial fixed-function state tracker. Values come
/// from backend setter arguments; no driver query, texture image or vertex
/// buffer is read. This is evidence for first-use correlation, NOT a complete
/// shader key or a claim that the driver compiled a shader.
pub(crate) struct RenderState {values:[u64;10],active:usize,seen:std::collections::HashSet<u64>}
impl Default for RenderState {fn default()->Self {Self{values:[0,1u64<<32,0x207u64<<32,0x201,0x1d01,0x2100,0x2100,0,0,(4u64<<32)|0x1406],active:0,seen:std::collections::HashSet::new()}}}
impl RenderState {
    pub(crate) fn set(&mut self,slot:usize,value:u64){if requested(){self.values[slot]=value;}}
    pub(crate) fn active(&mut self,texture:u32){if requested(){self.active=texture.saturating_sub(0x84c0) as usize;}}
    pub(crate) fn env(&mut self,target:u32,pname:u32,param:u32){if target==0x2300 && pname==0x2200 && self.active<2 {self.set(5+self.active,param as u64);}}
    pub(crate) fn cap(&mut self,cap:u32,on:bool){
        if !requested(){return;}
        let index=if cap==0xde1 && self.active<2 {Some(12+self.active)}else{[0xb71,0xbe2,0xbc0,0xb60,0xb50,0xb44,0xc11,0xba1,0x803a,0x8840,0xb57,0xb90].iter().position(|v|*v==cap)};
        if let Some(index)=index{if on{self.values[0]|=1u64<<index;}else{self.values[0]&=!(1u64<<index);}}
    }
    pub(crate) fn client(&mut self,array:u32,on:bool){if !requested(){return;}if let Some(i)=[0x8074,0x8075,0x8076,0x8078,0x8844,0x86ad].iter().position(|v|*v==array){if on{self.values[8]|=1u64<<i;}else{self.values[8]&=!(1u64<<i);}}}
    fn key(&self,mode:u32,index_type:u32)->u64 {let mut h=0xcbf29ce484222325u64;for v in self.values.into_iter().chain([mode as u64,index_type as u64]){for byte in v.to_le_bytes(){h=(h^byte as u64).wrapping_mul(0x100000001b3);}}h}
    pub(crate) fn draw(&mut self,mode:u32,index_type:u32){
        if !enabled(){return;}let cost=Instant::now();let key=self.key(mode,index_type);
        COUNTERS[Kind::RenderKey as usize].push(0,0,key);
        let mut first=false;
        if !self.seen.contains(&key) {
            if self.seen.len()<4096 {
                self.seen.insert(key);first=true;
            }else{COUNTERS[Kind::RenderKeyOverflow as usize].push(1,0,key);}
        }
        crate::ds_diagnostics::collector_cost(cost);
        if first {let v=self.values;
                crate::ds_diagnostics::event("render_state_key_0",[key,v[0],v[1],v[2]]);
                crate::ds_diagnostics::event("render_state_key_1",[key,v[3],v[4],v[5]]);
                crate::ds_diagnostics::event("render_state_key_2",[key,v[6],v[7],v[8]]);
                crate::ds_diagnostics::event("render_state_key_3",[key,v[9],mode as u64,index_type as u64]);
        }
    }
}
#[cfg(test)] mod render_tests {
    use super::*;
    #[test] fn keys_are_deterministic_and_ignore_object_identity_and_draw_count(){let a=RenderState::default();let mut b=RenderState::default();assert_eq!(a.key(4,0x1403),b.key(4,0x1403));b.values[0]=1;assert_ne!(a.key(4,0x1403),b.key(4,0x1403));assert_ne!(a.key(4,0x1403),a.key(4,0x1401));}
}

pub(crate) fn hle_binding(address:u32,svc:u32,symbol:&str){
    if !enabled(){return;}let cost=Instant::now();
    let text=format!("{address:08x}:{svc:08x}:{symbol}");
    crate::ds_diagnostics::collector_cost(cost);
    crate::ds_diagnostics::feature("hle_binding",&text);
}
