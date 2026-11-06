import{names,limits,gameRect,resolve,validate,card}from'./layout.mjs';
const $=id=>document.getElementById(id),canvas=$('canvas'),ctx=canvas.getContext('2d');
const defaults=await fetch('default-style.json').then(r=>r.json()),samples=await fetch('samples.json').then(r=>r.json());
let config=structuredClone(defaults),layer='chapter',scope='global',selected={chapter:'CHAPTER_01_GAME_TITLE',objective:'OBJ_GOV_02',subtitle:'Anchor_01'},image=new Image(),history=[],future=[],drag=null,bounds={},draft={},ready=false;
const textMap=new Map(samples.map(e=>[e.id,e.text]));
const storage='deadspace-overlay-studio-v1';
try{let saved=JSON.parse(localStorage.getItem(storage));if(saved)config=validate(saved);}catch{}
function notice(text,error=false){$('notice').textContent=text;$('notice').classList.toggle('error',error);}
function persist(){localStorage.setItem(storage,JSON.stringify(config));$('saveState').textContent='已保存到此浏览器 · '+new Date().toLocaleTimeString();$('overrideCount').textContent=Object.keys(config.overrides||{}).length;}
function checkpoint(){history.push(JSON.stringify(config));if(history.length>80)history.shift();future=[];}
function undo(reverse=false){const from=reverse?future:history,to=reverse?history:future;if(!from.length)return;to.push(JSON.stringify(config));config=JSON.parse(from.pop());draft={};sync();persist();}
function id(){return selected[layer];}
function labelText(name=layer){const key=selected[name],over=config.overrides?.[key];
 let first=textMap.get(key)||defaults.samples[name],second='';
 if(name==='chapter'){const n=Number(key.slice(8,10));first=`第 ${n} 章 · ${first}`;second=chapterEnglish[n-1]||defaults.samples.chapterSecondary;}
 return [draft[key]?.[0]??over?.text??first,draft[key]?.[1]??over?.secondaryText??second];
}
const chapterEnglish=['Honorable Intentions','Exit Wounds','Willing Accomplice','Identity Crisis','Leaving Traces','Lumbering Beast','Bounds and Leaps','Encroachment','Trading Up','Reversal','Acute Psychosis',"Yesterday's Problem"].map((t,i)=>`CHAPTER ${String(i+1).padStart(2,'0')} · ${t}`);
function targetStyle(){if(scope==='global')return config.layers[layer];config.overrides??={};config.overrides[id()]??={};return config.overrides[id()].style??={};}
function change(key,value){checkpoint();targetStyle()[key]=value;persist();sync(false);}
function filtered(){const q=$('entrySearch').value.toLowerCase();return samples.filter(e=>{
 const match=layer==='chapter'?/^CHAPTER_\d{2}_GAME_TITLE$/.test(e.id):layer==='objective'?e.id.startsWith('OBJ_'):!e.id.startsWith('OBJ_')&&!e.id.startsWith('CHAPTER_');
 return match&&(!q||`${e.id} ${e.text}`.toLowerCase().includes(q));});}
function entryOptions(){const list=filtered();$('entryId').replaceChildren();
 for(const e of list){const o=new Option(`${config.overrides?.[e.id]?'● ':''}${e.id} · ${e.text.slice(0,25)}`,e.id);$('entryId').append(o);}
 $('entryId').value=id();if(!$('entryId').value&&list.length){selected[layer]=list[0].id;$('entryId').value=id();}
}
const labels={x:'水平锚点 %',y:'垂直锚点 %',width:'文字区域宽度 %',fontSize:'字号 · 参考像素',secondaryFontSize:'第二行字号',lineHeight:'行高',padding:'内边距',radius:'圆角',opacity:'底板不透明度',strokeOpacity:'边框不透明度',gap:'中英文间隔',maxHeight:'最大画面占比 %'};
function sync(options=true){
 $('layers').replaceChildren();for(const[key,value]of Object.entries(names)){const b=document.createElement('button');b.className='layer'+(key===layer?' active':'');b.innerHTML=`<span class="dot"></span><span>${value}<small>${key==='chapter'?'中英文 · 原生锚点':key==='objective'?'任务事件 · 自适应框体':'轨道分段 · 底部布局'}</small></span>`;b.onclick=()=>{layer=key;$('entrySearch').value='';sync();};$('layers').append(b);}
 if(options)entryOptions();$('layerTitle').textContent=names[layer];$('scopeBadge').textContent=scope==='global'?'全局默认':'单条特调';
 $('scopeGlobal').classList.toggle('active',scope==='global');$('scopeEntry').classList.toggle('active',scope==='entry');
 $('scopeHint').textContent=scope==='global'?'调整当前图层的默认样式；预览文案不会改游戏。':`只覆盖 ${id()}，其余条目继续继承全局。`;
 $('entryStatus').textContent=config.overrides?.[id()]?'此条目已有覆盖，可导出并导入游戏。':'此条目沿用原文与全局样式。';
 const [first,second]=labelText();$('primaryText').value=first;$('secondaryText').value=second;
 $('secondaryText').hidden=$('secondaryLabel').hidden=layer!=='chapter';
 const s=resolve(config,layer,id());$('controls').replaceChildren();
 const toggle=document.createElement('label');toggle.className='inline';toggle.style.margin='8px 0';const check=document.createElement('input');check.type='checkbox';check.checked=config.layers[layer].enabled;check.onchange=()=>{checkpoint();config.layers[layer].enabled=check.checked;persist();draw();};toggle.append(check,' 启用此图层（全局）');$('controls').append(toggle);
 for(const key of Object.keys(limits)){if(key==='secondaryFontSize'||key==='gap'){if(layer!=='chapter')continue;}
  const[min,max]=limits[key],el=document.createElement('div');el.className='control';const row=document.createElement('div');row.className='param';const title=document.createElement('label');title.textContent=labels[key];
  const num=document.createElement('input');num.type='number';num.min=min;num.max=max;num.step=['x','y','fontSize','secondaryFontSize'].includes(key)?.1:max<=2?.01:1;num.value=s[key]??(key==='gap'?3:key==='secondaryFontSize'?8.5:min);num.setAttribute('aria-label',labels[key]);row.append(title,num);
  const slider=document.createElement('input');slider.type='range';slider.min=min;slider.max=max;slider.step=num.step;slider.value=num.value;slider.setAttribute('aria-label',labels[key]);
  let editing=false;slider.onpointerdown=()=>{checkpoint();editing=true;};slider.oninput=()=>{if(!editing)checkpoint();targetStyle()[key]=Number(slider.value);num.value=slider.value;persist();draw();};slider.onchange=()=>{editing=false;};num.onchange=()=>change(key,Math.max(min,Math.min(max,Number(num.value))));el.append(row,slider);$('controls').append(el);
 }
 const alignment=document.createElement('select');alignment.setAttribute('aria-label','文字对齐');for(const[value,label]of[['left','左对齐'],['center','居中'],['right','右对齐']])alignment.add(new Option(label,value));alignment.value=s.align;alignment.onchange=()=>change('align',alignment.value);$('controls').append(alignment);
 $('colors').replaceChildren();for(const[key,title]of[['color','文字'],['secondaryColor','第二行'],['background','底板'],['stroke','边框']]){
  if(key==='secondaryColor'&&layer!=='chapter')continue;const row=document.createElement('div');row.className='color-row';const label=document.createElement('label');label.textContent=title;const picker=document.createElement('input');picker.type='color';picker.value=s[key]||'#ffffff';picker.setAttribute('aria-label',title+'颜色');
  const value=document.createElement('input');value.type='text';value.value=picker.value;value.setAttribute('aria-label',title+'十六进制颜色');picker.onchange=()=>change(key,picker.value);value.onchange=()=>{if(/^#[\da-f]{6}$/i.test(value.value))change(key,value.value);else notice('颜色格式为 #RRGGBB',true);};row.append(label,picker,value);$('colors').append(row);
 }
 $('overrideCount').textContent=Object.keys(config.overrides||{}).length;draw();
}
function draw(guides=$('guides').checked){if(!ready)return;ctx.clearRect(0,0,canvas.width,canvas.height);ctx.drawImage(image,0,0,canvas.width,canvas.height);const rect=gameRect(canvas.width,canvas.height);bounds={};
 if(guides){ctx.save();ctx.strokeStyle='#80dbe35c';ctx.setLineDash([6,7]);ctx.strokeRect(rect.x,rect.y,rect.w,rect.h);ctx.beginPath();ctx.moveTo(rect.x+rect.w/2,rect.y);ctx.lineTo(rect.x+rect.w/2,rect.y+rect.h);ctx.moveTo(rect.x,rect.y+rect.h/2);ctx.lineTo(rect.x+rect.w,rect.y+rect.h/2);ctx.stroke();ctx.restore();}
 for(const name of Object.keys(names)){if(name!==layer&&!$('allLayers').checked)continue;const[first,second]=labelText(name);bounds[name]=card(ctx,resolve(config,name,selected[name]),first,second,rect,guides&&name===layer);}
 const s=resolve(config,layer,id());$('selectionInfo').textContent=`${id()} · X ${s.x.toFixed(2)}% / Y ${s.y.toFixed(2)}% · ${scope==='entry'?'单条覆盖':'全局默认'}`;
}
async function background(source){const next=new Image();next.src=source;await next.decode();image=next;const ratio=image.width/image.height;const max=1920;canvas.width=Math.round(Math.min(image.width>1000?image.width:1920,max));canvas.height=Math.round(canvas.width/ratio);ready=true;$('dimensions').textContent=`${image.width} × ${image.height} · 横屏比例 ${ratio.toFixed(2)}`;fit();draw();}
function fit(){const box=$('dropZone').getBoundingClientRect(),ratio=canvas.width/canvas.height;let w=Math.min(box.width-40,(box.height-40)*ratio);canvas.style.width=Math.max(100,w)+'px';canvas.style.height=Math.max(100,w)/ratio+'px';}
function point(e){const box=canvas.getBoundingClientRect();return{x:(e.clientX-box.left)*canvas.width/box.width,y:(e.clientY-box.top)*canvas.height/box.height};}
canvas.onpointerdown=e=>{const p=point(e),b=bounds[layer];if(!b||p.x<b.x||p.x>b.x+b.w||p.y<b.y||p.y>b.y+b.h)return;checkpoint();const s=resolve(config,layer,id());drag={p,x:s.x,y:s.y};canvas.setPointerCapture(e.pointerId);canvas.classList.add('dragging');};
canvas.onpointermove=e=>{if(!drag)return;const p=point(e),r=gameRect(canvas.width,canvas.height);targetStyle().x=Math.max(0,Math.min(100,drag.x+(p.x-drag.p.x)/r.w*100));targetStyle().y=Math.max(0,Math.min(100,drag.y+(p.y-drag.p.y)/r.h*100));draw();};
canvas.onpointerup=e=>{if(!drag)return;drag=null;canvas.classList.remove('dragging');persist();sync(false);};canvas.onpointercancel=canvas.onpointerup;
window.addEventListener('keydown',e=>{if(/INPUT|TEXTAREA|SELECT/.test(document.activeElement.tagName))return;if((e.ctrlKey||e.metaKey)&&e.key.toLowerCase()==='z'){e.preventDefault();undo(e.shiftKey);return;}
 if(!['ArrowLeft','ArrowRight','ArrowUp','ArrowDown'].includes(e.key))return;e.preventDefault();checkpoint();let s=resolve(config,layer,id()),step=e.shiftKey?1:.1;if(e.key==='ArrowLeft')s.x-=step;if(e.key==='ArrowRight')s.x+=step;if(e.key==='ArrowUp')s.y-=step;if(e.key==='ArrowDown')s.y+=step;Object.assign(targetStyle(),{x:Math.max(0,Math.min(100,s.x)),y:Math.max(0,Math.min(100,s.y))});persist();sync(false);});
function download(blob,name){const url=URL.createObjectURL(blob),a=document.createElement('a');a.href=url;a.download=name;a.click();setTimeout(()=>URL.revokeObjectURL(url),1000);}
$('scopeGlobal').onclick=()=>{scope='global';sync(false);};$('scopeEntry').onclick=()=>{scope='entry';sync(false);};$('entrySearch').oninput=()=>{entryOptions();sync(false);};$('entryId').onchange=()=>{selected[layer]=$('entryId').value;sync(false);};
for(const key of['primaryText','secondaryText'])$(key).oninput=()=>{draft[id()]=[$('primaryText').value,$('secondaryText').value];draw();};
$('saveEntry').onclick=()=>{if(!$('primaryText').value.trim())return notice('请保留非空文案；隐藏整层使用全局开关。',true);checkpoint();config.overrides??={};config.overrides[id()]??={};config.overrides[id()].text=$('primaryText').value;if(layer==='chapter'&&$('secondaryText').value.trim())config.overrides[id()].secondaryText=$('secondaryText').value;scope='entry';delete draft[id()];persist();sync();notice('已保存单条覆盖，应用后仅影响 '+id());};
$('clearEntry').onclick=()=>{checkpoint();delete config.overrides?.[id()];delete draft[id()];persist();sync();};$('reset').onclick=()=>{checkpoint();if(scope==='entry')delete config.overrides?.[id()];else config.layers[layer]=structuredClone(defaults.layers[layer]);persist();sync();};
$('undo').onclick=()=>undo();$('redo').onclick=()=>undo(true);$('guides').onchange=()=>draw();$('allLayers').onchange=()=>draw();$('toggleSidebars').onclick=()=>{document.body.classList.toggle('hide-sidebars');fit();};
$('background').onchange=()=>background($('background').value);$('importImage').onclick=()=>$('imageFile').click();$('imageFile').onchange=e=>{if(e.target.files[0])background(URL.createObjectURL(e.target.files[0])).then(()=>notice('已导入截图；如方向不对，可使用 90° 旋转。')).catch(err=>notice(err.message,true));};
async function rotate(direction){const c=document.createElement('canvas');c.width=image.height;c.height=image.width;const x=c.getContext('2d');x.translate(c.width/2,c.height/2);x.rotate(direction*Math.PI/2);x.drawImage(image,-image.width/2,-image.height/2);await background(c.toDataURL('image/png'));}
$('rotateL').onclick=()=>rotate(-1);$('rotateR').onclick=()=>rotate(1);$('exportPng').onclick=()=>{draw(false);canvas.toBlob(b=>download(b,'DeadSpace-overlay-preview.png'));draw();};
$('exportConfig').onclick=()=>{try{download(new Blob([JSON.stringify(validate(config),null,2)],{type:'application/json'}),'overlay-style.json');}catch(e){notice(e.message,true);}};
$('importConfig').onclick=()=>$('configFile').click();$('configFile').onchange=async e=>{try{let file=e.target.files[0];if(file.size>1048576)throw Error('配置不能超过 1MB');checkpoint();config=validate(JSON.parse(await file.text()));draft={};persist();sync();notice('已导入配置，包括 '+Object.keys(config.overrides||{}).length+' 条特调。');}catch(err){notice(err.message,true);}};
for(const event of['dragenter','dragover'])$('dropZone').addEventListener(event,e=>{e.preventDefault();$('dropZone').classList.add('dragover');});$('dropZone').ondragleave=()=>$('dropZone').classList.remove('dragover');$('dropZone').ondrop=e=>{e.preventDefault();$('dropZone').classList.remove('dragover');const file=e.dataTransfer.files[0];if(file?.type.startsWith('image/'))background(URL.createObjectURL(file));};
async function api(path,body){const response=await fetch('/api/'+path,{method:body?'POST':'GET',headers:body?{'Content-Type':'application/json'}:{},body:body?JSON.stringify(body):undefined});const value=await response.json();if(!response.ok)throw Error(value.error||response.statusText);return value;}
$('capture').onclick=async()=>{try{notice('正在截取当前平板画面…');const value=await api('capture',{});await background(value.url);notice('已读取当前实机截图，没有操作游戏。');}catch(e){notice(e.message,true);}};
$('apply').onclick=async()=>{try{$('apply').disabled=true;const value=await api('apply',{target:$('target').value,config:validate(config)});notice(value.message);$('saveState').textContent='已应用 · '+new Date().toLocaleTimeString();}catch(e){notice(e.message,true);}finally{$('apply').disabled=false;}};
new ResizeObserver(fit).observe($('dropZone'));await document.fonts.load('14px GameText');await background('assets/corridor.jpg');sync();notice('已使用角色稳定的游玩截图。可先调全局，再为指定 ID 单独覆盖。');
try{const status=await api('status');$('connection').textContent=status.connected?'平板已连接':'平板未连接';}catch{$('connection').textContent='预览模式';notice('编辑和导出可用；应用到游戏需用 Start-Studio.ps1 启动本地服务。');}
